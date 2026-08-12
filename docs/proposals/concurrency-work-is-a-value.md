# Concurrency — work is a value

Status: ratified record, 2026-08-08. Thirteen `D-CONC-*` decisions on card
#1505 are ratified and define settled law.
Scope: tasks, groups, channels, readiness waits, shared state, transactions,
schedules, streams, protocols, and the service-plane substrate.
This document records the ratified design. Implementation is tracked on
separate cards.

## Executive summary

**The finding.** Jet's concurrency works, but it is harder to read and write
than it needs to be. Spawning takes a module import and a lambda. Fan-out takes
a block, a handle per task, and a list call. A worker pool takes 49 lines.
Shared state takes a closure for every read. Task failure hides in strings.
Under the surface, the compiler holds the same three facts about concurrent
work that it already holds about other values — but with private copy-paste
machinery instead of its own type system.

**The idea.** A unit of concurrent work is an ordinary value. The compiler
holds three facts about it: its **state** (typestate), its **duty**
(obligation), and its **reach** (may it cross to another worker). Jet already
has one machine for each fact. When the machinery collapses into those
machines, the surface collapses too: parallel code starts to read like plain
code.

**The surface wins, before and after.**

| Job | Today | Ratified form |
|---|---|---|
| Run two things at once | 5 lines, a group, two handles, a list call | `(a, b) :: task.all { f(), g() }` |
| First result wins | group + handles + a list wait | `task.race { slow(), fast() }` |
| First successful result | group + handles + a list wait | `task.any { try_a(), try_b() }` |
| One background task | imported spawn helper plus a lambda | `task work()` |
| Task failure | string state and trace helpers | `h.join() ?? fallback` — the normal `?` rail |
| Bounded worker pool | 49 lines of hand-made channel tokens | `task.group g(limit: 4) { … }` |
| Drain a channel | `loop { v :: rx.receive() ?? break … }` | `loop v, rx { … }` |
| Wait on two channels | `g.select().recv(a).recv(b).after(ms: 100, value: -1).wait()` | `if { v, a -> …  v, b -> …  after 100ms -> … }` (D-CONC-CHAN2=D) |
| Read shared state | `config.read(c => c.name)` | `config.name` |
| Change shared state | `config.edit(c => { c.hits += 1 })` | `config.hits += 1` |

**Why this is safe to simplify.** Every removed word was ceremony around a
fact the compiler already proves. Structure stays: a task can never outlive
its scope. Data-race freedom stays: every crossing is checked. The magic is
checked magic.

**Ratified slate.** The decisions settle one substrate, one crossing checker,
one stream law, the nested task surface, the task-failure rail, the join duty,
channels and readiness waits, shared state, schedules, transactions, and group
parameter positions. `D-CONC-FAIL1=A` retires the separate task outcome
surface from `D-CONC-OUTCOME1=A`; `D-CONC-CHAN2=D` changes the readiness-table
spelling selected by `D-CONC-CHAN1=A`.

**Breaking changes.** This is a greenfield redesign. The earlier task-group,
handle-list, imported-spawn, select-builder, and shared-value closure forms are
replaced by the ratified forms. Each decision names the earlier law it amends.

## Glossary

- **Task** — one unit of work that runs on its own. `task f()` starts one and
  gives a `Task<T>` handle.
- **Scope** — any block. Every task belongs to the scope that started it and
  cannot outlive it.
- **Group** — a scope you can name, pass, and give rules to (like a worker
  limit). Needed only for dynamic fan-out.
- **Duty** — a thing the compiler makes you finish before a value dies. A
  bound task handle carries the duty "join or detach me".
- **Reach** — the checked fact that a value may move to another worker.
- **The `?` rail** — Jet's one error path: `T ? Err` returns, `?` to pass an
  error up, `??` for fallbacks, arm tables to tell errors apart.
- **Typestate** — compile-time tracking of a value's current state, with
  operations gated by state.
- **Arm table** — Jet's one branching shape: `{ head -> body }` (S68).

## The one idea

**A unit of concurrent work is a value like any other. The compiler holds
three facts about it — state, duty, reach — with the machines it already has.
The surface then says only what the machines cannot infer.**

