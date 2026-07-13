# Epoch 3 pillar — async networking & Go-scale concurrency

**Status:** Linux runtime internals shipped under Tower #126; Linux Core
wait-point conformance is audited under #306. Windows-native lifecycle and CI
proof is tracked only by #527 (updated 2026-07-12).

The intended Windows backend uses IOCP directly: sockets are associated with one completion port,
zero-byte overlapped `WSARecv`/`WSASend` operations park without consuming data,
and registration/cancellation control packets wake the port. Monotonic completion
keys reject stale packets; `CancelIoEx` retires pending operations on task cancel,
deadline, or scope exit. #527 must prove this on a real Windows lane; emitted-code
presence is not platform proof. No portable polling may run on the Windows native path.
It now owns the Go-scale M:N runtime, native parkers, scoped combinators, select,
deadlines/cancellation integration, observability, and scale proof. Former
standalone cards #36 and #103 are merged into #126.

## Goal

Ship a Go-scale **M:N green-thread runtime under Jet's existing task/channel
model** (D-ASYNCRT1=A) so Jet services can handle **100k+ idle connections** and
CPU-heavy concurrent workloads without function coloring, memory-safety holes, or
rustc-facing diagnostics.

The target is async/await ergonomics without async/await function coloring:
ordinary reads, writes, channel waits, timers, and task joins may park the Jet
task. The runtime resumes that task when the operation is ready.

## Epoch 2 vs Epoch 3

| | Epoch 2 (ships) | Epoch 3 (this doc) |
|---|---|---|
| Model | S53 **tasks + channels** (E2-V5) | Integrated async runtime |
| Scale target | Internal services, thousands of connections | Public APIs, 100k+ websockets |
| Syntax | blocking I/O + worker tasks | same surface; blocking-looking I/O parks Jet tasks |
| Honest line | "Go circa 2012" | "Go-class, but typed + safe" |

Epoch 2 did not block on this pillar. Epoch 3 upgrades the runtime under the
same user model.

## Likely building blocks

- M:N task scheduler under `task { }` and channels (D-ASYNCRT1=A).
- Platform readiness backend for task parking (D-MNIO1=A).
- Structured-concurrency task scope (`taskgroup`, D-NURSERY1/D-TASKSCOPE1 shipped).
- Task-scope combinators: all/race/any (D-CONCCOMB1, D-RACEWIN1=A; built under #126).
- First-ready event selection for channels/timers/I/O (D-CONCSELECT1=A, fluent `g.select().recv(...).after(...).wait()?`).
- Deadline propagation through task context (D-DEADLINE1=A, shipped foundation).
- Expert-visible runtime controls: detached-task audit, scheduler/poller metrics,
  task names, worker/poller tuning, and fairness policy.
- Integration with `Fallible` + `?` (no callback-colored types in user surface).
- TLS and DB drivers remain FFI-bridged; async wraps blocking bridges first,
  native async I/O later.

## Canonical build notes

- Do not split `race`/`all`/`any`, try-both/keep-winner, select, or cancellation
  into separate implementation cards. They are one scoped-concurrency push under #126.
- Do not add `@async`/`await`, function coloring, or a user coroutine primitive.
  D-ASYNCRT1 and D-COROUTINE1 keep ordinary code + task handles as the one path.
- Build order: native parker abstraction, stdlib wait-point routing, scoped
  combinator correctness, if-fused select, observability/misuse diagnostics, then
  scale/perf proof.

## Non-goals

- No `@async`/`await` function coloring.
- No `unsafe` callback bridges in user code.
- No unstructured task leaks as the beginner path.

See also: [`concurrency-vision.md`](concurrency-vision.md).
