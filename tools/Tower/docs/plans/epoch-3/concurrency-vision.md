# Epoch 3 Concurrency Vision

**Status:** reconciled 2026-06-28 after owner questions on c101/c102.

This is the cohesive end-state if the recommended open ballots are ratified.
Exact surface spelling remains owner-controlled; examples show the intended shape,
not a ratified syntax change. `nursery` is now treated as borrowed Trio jargon,
not the assumed final Jet keyword; D-TASKSCOPE1 owns the spelling.

## Thesis

Jet should feel like Go for beginners, like Trio for task lifetime, like Verse for
race/all logic, like Tokio for peak networking, and like Rust for safety. The
one-line user promise:

> Write normal code. Spawn scoped tasks when you want parallelism. Blocking-looking
> I/O parks the Jet task, not an OS thread. Scope exit proves the work is done,
> cancelled, or reported.

## One Model

Jet's concurrency model is:

1. **Ordinary code is the async surface.** A call such as `socket.read()?` may
   suspend the current Jet task, but the function is not colored. No caller has
   to become `async`.
2. **Tasks + channels are the user primitives.** Users write `task { }`, pass
   messages through typed channels, and collect results through task handles.
3. **M:N green threads are the runtime model** (D-ASYNCRT1=A). A Jet task parks
   on I/O, timers, channel waits, cancellation, or deadlines without pinning an
   OS thread.
4. **Scoped task groups are the default lifetime model** (D-NURSERY1 open, rec
   A; spelling in D-TASKSCOPE1). A lexical scope owns child tasks. Scope exit
   joins, cancels, and reports errors.
5. **Race/all/any are task-scope combinators** (D-CONCCOMB1, D-RACEWIN1=A). They
   do not create a second spawn mechanism.
6. **Deadlines flow through task context** (D-DEADLINE1 open, rec A). A parent
   sets the budget once; I/O and wait points observe it automatically.
7. **Coroutines stay internal by default** (D-COROUTINE1 open, rec B). The
   scheduler may be coroutine-based, but users do not learn a second primitive.
8. **OS readiness is runtime plumbing** (D-MNIO1 open, rec A). Linux
   io_uring/epoll, kqueue, and IOCP sit below the same user code.

## Beginner Surface

A beginner server should look synchronous and still scale:

```jet
fn handle(conn: net.Conn) -> () ? NetError {
    loop {
        msg @= conn.read_text()?        // parks this Jet task
        reply @= route(msg)?
        conn.write_text(reply)?         // parks again if socket is not ready
    }
}

fn main() -> () ? NetError {
    listener @= net.listen(":8080")?

    taskgroup g {
        loop {
            conn @= listener.accept()?  // one OS poller, not one OS thread
            g.task { handle(conn) }
        }
    }
}
```

The beginner does not see epoll, futures, pinning, callbacks, `async fn`, or
manual cancellation plumbing. A 100k idle WebSocket server is task-per-connection;
idle connections consume parked task state and file descriptors, not stacks of
blocked OS threads.

## Async/Await Ergonomics Without Function Coloring

Jet should provide the thing programmers like about async/await: sequential code
that can pause. It should reject the part they dislike: colored call graphs.

```jet
fn profile(id: UserId) -> Profile ? ServiceError {
    taskgroup g {
        user_h @= g.task { users.fetch(id) }
        bill_h @= g.task { billing.fetch(id) }
        flag_h @= g.task { flags.fetch(id) }

        user @= user_h.wait()?
        bill @= bill_h.wait()?
        flag @= flag_h.wait()?

        Profile.{ user, bill, flag }
    }
}
```

`wait()` is an explicit join point for task handles. Ordinary I/O calls can also
park. No `await` keyword is required because suspension is a property of Jet
tasks, not a property of function types.

## Verse-Style Race Logic

Race is a structured task-group operation. The winner is returned; losing siblings
are cancelled by the scope.

```jet
fn fetch_fast(path: String) -> Bytes ? NetError {
    taskgroup g {
        g.race([
            g.task { cdn_a.get(path) },
            g.task { cdn_b.get(path) },
            g.task { origin.get(path) },
        ])?
    }
}
```

The combinator set should cover the common cases:

```jet
g.all(tasks)?       // wait for all; fail fast and cancel siblings on error
g.race(tasks)?      // first success wins; cancel losers
g.any(tasks)?       // first completed result wins; success or error is visible
g.select(cases)?    // first channel/timer/I/O event wins; D-CONCSELECT1 open
```

The important rule is I8: all of these compose over one spawn/lifetime model.
There is no separate `race` primitive that can leak work.

## Deadlines And Cancellation

Deadlines are inherited through context so users cannot forget to pass them:

```jet
fn handle_request(req: Request) -> Response ? ServiceError {
    #Context(deadline: time.after(ms: 200)) {
        taskgroup g {
            data_h @= g.task { db.fetch(req.id) }
            auth_h @= g.task { auth.check(req.token) }

            data @= data_h.wait()?   // observes the 200 ms deadline
            auth @= auth_h.wait()?   // same inherited deadline
            render(data, auth)
        }
    }
}
```