For a beginner: write `task`, `task.all`, `task.race`, `task.any`, and plain
field access on shared values. The compiler quietly proves that no work is
lost, no child outlives its scope, and no data races. Every failure arrives on
the same `?` rail as a file error.

For an expert: every fact is a value you can name, query, and reflect. Handles,
groups, endpoints, and failures are ordinary typed values, so they compose
with generics, tools, and tests.

## Evidence — why the rethink is needed

Each row is one job the compiler does twice under different names. File and
line references prove each claim.

| # | Same job, two names | Proof |
|---|---|---|
| 1 | The unjoined-task check is a copy of the `#SingleUse` check. One warns, one errors. | `CheckerOwnership.rs:4141` vs `:4173`; the comment says "Mirrors the unjoined-task check" |
| 2 | E0140's own text names "an unjoined task" — but `Task<T>` is not `#SingleUse` | `CheckerOwnership.rs:5889` |
| 3 | A task's lifecycle is tracked by three ad-hoc fields while the typestate engine sits unused | `mod.rs:762`, `CheckerTaskGroup.rs:12` vs `State.rs` |
| 4 | `protocol` already compiles to `#SingleUse` + `state` + `#Transition`. The composite works. | `Sema/Protocol.rs:13-80` |
| 5 | Five checkers ask "may this cross to another worker": tasks, `para_*`, kernels, cells, fixed backings | `CheckerOwnership.rs:4229/:4463/:5117/:4495`, `AST/items.rs:832` |
| 6 | Send-safety is stored as a stray `bool`, outside the fact registry and the type-system-v2 plane list | `mod.rs:1124` |
| 7 | The runtime knows a task ends four ways. Jet code gets a `String`. | `scheduler.rs:1223` vs `StructuralDebug.rs:31` |
| 8 | Four handle types exist only in error messages: `TaskGroup`, `SelectBuilder`, `Transaction`, `Capability` | `effects_surface.rs:128/147/263/121` |
| 9 | `Receiver<T>` cannot be written in a signature; a dead `Channel` entry can | `type_assign.rs:284` |
| 10 | Streams run on the task scheduler with a separately written shutdown law — and the copies drifted | D-STREAMYIELD1 vs `field-audit-2026-08-03.md:194` |
| 11 | `select` does not work on the interpreter tier; the `.read` arm is dropped on every tier | `TIR/eval/exprs.rs:5139`, `emit/helpers.rs:225` |
| 12 | The earlier STM text differed from the shipped ordered-lock commit | `docs/spec/syntax-decisions.md:1903-1907` and `crates/jet-codegen/src/Prelude/Core/RuntimeControl.rs:115-140` |

## The surface

This is the heart of the record. Each area shows today's code, the ratified
code, and what gets deleted. Every ratified form is marked by its decision.

### 1. Spawning and fan-out — D-CONC-SPAWN1=D

**Earlier draft.** Spawning required an imported helper or a named group with
one handle per child, followed by a list wait. The ratified form removes that
ceremony.

**Ratified law.** One keyword covers the spawn surface. No import. No lambda
wrapper for the common forms.

```jet
results :: (task.all { sum_range(1, 25), sum_range(26, 50) }) ?? []
print(results[0] + results[1])

winner :: (task.race { slow(), fast() }) ?? 0       // first success; cancel losers
first  :: (task.any { try_eu(), try_us() }) ?? 0    // first completion; cancel the rest

h :: task work()                         // one child task, handle in hand
h.join() ?? 0

task.group g(limit: 4) {                  // dynamic fan-out with a worker cap
    loop url, urls { task fetch(url) }
}                                        // the group joins every child here
```

The rules, in plain words:

- `task f()` starts a child of the current scope. Bind the handle and the
  duty to join or detach it is yours. Leave it unbound and the scope joins it
  at the end.
- `task.all` / `task.race` / `task.any` return `T ? TaskFailure`. `all` waits
  for every branch and fail-fast cancels siblings; `race` returns the first
  successful result and cancels losers; `any` returns the first completed
  result and cancels the rest. They need no handles at all.
- `task.group g(limit: N)` is for dynamic counts and caps. `g` is a value you
  can pass to helpers (`fn drain(g: TaskGroup, rx: Receiver<Job>)`). It can never
  be stored, so no child outlives its scope.
