# Concurrency — work is a value

Status: proposal v2, 2026-08-06. Owner decisions: ten ballots on card #1505.
Scope: tasks, groups, channels, select, shared state, transactions, schedules,
streams, protocols, and the unbuilt service plane.
Design-only until S53 unfreezes. Nothing here starts implementation.

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

| Job | Today | Proposed |
|---|---|---|
| Run two things at once | 5 lines, a group, two handles, a list call | `(a, b) :: all { f(), g() }` |
| First result wins | group + handles + `g.race([slow, fast])` | `race { slow(), fast() }` |
| One background task | `tasks.spawn(() => work())` | `task work()` |
| Task failure | `h.exception() == "cancelled"` string test | `h.join() ?? fallback` — the normal `?` rail |
| Bounded worker pool | 49 lines of hand-made channel tokens | `group g(limit: 4) { … }` |
| Drain a channel | `loop { v :: rx.receive() ?? break … }` | `loop v, rx { … }` |
| Wait on two channels | `g.select().recv(a).recv(b).after(ms: 100, value: -1).wait()` | `select { v, a -> …  v, b -> …  after 100ms -> … }` |
| Read shared state | `config.read(c => c.name)` | `config.name` |
| Change shared state | `config.edit(c => { c.hits += 1 })` | `config.hits += 1` |

**Why this is safe to simplify.** Every removed word was ceremony around a
fact the compiler already proves. Structure stays: a task can never outlive
its scope. Data-race freedom stays: every crossing is checked. The magic is
checked magic.

**What the ballots ask.** Three machinery choices (one substrate, one crossing
checker, one stream law), and seven surface choices (spawn shape, failure on
the `?` rail, drop rule, channels and select, shared state, schedules, and the
transaction law). The surface choices lead; a machinery ballot that a surface
pick makes moot is withdrawn.

**Breaking changes.** This is a greenfield redesign. `taskgroup`, `g.task =>`,
`tasks.spawn(() => …)`, the select builder chain, and the `read`/`edit`
closures are all replaced if their ballots pass. Each ballot names the
ratified decisions it amends.

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

For a beginner: write `task`, `all`, `race`, and plain field access on shared
values. The compiler quietly proves that no work is lost, no child outlives
its scope, and no data races. Every failure arrives on the same `?` rail as a
file error.

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
| 12 | The STM law says "retried on conflict"; the runtime ships ordered locks with no retry | `syntax-decisions.md:1828` vs `RuntimeControl.rs:115-140` |

## The surface

This is the heart of the proposal. Each area shows today's code, the proposed
code, and what gets deleted. Every proposed line is marked by its ballot.

### 1. Spawning and fan-out — D-CONC-SPAWN1

**Today.** Two spawn forms, one behind a module import and a lambda.

```jet
use core.tasks as tasks
taskgroup g {
    a :: g.task => sum_range(1, 25)
    b :: g.task => sum_range(26, 50)
    results :: g.all([a, b])
    print(results[0] + results[1])
}
h :: tasks.spawn(() => work())
h.join()
```

**Proposed.** Four spellings cover everything. No import. No lambda wrapper.

```jet
(a, b) :: all { sum_range(1, 25), sum_range(26, 50) }   // run both, wait, get both
print(a + b)

winner :: race { slow(), fast() }        // first done wins; the loser is cancelled
first  :: any  { try_eu(), try_us() }    // first Ok wins; errors wait for a winner

h :: task work()                         // one child task, handle in hand
h.join()

group g(limit: 4) {                      // dynamic fan-out with a worker cap
    loop url, urls { task fetch(url) }
}                                        // the group joins every child here
```

The rules, in plain words:

- `task f()` starts a child of the current scope. Bind the handle and the
  duty to join or detach it is yours. Leave it unbound and the scope joins it
  at the end.
- `all` / `race` / `any` keep their ratified meanings (fail fast, cancel
  losers, first Ok). They need no handles at all.
