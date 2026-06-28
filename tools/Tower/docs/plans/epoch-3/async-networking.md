# Epoch 3 pillar — async networking & Go-scale concurrency

**Status:** owner-ratified direction (2026-06-16, D-NET2). **Epoch 3 pillar.**

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
- Platform readiness backend for task parking (D-MNIO1 open).
- Structured-concurrency task scope (D-NURSERY1 open; spelling in D-TASKSCOPE1).
- Task-scope combinators: all/race/any (D-CONCCOMB1, D-RACEWIN1=A).
- First-ready event selection for channels/timers/I/O (D-CONCSELECT1 open).
- Deadline propagation through task context (D-DEADLINE1 open).
- Expert-visible runtime controls: detached-task audit, scheduler/poller metrics,
  task names, worker/poller tuning, and fairness policy.
- Integration with `Fallible` + `?` (no callback-colored types in user surface).
- TLS and DB drivers remain FFI-bridged; async wraps blocking bridges first,
  native async I/O later.

## Open design questions

- Exact scoped task-group syntax/API (D-TASKSCOPE1).
- Whether user-facing coroutines exist or remain internal substrate (D-COROUTINE1).
- Deadline propagation carrier (D-DEADLINE1).
- OS readiness backend (D-MNIO1).
- Whether structured `select` joins the combinator set (D-CONCSELECT1).

## Non-goals

- No `@async`/`await` function coloring.
- No `unsafe` callback bridges in user code.
- No unstructured task leaks as the beginner path.

See also: [`concurrency-vision.md`](concurrency-vision.md).