- `D-CONC-GROUP1=A` allows a group borrow in free-function and method
  parameters. Storage, return, capture, and fields remain banned.

**Deleted:** the earlier task-group blocks, handle-list combinators, imported
spawn helper, and parallel task-outcome surface. D-CONC-SPAWN1=D keeps the
structured lifetime and combinator laws while changing their spelling.
D-CONC-GROUP1=A amends D-TASKGROUP-PARAM1's parameter-position rule.

### 2. Task failure on the `?` rail — D-CONC-FAIL1=A

**Earlier draft.** Failure facts were exposed as strings and traces, and a
child panic killed the process instead of becoming a joined error.

**Ratified law.** `join` is fallible like any other call. One rail carries
every error in the language.

```jet
score :: task compute()
best  :: score.join() ?? 0               // fallback, like any ? call

if slow.join() == {                      // tell the failures apart (S68 arm table)
    .Ok(v)               -> print(v)
    .Err(.Cancelled)     -> print("stopped early")
    .Err(.Panicked(why)) -> print("worker failed: " + why)
    else                 -> print("out of time")
}
```

`join()` returns `T ? TaskFailure`. `TaskFailure` is a normal enum:
`.Cancelled`, `.DeadlineBlown`, `.Panicked(reason)`. It converts up through
`impl TaskFailure => AppErr` like every other error family (D-ERR-CONV).
`task.all { }` returns its tuple on the same rail, so one failed branch is one
`??` away from a fallback.

There is no separate outcome type. A task failure is an error. This is the
type-system-v2 answer: no parallel concept beside results.

**Deleted:** the string trace/state accessors, separate outcome/status types,
and the panic-kills-process rule for joined children. D-CONC-FAIL1=A amends
D-COROUTINE1 and retires the task-outcome surface ratified by
D-CONC-OUTCOME1=A.

### 3. Channels and readiness waits — D-CONC-CHAN1=A, D-CONC-CHAN2=D

**Today.** A module call, a manual drain dance, and a builder chain.

```jet
(tx, rx) :: tasks.channel<Int>(capacity: 8)
loop {
    job :: rx.receive() ?? break
    handle(job)
}
winner :: g.select().recv(ch1).recv(ch2).after(ms: 100, value: -1).wait()
```

**Ratified law.** Channels are builtin values. Draining is a loop. Waiting on
several sources is a subjectless `if` table. It adds no branching keyword.
D-CONC-CHAN2=D amends the readiness-table spelling selected by
D-CONC-CHAN1=A.

```jet
(tx, rx) :: channel<Int>(capacity: 8)

loop job, rx { handle(job) }             // receive until the channel closes

if {
    job, jobs    -> handle(job)          // arm binding mirrors `loop v, source`
    msg, control -> obey(msg)
    after 100ms  -> retry()              // unit literal, one time rail (D-TYPE2-TIME1)
}
```

- `Receiver<T>` and `Sender<T>` become nameable in signatures.
- The wait table works anywhere in a task, on plain endpoints. It does not
  need a group. `select` is not a keyword; it stays a free identifier.
- The dead `Channel` table entry and the `.read` arm (accepted today, silently
  dropped on every tier) are deleted.

**Deleted:** the `g.select()` builder, `tasks.channel`, and the `.read` arm.
D-CONC-CHAN1=A amends D-CONCSELECT1 and narrows D-TASKRUNTIME1's module
surface. D-CONC-CHAN2=D makes the wait spelling the subjectless `if` table
shown above.

### 4. Shared state and transactions — D-CONC-SHARE1=A, D-CONC-STM1=A

**Today.** A closure per touch.

```jet
config :: Shared.new(AppConfig.{name: "jet-server", hits: 0})
label :: config.read(c => c.name)
config.edit(c => { c.hits += 1 })
```

**Ratified law.** A shared value reads and writes like a value. Each statement
is one atomic step. Several statements commit together under `#Transact`.

```jet
config :: shared AppConfig.{name: "jet-server", hits: 0}   // Shared<AppConfig>

label :: config.name                  // one locked read
config.hits += 1                      // one locked write

#Transact {                           // several steps, one commit
    from.balance -= amount
    to.balance += amount
}
```