- `group g(limit: N)` is for dynamic counts and caps. `g` is a value you can
  pass to helpers (`fn drain(g: Group, rx: Receiver<Job>)`). It can never be
  stored, so no child outlives its scope.

**Deleted:** `taskgroup` blocks, `g.task =>`, `g.all([…])`, `g.race`, `g.any`,
`tasks.spawn(() => …)`, `tasks.spawn_group`. Amends D-TASKSCOPE1,
D-NURSERY1's spelling (not its law), D-CONCCOMB1's call shape, and
D-TASKGROUP-PARAM1 (the group parameter rule carries over to `Group`).

### 2. Task failure on the `?` rail — D-CONC-FAIL1

**Today.** Failure facts hide in strings, and a child panic kills the process.

```jet
h.cancel()
if h.exception() == "cancelled" { print("stopped") }
print(h.trace())     // "paused=false,cancel=true"
```

**Proposed.** `join` is fallible like any other call. One rail for every error
in the language.

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
`all { }` returns its tuple on the same rail, so one failed branch is one
`??` away from a fallback.

There is no separate outcome type. A task failure is an error. This is the
type-system-v2 answer: no parallel concept beside results.

**Deleted:** `trace()`, `exception()`, and the panic-kills-process rule for
joined children. Amends D-COROUTINE1's handle surface.

### 3. Channels and select — D-CONC-CHAN1

**Today.** A module call, a manual drain dance, and a builder chain.

```jet
(tx, rx) :: tasks.channel<Int>(capacity: 8)
loop {
    job :: rx.receive() ?? break
    handle(job)
}
winner :: g.select().recv(ch1).recv(ch2).after(ms: 100, value: -1).wait()
```

**Proposed.** Channels are builtin values. Draining is a loop. Waiting on
several sources is an arm table — the same `head -> body` shape as `if`.

```jet
(tx, rx) :: channel<Int>(capacity: 8)

loop job, rx { handle(job) }             // receive until the channel closes

select {
    job, jobs    -> handle(job)          // arm binding mirrors `loop v, source`
    msg, control -> obey(msg)
    after 100ms  -> retry()              // unit literal, one time rail (D-TYPE2-TIME1)
}
```

- `Receiver<T>` and `Sender<T>` become nameable in signatures.
- `select` works anywhere in a task, on plain endpoints. It no longer needs a
  group.
- The dead `Channel` table entry and the `.read` arm (accepted today, silently
  dropped on every tier) are deleted.

**Deleted:** the `g.select()` builder, `tasks.channel`, the `.read` arm.
Amends D-CONCSELECT1 and narrows D-TASKRUNTIME1's module surface.

### 4. Shared state and transactions — D-CONC-SHARE1, D-CONC-STM1

**Today.** A closure per touch.

```jet
config :: Shared.new(AppConfig.{name: "jet-server", hits: 0})
label :: config.read(c => c.name)
config.edit(c => { c.hits += 1 })
```

**Proposed.** A shared value reads and writes like a value. Each statement is
one atomic step. Several statements commit together under `#Transact`.

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

D-CONC-STM1 settles a real drift in the same area: the ratified STM text says
"retried on conflict", but the runtime takes locks in address order and runs
the block exactly once. The ballot picks which one is law. The recommendation
is the shipped behavior — your code runs once, logs print once.

**Deleted (if SHARE1 passes):** the `read`/`edit` closure forms and the
`#Transact(tx)` mandatory name (the name stays only for `on_commit` /
`on_rollback` hooks). Amends D-SHARED-API1 and D-TXN2.

### 5. Schedules, pools, and services — D-CONC-SCHED1

**Today.** The schedule marker parses its own private duration table. There is
no worker cap. The service plane is ratified but unbuilt.

**Proposed.** Scheduling is data on the work.

```jet
#[Job, Every(5min)]                   // spelling stays; 5min is now the one
fn prune_sessions() { … }             // Duration literal every API uses

#[Job, Every("03:00")]
fn nightly_backup() {
    group g(limit: 4) {
        loop shard, shards { task back_up(shard) }
    }
}
```

