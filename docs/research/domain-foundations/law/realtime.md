# realtime

## Ratified

- **D-MEM-FACTS1=B** — “`!Mem.Alloc`, `!Mem.Rc`, and `!Mem.Alloc(above: N)` are effect-row prohibitions”; each prohibition checks “every reachable call, including dependencies,” and reports source, full call path, denial spelling, and provenance. `open-world dispatch cannot prove a strict fact`. — `docs/spec/syntax-decisions.md:2057-2062`
- **D-MEM1 / D-MEM-PARAM1=A** — “unmarked = read,” `&T = exclusive write`, `^T = take`; `Shared<T>` is a named crossing escape, and scoped memory facts use `Mem.*` effect denials. — `docs/spec/syntax-decisions.md:2189-2204`
- **D-CONC-STM1=A** — “a transaction body runs exactly once”; participants acquire write locks in stable order, buffered changes apply, locks release, and “contention waits instead of retrying.” — `docs/spec/syntax-decisions.md:2394-2398`
- **D-CONC-CHAN1=A** — `channel<T>()` is builtin; `loop value in receiver` drains until close; `Receiver<T>` and `Sender<T>` are nameable; `after` takes a Duration; cancellation remains D-CANCELMODEL1. — `docs/spec/syntax-decisions.md:2416-2423`
- **D-CONC-SHARE1=A** — `shared expr` builds the cell; ordinary field access is used; “each statement is one atomic step”; several steps commit under `#Transact`; lock order and crossing safety remain checked. — `docs/spec/syntax-decisions.md:2425-2431`
- **D-CANCELMODEL1=C / D-SHIELDNAME1=A** — cancellation is “preemptive at wait points” (channel receive/send, sleep, join, select, I/O); the cancelled task unwinds and runs cleanup. `#Shield { … }` defers, never discards, the unwind until the region exits. — `docs/spec/syntax-decisions.md:2481-2491`
- **D-CONC-SCHED1=A** — the schedule value is `Duration` or wall-clock time; a scheduled `#Job` is the lifecycle unit, while `task` remains structured concurrency. — `docs/spec/syntax-decisions.md:2400-2407`
- **D-SCHEDULE1=A** — `#Every(…)` is a marker on `#Job fn`; one declaration feeds `jet dev`, services, and jetos. Complex calendars, timezones, and jitter remain runtime/jetos concerns. — `docs/spec/syntax-decisions.md:6190-6206`
- **D-GAME-ASSET1 / D-GAME-ECS1 / D-GAME-INPUT1 / D-GAME-REPLAY1 / D-GAME-BACKEND1 / D-GAME-BUDGET1** — the Core floor is headless; `game.run(scene, replay: replay)` produces a “deterministic transcript without renderer/audio/editor dependencies,” while renderer, audio, editor, and native asset I/O are replaceable package layers. — `docs/spec/syntax-decisions.md:3391-3402`
- **D-PERFBUDGET-SURFACE1=A / D-PERFBUDGET-BASELINE1=A** — budgets are typed facts under `module perf.<role>` and all statistical baselines are pinned; “missing, mismatched, stale, zero, or unavailable evidence never silently passes.” — `docs/spec/performance-budget-decisions.md:7-11`
- **D-PERFBUDGET-GRAMMAR1=A** — `FrameTime` is listed among lower-is-better metrics; the law defines typed comparison/enforcement, not a WCET or ISR contract. — `docs/spec/performance-budget-decisions.md:25-53`

## Shipped

- `core.time` exposes `Duration`, `Duration.nanoseconds`, `Instant`, `time.sleep(Duration)`, and `#Context`-aware E3003 behavior; `time.now()` is milliseconds and `sleep` is a blocking call. — `docs/reference/core-library.md:2236-2277`
- The runtime adapter converts Duration nanoseconds to integer milliseconds (`nanos.saturating_div(1_000_000)`) before deadline and sleep calls. This is evidence that sub-millisecond scheduling is not a ratified realtime guarantee. — `crates/jet-codegen/src/Prelude/CoreLib/Top/TimeSleep.rs:53-68`
- Deadline and wait-point behavior is exercised by `examples/features/concurrency/deadline_context.jet`; cancellation and cleanup by `examples/features/concurrency/cancel_cleanup.jet`; typed channel drain by `examples/features/concurrency/pipeline.jet`.
- `core.game` ships `on_frame`, replay, `Backend.headless()`, and a three-frame headless budget; the reference API says runtime does not skip/reschedule work. — `docs/reference/core-library.md:2043-2088`
- `core.perf` exposes a runtime fidelity signal, but adaptive providers and automatic adaptive scheduling do not ship in Epoch 3. — `docs/reference/core-library.md:2092-2119`

## Undecided

- Whether Jet needs a fixed-rate callback or absolute-period API for audio/control loops, including ISR binding, jitter/overrun behavior, priority, and callback lifetime.
- Whether nanosecond or absolute waits should be observable scheduler guarantees rather than Duration inputs projected to milliseconds; `time.sleep`'s truncation behavior has no stronger ruling.
- Whether `Shared`/channels need an atomic or lock-free shared-state API, and what memory ordering, sendability, and `!Mem.Alloc` / `!Time` effect facts it would carry.
- Whether CPU work (as opposed to a wait point) has a deadline/WCET contract, and what static evidence, runtime enforcement, or receipt would prove it.

## Conflicts

- D-SCHEDULE1 ratifies `#Every` for `#Job` declarations, not a fixed-rate callback, ISR, or hard-realtime scheduler. It explicitly leaves complex cadence to runtime/jetos layers.
- D-CANCELMODEL1 and D-DEADLINE1 make cancellation/deadline observation preemptive at wait points. They do not authorize interruption of a CPU-bound callback or prove WCET.
- D-CONC-STM1 and D-CONC-SHARE1 specify ordered locks and atomic statements. A ballot for lock-free `Shared` semantics would be a new mechanism, not an implementation of the current Shared law.
- D-GAME-BACKEND1's default headless run is a deterministic three-frame transcript with no audio or renderer; it is not a playable realtime loop.
- The performance law includes `FrameTime`, but it does not define WCET. The same document says “no perf namespace or BudgetSpec/Report implementation exists,” while the reference inventory and `core.perf` section list that surface; reconcile this documentation/implementation conflict before treating it as a new realtime primitive. — `docs/spec/performance-budget-decisions.md:63-67`; `docs/reference/core-library.md:2092-2119,4281-4296`