The lock story in one sentence: one statement is one step, one `#Transact` is
one commit, and the compiler orders every lock, so programs cannot deadlock
on shared values. Expert guards (`guard_read`, `guard_edit`, `Condition`)
stay for manual control.

D-CONC-STM1=A settles the drift in the earlier STM text: the block body runs
exactly once, locks are acquired in fixed order, and contention waits instead
of retrying. A log line inside the block runs once.

**Deleted:** the `read`/`edit` closure forms and the `#Transact(tx)` mandatory
name. The name stays for `on_commit` and `on_rollback` hooks. D-CONC-SHARE1=A
amends D-SHARED-API1 and D-TXN2.

### 5. Schedules, pools, and services — D-CONC-SCHED1=A

**Today.** The schedule marker still parses its private duration table. There is
no worker cap. The service topology is ratified; its typed schedule consumer is
unshipped.

**Ratified law.** Scheduling is typed data on the work.

```jet
#[Job, Every(5min)]                   // spelling stays; 5min is now the one
fn prune_sessions() { … }             // Duration literal every API uses

#[Job, Every("03:00")]
fn nightly_backup() {
    task.group g(limit: 4) {
        loop shard, shards { task back_up(shard) }
    }
}
```

- One vocabulary: a **job** is a task the runtime starts. Card #1448's naming
  cleanup is part of this law.
- The schedule value becomes typed data behind the unchanged marker, so
  `jet dev`, services, and jetos read one value.
- The service plane (D-SERVICE1) then builds as: a supervisor is a task that
  owns a group; a restart rule is data on that group. No new mechanism.

The ratified law deletes the private schedule table. The current implementation
still accepts only `ns`, `us`, `ms`, `s`, and `min`; the D-TYPE2-TIME1 value,
`2h`/`1d`, and service/jetos consumers are unshipped.

### 6. Protocols — clearer, and on the same three facts

Protocols already work the way this whole proposal wants: an endpoint is a
value, its state is typestate, and "finish the conversation" is a duty. Here
is the full picture in one example.

```jet
protocol Payment {
    client: Charge(cents: Int)        // client speaks first
    server: Receipt(id: Int)          // then the server answers
}

fn run() {
    (c, s) :: Payment.pair()                  // generated endpoint pair
    task serve(s)                             // hand the server end to a child

    c1 :: c.Charge(1200) ?? return            // send; the endpoint moves to its next state
    r  :: c1.recv_Receipt() ?? return         // receive; conversation complete
    print(r.id)
}

fn serve(s: ^Payment.Server) {
    q :: s.recv_Charge() ?? return
    q.Receipt(41) ?? return
}
```

What the compiler enforces, with no runtime cost:

- Send `Receipt` before `Charge` arrives → compile error (wrong state, E0150).
- Drop `c1` without finishing → compile error (unfinished duty, E0140).
- The endpoint moved to `serve` must be sendable → checked (reach).

That is state, duty, and reach — the same three facts as every task. Today
each endpoint is minted by string-generated code and the two ends are made
separately. The `pair()` constructor and honest generated types use the same
machinery settled by D-CONC-UNIT1.

## How this uses type system v2

Settled answers:

- **Results.** Task failure is not a new concept. `join` returns `T ?
  TaskFailure`, an ordinary enum on the one error rail, with `??`, `?`,
  declared conversions, and arm tables. Nothing overlaps with optionals or
  results, because it *is* results.