- One vocabulary: a **job** is a task the runtime starts. Card #1448's naming
  cleanup lands inside this.
- The schedule value becomes typed data behind the unchanged marker, so
  `jet dev`, services, and jetos read one value.
- The service plane (D-SERVICE1) then builds as: a supervisor is a task that
  owns a group; a restart rule is data on that group. No new mechanism.

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
    (c, s) :: Payment.pair()                  // proposed: both endpoints, one call
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
separately. The proposed `pair()` constructor and honest generated types ride
the same machinery ballot (D-CONC-UNIT1).

## How this uses type system v2

Direct answers to the open questions:

- **Results.** Task failure is not a new concept. `join` returns `T ?
  TaskFailure`, an ordinary enum on the one error rail, with `??`, `?`,
  declared conversions, and arm tables. Nothing overlaps with optionals or
  results, because it *is* results.
- **Time.** `after 100ms`, `Every(5min)`, and deadlines all read the one
  Duration rail that D-TYPE2-TIME1 (card #1497) defines. The private schedule
  suffix table dies.
- **Knowledge planes.** State, duty, and reach become registered planes in
  the v2 fact registry. Send-safety is the plane the v2 inventory missed; this
  proposal adds it. Facts become nameable and reflectable like every other
  plane.
- **One branching engine.** `select` arms are S68 arm-table arms, not a
  private grammar. The binding `v, source` mirrors `loop v, source`.

## What this unlocks

- **Parallel code reads like plain code.** `all { f(), g() }` says exactly
  what happens. No handles, no lists, no group name for the common case.
- **One error lesson.** A beginner who learned `?? 0` on file reads already
  knows how to handle a task failure.
- **Worker pools are one line.** `group g(limit: 4)` replaces the 49-line
  token pattern the pragmatism audit flagged.
- **Channel services are two lines.** `loop job, rx` plus `select` covers the
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
  tier parity (I9) is repaired where it is broken today (select on the
  interpreter, the `.read` arm).

## Decisions for the owner

Surface ballots lead. A machinery ballot that a surface pick makes moot is
withdrawn before ratification.

| Ballot | Question | Kind |
|---|---|---|
| D-CONC-SPAWN1 | Adopt `task` / `all` / `race` / `any` / `group(limit:)` and delete the old spawn surface? | surface |
| D-CONC-FAIL1 | Put task failure on the `?` rail as `TaskFailure`? (owner direction 2026-08-06) | surface |
| D-CONC-JOIN1 | A bound handle that is dropped: error, auto-join, or warning? | surface |
| D-CONC-CHAN1 | Builtin channels, `loop v, rx`, arm-table `select`; delete the builder? | surface |
| D-CONC-SHARE1 | Shared values read and write like values; statements lock; `#Transact` commits? | surface |
| D-CONC-SCHED1 | Typed schedule data, one job vocabulary, service plane on the substrate? | surface |
| D-CONC-STM1 | Transaction law: ordered one-run commit, or retry? | law |
| D-CONC-UNIT1 | Re-found the internals on typestate + obligations + one fact registry? | machinery |
| D-CONC-CROSS1 | One crossing checker and one error voice for every worker boundary? | machinery |
| D-CONC-STREAM1 | One lifecycle law for streams and tasks? | machinery |

## Implementation shape

Design-only until S53 unfreezes. After the gate and ratification:

- **Phase A — machinery.** Land UNIT1/CROSS1/STREAM1 internals with today's
  surface and every test green. Fix the interpreter select gap.
- **Phase B — surface.** Land each ratified surface ballot as one greenfield
  migration: new spelling in, old spelling deleted, spec, examples, goldens,
  and docs updated in the same change.
- **Phase C — build the owed features on the substrate.** Service plane,
  typed job scopes, Windows IOCP conformance.