If the deadline closes, parked I/O wakes with a Jet diagnostic/result owned by the
front end. rustc never speaks to the user. Cancellation is cooperative at safe
points: I/O waits, channel waits, task joins, timers, and explicit cancellation
checks in long CPU loops.

## Backpressure And Streams

Channels remain the simple backpressure tool:

```jet
fn pipeline(input: Channel<Request>, output: Channel<Response>) -> () ? Error {
    taskgroup g {
        repeat workers.count {
            g.task {
                loop {
                    req @= input.recv()?      // parks until work arrives
                    res @= process(req)?
                    output.send(res)?         // parks when capacity is full
                }
            }
        }
    }
}
```

The channel capacity is a real memory/backpressure bound, not just a queue hint.
Expert APIs can expose fairness policy and overflow policy, but the default stays
typed send/recv with structured cancellation.

## Expert Control

Expert control should be explicit, auditable, and local:

```jet
taskgroup g(policy: .{
    max_tasks: 4096,
    fail: .CancelSiblings,
    priority: .High,
    fairness: .LowLatency,
}) {
    g.task(name: "ingest") { ingest_loop() }
    g.task(name: "flush") { flush_loop() }
}

task.detach(name: "metrics-flush", policy: .Background) {
    metrics.flush_forever()
}
```

Detached work is an escape hatch, not the beginner path. It should be visible in
code review and linted without an explicit policy/name. Runtime experts should be
able to tune worker count, poller backend, priorities, task names, tracing, stack
budgets, and queue fairness without changing the beginner model.

## Devil's Advocate Pass

Hard objections against this proposal:

1. **Hidden suspension can hide latency.** If `db.fetch()` looks like a normal
   call, a reader may not realize it can park, be cancelled, or resume later.
2. **Blocking-looking code can invite blocking implementations.** A stdlib author
   could accidentally call a real OS-blocking API and pin an OS thread.
3. **Implicit deadlines can feel spooky.** A helper can fail because a parent set
   a deadline far away in the call tree.
4. **Structured scopes can feel heavy for one-off parallelism.** If every spawn
   needs a named scope, small examples can look busier than Go.
5. **`nursery` is bad product language for Jet.** It is precise to Trio users but
   obscure to everyone else. It does not read like a systems language primitive.
6. **`race` semantics can surprise.** First-success, first-complete, fail-fast,
   and wait-all are different. Names and diagnostics must keep them distinct.
7. **Expert controls can grow a second runtime surface.** If policy objects,
   detached tasks, custom pollers, and tracing knobs become common in beginner
   code, the model loses its simplicity.
8. **"Better than Go/Tokio" cannot be asserted before benchmarks.** The design
   can target a higher ceiling, but implementation must prove it.

Hardening changes:

- Treat `nursery` as rejected/default-borrowed vocabulary. D-TASKSCOPE1 now owns
  the final spelling; the recommended direction is plain task-group language.
- Make suspension visible in API docs and effects, not function color. Any stdlib
  function that can park must show a `Parks`/`Waits` contract in generated docs
  and must be testable under cancellation.
- Add an internal rule: user-looking blocking I/O APIs must route through the Jet
  parker. Real OS-blocking bridges need an explicit blocking pool or expert mark.
- Require task-scope diagnostics: leaked detached task, forgotten handle wait,
  cancellation swallowed, deadline ignored, and real OS block in scheduler worker.
- Keep expert controls local and named; no global scheduler policy in ordinary
  examples.

## Is Hidden Async Clarity A Problem?

It is a tradeoff, not automatically a bug.

Jet should not hide **concurrency**: `taskgroup`, `task`, channels, `wait`,
`race`, `all`, `any`, `select`, deadline context, and detach points are visible.
Jet does hide **suspension plumbing**: a function does not become `async` just
because it may park on I/O.

That matches successful systems:

- Go: `conn.Read` looks blocking; the runtime parks the goroutine on netpoll.
  Go reached broad industry adoption with no async marker.
- Erlang/BEAM: process receive/send and I/O are concurrent without function
  coloring; the model is actor/process based, not `await` based.
- Java virtual threads: Java deliberately added blocking-looking virtual-thread
  I/O after years of callback/future APIs because ordinary call stacks are easier
  to reason about.
- Ruby fibers/Falcon-style servers and Loom-style runtimes make blocking-looking
  calls yield cooperatively, with adoption in narrower but real production niches.

The risk is not "users cannot see `await`." The risk is "users cannot see
latency, cancellation, or task lifetime." Jet handles that with visible task
creation, visible joins/combinators, visible deadlines, generated API wait
contracts, runtime tracing, and diagnostics.

## Runtime Architecture

The runtime stack should be:

```text
Jet source
  -> parser/sema owns all concurrency diagnostics
  -> task/taskgroup/channel IR operations
  -> M:N scheduler with work stealing
  -> task parking table for I/O, timers, channels, cancellation
  -> platform poller: io_uring/epoll, kqueue, IOCP
  -> generated Rust as hidden verifier/optimizer
```

Performance targets:

- 100k+ idle TCP/WebSocket connections on commodity hardware.
- O(ready events), not O(open connections), wake cost.
- No OS-thread-per-connection fallback in the normal networking path.
- Bounded allocations per parked task; no callback heap cascade.
- Fast path for ready I/O and uncontented channel send/recv.
- Observable runtime: task names, taskgroup tree, deadlines, cancellation reason,
  poller metrics, and per-task wait state.

## Why This Can Beat Existing Systems

| System | Keeps | Jet improves |
|---|---|---|
| Go | Simple blocking-looking code; task-per-connection scale | Structured lifetime by default, typed errors, no forgotten `context` threading |
| Rust/Tokio | High ceiling, native pollers, backpressure tools | No colored function graph, no `Pin`/future ceremony in user code |
| Trio | Best scoped task lifetime model | Native compiled performance, owned syntax/diagnostics, systems-language control |
| Kotlin/Swift | Structured tasks and good cancellation | No suspend-color propagation as the primary user model |
| JavaScript | Familiar sequential async style | Strong cancellation, typed errors, real parallel CPU work, no promise leak culture |
| Erlang/BEAM | Massive concurrency, supervision mindset | Memory-safe native compilation, ownership/effects, explicit expert unsafe gates |
| Verse | `race`/`all` logic as a first-class idea | Integrates race logic with typed task groups, channels, deadlines, and systems I/O |

## What This Rejects

- No `async`/`await` function coloring.
- No unstructured spawn as the beginner default.
- No callback-first networking API.
- No separate coroutine/generator primitive unless the owner later ratifies it.
- No "try rustc and see" checking. Sema validates ownership, captures, effects,
  cancellation surfaces, and diagnostics before codegen.
- No second deadline channel beside task context.

## Decision Dependencies

| Card | Decision | Role |
|---|---|---|
| c126 | D-ASYNCRT1=A | Chosen keystone: M:N scheduler under tasks/channels |
| c126 | D-MNIO1 open | Picks epoll/kqueue/IOCP/io_uring/libuv implementation strategy |
| c101 | D-COROUTINE1 open | Should coroutines stay internal substrate? Rec B |
| c102 | D-NURSERY1 open | Makes scoped task group the canonical spawn scope |
| c36 | D-CONCCOMB1 ratified | Verse-style combinators, implemented through scoped task groups |
| c36 | D-CONCSELECT1 open | First-ready event selection for channels, timers, task handles, I/O |
| c102 | D-TASKSCOPE1 open | Chooses final keyword/API spelling for the scope |
| c103 | D-RACEWIN1=A | Success race folds into task-scope combinator set |
| c112 | D-DEADLINE1 open | Deadline propagation via taskgroup/context |

## Implementation Order

1. **Runtime substrate:** D-MNIO1, scheduler queues, task parking, wakeups,
   timers, and cancellable waits.
2. **Task-scope surface:** D-NURSERY1 plus D-TASKSCOPE1, scoped spawn, task
   handles, join/wait, capture rules, error propagation, and detached-task lint.
3. **Channels/backpressure:** wake-on-send/recv, bounded capacity, cancellation,
   fairness tests, and diagnostics.
4. **Combinators:** `all`, `race`, `any`, and, if D-CONCSELECT1 is ratified,
   `select` over taskgroup/context-owned waits.
5. **Deadlines:** D-DEADLINE1, context inheritance, timeout errors, cancellation
   reason propagation, and diagnostics.
6. **Networking scale:** task-per-connection TCP, HTTP, WebSocket examples and
   stress tests for 100k idle connections.
7. **Expert observability/control:** tracing, task names, taskgroup tree dumps,
   scheduler/poller metrics, tuning knobs, and detached-task audit trail.

## Answer To The Open Questions

Coroutines, scoped task groups, race/any, deadlines, and Go-scale networking are not five
competing models. They stack:

```text
ordinary Jet code
  -> task { } / channels / task handles
  -> scoped task groups own task lifetimes
  -> all/race/any are task-scope combinators; select joins if D-CONCSELECT1 passes
  -> deadlines/cancellation flow through task context
  -> M:N scheduler parks tasks on I/O/timers/channels
  -> platform readiness backend wakes parked tasks
```

The only open owner choices are whether scoped task groups are canonical
(D-NURSERY1), exact spelling (D-TASKSCOPE1), whether coroutines stay internal
(D-COROUTINE1), deadline carrier details (D-DEADLINE1), the OS readiness backend
(D-MNIO1), and whether first-ready event selection joins the structured
combinator set (D-CONCSELECT1). The coherent path is: D-COROUTINE1=B,
D-NURSERY1=A, D-TASKSCOPE1=A or owner-preferred spelling, D-DEADLINE1=A,
D-MNIO1=A, D-CONCSELECT1=A.