- **Time.** `after 100ms`, `Every(5min)`, and deadlines all read the one
  Duration rail that D-TYPE2-TIME1 (card #1497) defines. The private schedule
  suffix table is retired by law; the current parser boundary is recorded
  above.
- **Knowledge planes.** State, duty, and reach are registered planes in the
  v2 fact registry. Send-safety is the crossing plane settled by
  D-CONC-CROSS1=A. Facts become nameable and reflectable like every other
  plane.
- **One branching engine.** Readiness arms are S68 arm-table arms inside a
  subjectless `if`, not a private grammar. The binding `v, source` mirrors
  `loop v, source`.

## What this unlocks

- **Parallel code reads like plain code.** `task.all { f(), g() }` says exactly
  what happens. No handles, no lists, no group name for the common case.
- **One error lesson.** A beginner who learned `?? 0` on file reads already
  knows how to handle a task failure.
- **Worker pools are one line.** `task.group g(limit: 4)` replaces the 49-line
  token pattern the pragmatism audit flagged.
- **Channel services are two lines.** `loop job, rx` plus a readiness `if`
  table covers the
  most common concurrency shape in real code.
- **Shared state loses its closure tax.** Counters and config are field
  reads and writes again, still race-free.
- **The service plane builds once.** Supervisors, restarts, and typed job
  scopes land on values whose lifetimes the compiler already proves.
- **Experts lose nothing.** Handles, groups, endpoints, guards, and failures
  are ordinary typed values — nameable, generic, reflectable.

## What stays, and why it earns its place

- **No coloring, ever.** No `async`, no `await`, no function color. E0040
  stays law. This is Jet's biggest ergonomic win and it is already ratified.
- **Structure stays.** A child never outlives its scope. The spelling gets
  lighter; the law does not move.
- **Preemptive cancellation** (D-CANCELMODEL1=C), `#Shield`, and
  `#Context(deadline:)` stay: one unwind engine, proven design.
- **`para_map` and friends stay**: the one-word answer for data parallelism.
- **Walls stay**: no actors, no mutex surface, no top type, protocols hold at
  two endpoints. All knowledge erases before codegen — zero runtime cost, and
  tier parity (I9) is repaired where it is broken today (the readiness wait on
  the interpreter, the `.read` arm).

## Ratified decisions

All thirteen decisions are ratified. The table records each outcome and its
settled law. Superseded surfaces stay named only where the later decision
records the amendment.

| Decision | Outcome | Settled law |
|---|---|---|
| D-CONC-UNIT1 | A | State, duty, and reach use one substrate: typestate, single-use obligations, and one crossing plane. No surface change. |
| D-CONC-JOIN1 | A | Dropping a bound task handle is a compile error. Join it, use its result, or detach it. The rule extends D-LIN1 and amends L1101. |
| D-CONC-GROUP1 | A | A group borrow works in free-function and method parameters. It cannot be stored, returned, captured, or put in a field. |
| D-CONC-OUTCOME1 | A, retired by FAIL1=A | The typed outcome/status surface was ratified, then retired. Separate status and trace accessors do not ship. |
| D-CONC-CROSS1 | A | Crossing safety is one registered fact plane with one error family. Existing task, adapter, kernel, cell, and fixed-backing semantics stay unchanged. |
| D-CONC-STM1 | A | A transaction body runs once. The commit takes locks in fixed order. Contention waits; it does not retry. |
| D-CONC-SCHED1 | A | Schedule values use the typed time rail. A job is a task the runtime starts. Services use supervisor tasks and groups. |
| D-CONC-STREAM1 | A | A stream is a task. Dropping its iterator cancels its producer at the next wait point, with normal cleanup. |
| D-CONC-CHAN1 | A, spelling amended by CHAN2=D | `channel<T>()` is builtin. `loop value, receiver` drains it. The readiness wait uses the arm-table shape on plain endpoints. |
| D-CONC-SHARE1 | A | `shared` values use plain field access. Each statement is one atomic step. Several steps use `#Transact`; expert guards stay. |
| D-CONC-SPAWN1 | D | One reserved word owns the family: `task`, `task.all`, `task.race`, `task.any`, and `task.group`. Only `task` is reserved. |
| D-CONC-FAIL1 | A | `join()` returns `T ? TaskFailure`. `TaskFailure` has `.Cancelled`, `.DeadlineBlown`, and `.Panicked(reason)`. It retires the separate outcome types. |
| D-CONC-CHAN2 | D | The readiness wait is a subjectless `if` table with binding/source heads, `after` time, optional non-blocking `else`, and one atomic wait. `select` stays a free identifier. |

## Implementation shape

This record and its decision slate are complete. Implementation is separate
from this closeout and must use the settled law above.

- **Substrate.** Land UNIT1, CROSS1, and STREAM1 on the shared engines.
- **Surface.** Migrate each ratified spelling as one greenfield change. Delete
  every replaced consumer, example, golden, snapshot, and doc in that change.
- **Owed features.** Build the service plane, typed job scopes, and Windows
  IOCP conformance on the substrate.
