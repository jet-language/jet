# lifecycle

## Ratified

- **D-CANCELMODEL1=C / D-SHIELDNAME1=A** — cancellation is “preemptive at wait points”; a cancelled task unwinds at its next channel receive/send, `time.sleep`, join, select, or I/O, runs Drop cleanup, and `#Shield { … }` defers rather than discards the unwind. — `docs/spec/spec.md:2587-2604`
- **D-DEADLINE1** — `#Context(deadline: <Int epoch_ms>)` gives an ambient deadline; wait/IO points observe it, including joins, channel receive, `time.sleep`, and TCP read/write, and expiry emits E3003. — `docs/spec/spec.md:2664-2668`
- **D-CONC-STREAM1=A** — dropping a stream iterator cancels its producer; the producer unwinds at its next wait point and runs cleanup; `yield` is a wait point and drop-close is cancellation. — `docs/spec/syntax-decisions.md:2409-2414`
- **D-CONC-CHAN1=A** — `channel<T>()` is builtin, receivers drain until close, endpoints are nameable, and `after` takes a Duration; cancellation remains D-CANCELMODEL1. — `docs/spec/syntax-decisions.md:2416-2423`
- **D-CONC-SPAWN1=D** — the task family is `task`, `task.all`, `task.race`, `task.any`, and `task.group`; scope-end joins, fail-fast, cancel-losers, first-`Ok`, and same-completion source-order tie-break laws remain. — `docs/spec/syntax-decisions.md:2433-2438`
- **D-CONC-SCHED1=A** — a scheduled `#Job` is the lifecycle unit; `task` remains the structured-concurrency construct; service supervisors, groups, and restart data belong to the service plane. — `docs/spec/syntax-decisions.md:2400-2407`
- **D-PROCESS1** — `process.run` executes checked `Sh` argv directly and does not invoke a shell; `ProcessSpec` has typed `stdin`, stream modes, timeout, output/resource limits, and `pipeline`. — `docs/reference/core-library.md:1481-1545`
- **D-FAIL-EXIT1** — `process.exit`/`os.stop` use one explicit-stop boundary: deferred closes run in reverse declaration order, guards in reverse registration order, and `atexit` handlers in registration order; host kill/abort is outside this law and skips cleanup. — `docs/reference/core-library.md:1620-1633`
- **D-OSFACTS1** — `on_interrupt(handler)` is a process-lifetime Ctrl-C/SIGINT registration; handlers are additive, run in registration order, and there is no unregister/drop handle. — `docs/reference/core-library.md:1403-1455,1469-1474`

## Shipped

- `#Context` deadline behavior: `examples/features/concurrency/deadline_context.jet`; cancellation/cleanup and race loser behavior: `examples/features/concurrency/cancel_cleanup.jet`; channel close/drain: `examples/features/concurrency/pipeline.jet` and `examples/features/concurrency/channel_builtin_1560.jet`.
- `ProcessSpec`/child controls, timeout, output limits, pipeline, spawn, stdout, and `child.wait()` are exercised by `examples/features/io/process_builder.jet`. `process.exit`, `atexit`, scope guards, deferred close, and cleanup ordering are exercised by `examples/features/io/process_exit_cleanup.jet`.
- `ProcessChild` exposes `wait`, `exited`, `kill`, `terminate`, `interrupt`, a `.stdin` writer (`child.stdin.write(text)`), and stdout/stderr line streams. The compiler maps the Jet-level `ProcessStdin` marker as an internal handle shape. — `docs/reference/core-library.md:1730-1741`; `crates/jet-codegen/src/Codegen/Context.rs:712-722`
- Process wall-time limits stop the full child tree and close streams before the typed limit error; pipeline stages keep their own declared controls. — `docs/reference/core-library.md:1531-1545`
- HTTP graceful shutdown is shipped for HTTP/1.x and HTTP/2 over cleartext or TLS: `shutdown(grace)` stops accepts, sends GOAWAY where applicable, drains active work, cancels stragglers, refuses new requests/streams, and reports bounded counts. — `docs/reference/core-library.md:769-770`
- `on_interrupt` is shipped for Ctrl-C/SIGINT on Unix and Windows; POSIX `kill(pid, sig)` exists only in an audited `#Unsafe`/OS-gated surface. — `docs/reference/core-library.md:1452-1474`

## Undecided

- Whether SIGTERM and other process signals should enter the task cancellation/deadline model, and if so how signal ownership, escalation, and cleanup are defined.
- Whether `ProcessStdin` needs an explicit close/EOF operation, what writes after close do, and how stdin closure is represented in receipts and pipeline stages.
- Whether server request timeout is a public middleware/handler contract, how it maps to `#Context` and cancellation, and whether graceful shutdown's bounded grace is the same or a separate budget.
- Whether inter-process typed channels are needed beyond in-process `channel<T>` and OS-pipe-backed `process.pipeline`, including serialization, backpressure, authority, and failure semantics.

## Conflicts

- D-CANCELMODEL1 and D-DEADLINE1 observe only wait/IO points. A CPU-bound task or callback is not made preemptible merely by `#Context`, cancellation, or `#Shield`.
- `on_interrupt` is explicitly Ctrl-C/SIGINT only, with no unregister handle. A ballot claiming shipped SIGTERM propagation or general signal-to-cancellation behavior would exceed D-OSFACTS1.
- `ProcessSpec.timeout` is a child-tree wall-time/resource limit, not a server request-timeout middleware. Do not collapse those surfaces without a new ruling.
- `channel<T>` is the in-process typed channel law; `pipeline` connects ordinary process streams. Neither current ruling supplies typed inter-process transport.
- Explicit-stop cleanup is not guaranteed for a host kill/abort. Any proposal that promises cleanup after SIGKILL or an equivalent host abort conflicts with D-FAIL-EXIT1.
- `#Every` schedules `#Job` lifecycle units; it does not provide signal handling, request timeout, or cancellation propagation beyond the existing wait-point model. — `docs/spec/syntax-decisions.md:6190-6206`
