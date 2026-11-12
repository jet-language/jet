# Concurrency

Tasks, channels, cancellation, parallel work and services.

Run a lesson from the repository root, for example `jet run Examples/Concurrency/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [AllFailFast](AllFailFast.jet) — All fail-fast cancels siblings when one task panics.
- [CancelCleanup](CancelCleanup.jet) — Cancellation is preemptive at wait points.
- [ChannelBuiltin1560](ChannelBuiltin1560.jet) — channel<T>() is a builtin value with no import.
- [ContextTaskInherit](ContextTaskInherit.jet)
- [DeadlineContext](DeadlineContext.jet)
- [DetachedTask](DetachedTask.jet) — Fire-and-forget task with `.detach()`.
- [DeterministicWorld](DeterministicWorld.jet)
- [EventLifecycle](EventLifecycle.jet) — Synchronous Event registration order, priority, once, unsubscribe, scope cancellation and scope teardown, on every tier.
- [FreezeCapture](FreezeCapture.jet) — `freeze` makes one owned snapshot that any task may read.
- [NativeResourceStreams](NativeResourceStreams.jet) — Channel, timer and file-line streams stay native.
- [Protocol](Protocol.jet) — Declare the exchange once; compiler generates ordered Payment.Client / Payment.Server handles over #SingleUse + typestate (E0150 on out-of-order send/recv).
- [RaceCancel](RaceCancel.jet) — Race returns first success; losers cancel at runtime.
- [ScopedBorrowBands](ScopedBorrowBands.jet) — A task group joins every child, so a child may borrow a place its owner still gives access.
- [SelectChannel](SelectChannel.jet) — A readiness table waits on plain endpoints.
- [SharedStaleRevision](SharedStaleRevision.jet) — A publication must match the committed Shared revision it observed; the older result is rejected without overwriting new.
- [ShieldCommit](ShieldCommit.jet) — Cancellation is deferred inside `#Shield`.
- [SyncCRDT](SyncCRDT.jet) — Shipped CRDT values and the database row policy check.
- [TaskAllNamed](TaskAllNamed.jet) — Positional and named `task.all` run the same branches the same way; the named form returns a record of branch names.
- [TaskBlocked](TaskBlocked.jet) — A detached child parked on a full channel is named at the shared process-exit boundary.
- [TaskCPUClock](TaskCPUClock.jet) — Each task has its own CPU clock, which advances only while the task runs.
- [TaskControls](TaskControls.jet) — Coroutine-grade control via task handles (internal substrate).
- [TaskGroup](TaskGroup.jet) — Structured `task.group` with nested combinators.
- [TaskGroupParameter](TaskGroupParameter.jet) — Pass a task group to a named helper.
- [TaskPanic](TaskPanic.jet) — A joined child keeps its panic text and task identity on the TaskFailure rail.
- [Tasks](Tasks.jet)
- [TasksJoinAll](TasksJoinAll.jet) — A bound `Task` handle owes `join`.
- [TaskStateDuty](TaskStateDuty.jet) — Task state and join duty are compiler facts.
- [TaskTimeoutDeadline](TaskTimeoutDeadline.jet)

## Parallel

- [BoundedWorkers](Parallel/BoundedWorkers.jet)
- [ParallelIter](Parallel/ParallelIter.jet) — Explicit parallel collection adapters.
- [ParallelScan](Parallel/ParallelScan.jet) — Parallel file scan.
- [ParallelScanExpert](Parallel/ParallelScanExpert.jet) — Expert variant of ParallelScan.jet.
- [Pipeline](Parallel/Pipeline.jet)

## Services

- [JobQueue](Services/JobQueue.jet) — One typed job flows through the durable queue lifecycle.
- [ServiceAuthority](Services/ServiceAuthority/) — Durable receipts use typed endpoint identity keys.
- [ServicePartitionReconcile](Services/ServicePartitionReconcile.jet)
- [ServiceProjection](Services/ServiceProjection.jet) — Runtime retention and delivery policy are explicit values.
- [ServiceTree](Services/ServiceTree.jet) — A typed tree promotes an ordinary function into a named worker.
- [ServiceWorkflowOutcome](Services/ServiceWorkflowOutcome.jet)
