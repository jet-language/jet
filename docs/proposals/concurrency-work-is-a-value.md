# Concurrency — work is a value

Status: proposal, 2026-08-06. Owner decisions: nine ballots on card #1505.
Scope: tasks, taskgroups, channels, select, protocols, Shared/guards, `#Transact`/STM,
`#Job`/`#Every`, streams/generators, cancellation, and the service plane. Design-only until
S53 unfreezes; nothing here starts implementation. Sources: five research passes over spec,
sema, prelude, runtime, examples, Tower, prior audits, and peer-language lessons.

## Executive summary

**The finding.** Five research passes found that Jet already built the type theory its
concurrency needs — and then did not use it. The unjoined-task check and the `#SingleUse`
obligation check are two copies of the same pass, one a lint, one an error
(`CheckerOwnership.rs:4141` vs `:4173`; the second's comment says "Mirrors the unjoined-task
check"). E0140's own error text names "an unjoined task" as its example, yet `Task<T>` is not
`#SingleUse`. The `protocol` feature already composes `#SingleUse` + `state` + `#Transition`
into working session-typed concurrency. Five separate provers answer one question — "may this
value travel to another executor?" — with five error vocabularies. The runtime knows a task
ends in one of four ways; Jet users get that fact as a `String`.

**The idea.** **A unit of concurrent work is an ordinary value, and everything the compiler
must know about it is three facts Jet already tracks for other values: what state it is in
(typestate), what must still be done with it (obligation), and where it may travel (crossing
knowledge).** Concurrency stops being a feature plane with private machinery and becomes three
existing knowledge planes applied to a handful of value types, plus one scheduler.

**Why now.** The largest ratified-but-unbuilt block in the decision record is concurrency: the
six-decision service plane (D-SERVICE1..UPGRADE1, cards #444/#1150-#1153), typed job scopes
(D-JOB-SUBCMD1, #1448/#1449), and the reserved `#Async { }` block. Building them on today's
five hand-rolled provers and four outcome vocabularies means building them twice. The
type-system-v2 proposal (card #1497) is defining knowledge planes right now — and its
inventory is missing exactly one obvious plane: sendability, today a stray `bool`.

**The payoffs, concretely.**
- The structured-concurrency guarantee becomes a theorem, not a special case: a taskgroup is a
  borrow of its scope, so children cannot outlive it — E1110 is the escape rule wearing a
  different code.
- Dropping a task silently is no longer a lint loophole: the join duty becomes the same
  obligation the language already enforces for every `#SingleUse` value, with `.detach()` as
  its sanctioned discharge.
- Task outcomes become a real enum (`Finished`/`Panicked`/`Cancelled`/`DeadlineBlown`) instead
  of `trace()` strings; cancellation becomes queryable; select, dispatch, and `Loadable` speak
  one vocabulary.
- One crossing prover replaces five (task captures, `para_*`, GPU kernels, `Cell`/`#Local`,
  fixed backings) — and new parallel surfaces become table rows, not new provers.
- The service plane lands once, on structure that already proves child lifetimes: a supervisor
  is a task that owns a group; a scheduled job is a task whose spawner is the runtime.

**What the ballots ask.** One direction ballot (adopt the model), then eight standalone
choices: the join duty, spawn-authority spelling, typed outcomes, the one crossing plane,
resolving the D-STM1 text/implementation drift, scheduling-as-data, streams-as-tasks, and
completing the channel surface. Any subset can be adopted; each names the ratified decisions
it amends.

**What does not change.** Every beginner spelling ships as-is: `taskgroup g { }`,
`g.task =>`, `tasks.spawn`/`.join()`, `(tx, rx) :: tasks.channel<T>()`, `~tx`,
`Shared.read/edit`, `#Transact`, `#Shield`, `#Context(deadline:)`, `#[Job, Every(5min)]`,
`para_*`. No coloring, ever (E0040 stays law). No actors, no mutex surface, no new keywords.
All knowledge erases before codegen — zero runtime cost, I9 untouched.

## Glossary

- **Unit of work** — one thing that runs on its own: a spawned task, a taskgroup child, a
  stream producer, a scheduled job, a service worker.
- **Handle** — the ordinary value that stands for a unit of work in the program (`Task<T>`,
  a protocol endpoint, a `Receiver<T>`).
- **Typestate** — compile-time tracking of which named state a value is in, with operations
  gated by state (`state` blocks + `#Transition`, D-STATE1). Erased before codegen.
- **Obligation** — a duty the compiler enforces before a value may be dropped: use exactly
  once (`#SingleUse`, D-LIN1), close a resource (`defer close`), join a task.
- **Crossing knowledge** — the fact that a value may legally move to another executor (another
  task, a parallel worker, a GPU kernel). Today spelled E1101/E1102/E1111/E1130 and the
  `Cell`/`#Local` rules.
- **Plane** — one kind of knowledge with its own combination rules, in the type-system-v2
  sense: states, effects, units, obligations.
- **Spawn authority** — the right to start a child inside a scope. Today: the `taskgroup`
  handle `g`.
- **Structured concurrency** — the law that a child never outlives the scope that spawned it
  (D-NURSERY1).
- **Scheduler** — the M:N green-thread runtime (D-ASYNCRT1). Tasks park at wait points;
  no `async`/`await` coloring exists or ever will (E0040).
- **STM plane** — `Shared<T>` reads/writes inside `#Transact` committing atomically (D-STM1).

## The one idea

**A unit of concurrent work is a value like any other; the compiler holds exactly three facts
about it — its state, its duty, and its reach — and Jet already owns one machine for each.**

The beginner story: nothing changes on the page. You write `taskgroup g { }` and `g.task =>`,
and the compiler quietly knows your task is *running*, that somebody must *join* it, and that
what it captures is *safe to send*. When you get it wrong, every error speaks one language:
what state the value is in, what duty is undischarged, what boundary it cannot cross.

The expert story: every one of those facts is nameable, queryable, and reflectable. A task's
outcome is an enum you can match. Cancellation is a state you can ask about. Sendability is a
registered plane, not folklore. And when you build something new — a supervisor, a worker
pool, a session protocol with more states — you add rows to existing planes, not mechanisms.

## Evidence — the shadow systems

Every row is the same underlying job done twice under different names.

| # | Shadow system | Where it lives | The defect |
|---|---|---|---|
| 1 | Unjoined-task check vs `#SingleUse` check | `CheckerOwnership.rs:4141` / `:4173` | Byte-for-byte the same pass; one is lint L1101, the other error E0140. The doc comment admits the mirror. |
| 2 | E0140's own copy | `CheckerOwnership.rs:5889` | Names "an unjoined task" as its canonical example — but `Task<T>` is not `#SingleUse`. |
| 3 | No-clone rule for task handles | `Diagnostics.rs:1219` (`type_holds_task_handle`) | E0142's no-copy rule hand-rolled a second time for one type. |
| 4 | Task lifecycle tracking | `LocalInfo.task_lint_span` (`mod.rs:762`), the `moved` map, `PendingTaskSpawn.consumed` (`CheckerTaskGroup.rs:12`) | A three-state automaton tracked by three ad-hoc fields while the typestate engine (`State.rs`, 861 lines) sits unused. |
| 5 | `protocol` expansion | `Sema/Protocol.rs:13-80` | Proof the composite works: generates `#SingleUse` + `state` + `#Transition` source and re-parses it. Session concurrency is already typestate + obligation. |
| 6 | Five crossing provers | Sendability `CheckerOwnership.rs:4229/:4463` (E1101/E1102); group-borrow disjointness `CheckerTaskGroup.rs:203-301`; `para_*` E1111; kernel proofs `AST/items.rs:832` (E1130); `Cell`/`#Local` `CheckerOwnership.rs:5117`; `ThreadConfined` `:4495` | One question — "may this cross?" — five vocabularies, no shared code. |
| 7 | Sendability storage | `LocalInfo.sendable: bool` (`mod.rs:1124`) | A knowledge plane stored as a stray bool; absent from `FactRegistry` and from the type-system-v2 plane inventory. |
| 8 | Four outcome vocabularies | `JetSchedulerResult{Value,Panicked,Cancelled,Deadline}` (`scheduler.rs:1223`), `JetSelectOutcome` (`Prelude/Scheduler.rs:1935`), `DispatchState` (`core_types.rs:1382`), and the user surface: a `String` (`StructuralDebug.rs:31`) | The same three terminal facts, four spellings; the one Jet users see is `"paused=false,cancel=true"`. |
| 9 | Erased minted handles | `TaskGroup` (`effects_surface.rs:128`), `SelectBuilder` (`:147`), `Transaction` (`:263`), `Capability` (`:121`) | Phantom types users meet in errors but cannot write. `fn T.m(g: TaskGroup)` falls into raw E0119 (`CheckerItems.rs:1410`) — an undocumented hole. |
| 10 | `Receiver<T>` unnameable | `type_assign.rs:284` lists `Task \| Channel \| Sender` | `Receiver` omitted; `Channel` is a dead entry for a handle D-TUPLE-DESTRUCT1 deleted. |
| 11 | Three unwind doors | Cancel (D-CANCELMODEL1=C), deadline (E3003 via `#Context`), scope cleanup (`defer close`) | One unwind engine, three entrances; `#Shield` orders the first two, the third is unaware of both. |
| 12 | Two scheduling planes | `#Job`/`#Every` registry rows (`Policy.rs:857/:867`) vs `taskgroup`/`spawn` keywords + hardcoded CoreLib recognition | "This work runs on its own," spelled once as marker data, once as compiler special cases — opposite sides of marker law zero (D-VERDICT-1455-1). |
| 13 | Generators on the scheduler | D-STREAMYIELD1; drift in `field-audit-2026-08-03.md:194-236` (card #1392) | `Stream<T>` is a task joined by pulls, with the cancellation law rewritten independently — and the two copies have already diverged across tiers. |
| 14 | Three timing spellings | `#Every` suffix table (`math_layout.rs:686`), deadline as bare epoch-ms `Int`, `after`/`interval` as bare ms `Int`, while `Duration` exists | Already flagged by type-system-v2 (D-TYPE2-TIME1); every concurrency timer picks a different one. |
| 15 | STM law drift | `syntax-decisions.md:1828` says "retried on conflict"; `RuntimeControl.rs:115-140` ships canonical-order multi-lock, no retry | Ratified text and shipped semantics disagree; only the owner can pick which one is law. |

Below the ledger, two I9 breaches hide under a green `jit_gaps` gate: `g.select()` returns
`unsupported` for all five ops on the interpreter tier (`TIR/eval/exprs.rs:5139-5143`), and the
`.read(stream)` select arm is silently dropped on every tier (`emit/helpers.rs:225`,
`lower_ctx.rs:13031`). These are defects to fix (or delete) regardless of any ballot.

## The model

### The three planes

Every concurrent value carries up to three facts. Each fact already has a machine.

| Plane | The fact | Existing machine | Today's concurrency spelling |
|---|---|---|---|
| **State** | Where the value is in its lifecycle | Typestate (D-STATE1, `State.rs`) | Hand-rolled: `task_lint_span`, `PendingTaskSpawn`, `trace()` strings |
| **Duty** | What must happen before drop | Obligations (D-LIN1 `#SingleUse`; `defer close`) | Hand-rolled: L1101 lint, `type_holds_task_handle`, detach special cases |
| **Reach** | Which executor boundaries it may cross | Crossing knowledge (to be registered as a plane) | Hand-rolled five times: E1101/E1102, group disjointness, E1111, E1130, `Cell` rules |

### The roster

Each concurrency construct is a row — a value type whose cells come from the three planes.

| Value | State | Duty | Reach |
|---|---|---|---|
| `Task<T>` | created → running → paused → done/cancelled | join, or detach explicitly | result must be sendable |
| `TaskGroup` | open → closing | join all children at scope exit (D-NURSERY1) | borrow of its scope — never escapes (E1110) |
| `Sender<T>` / `Receiver<T>` | open → closed | close (or drop-close) | payloads must be sendable |
| Protocol `.Client`/`.Server` | the protocol's own states | `#SingleUse` — run to the end | endpoint crossing rules |
| `Stream<T>` | producing → done/closed | drop-close cancels the producer | pulled values must be sendable |
| `SharedGuard` | held | release (scope-bound) | never crosses |
| `Transaction` handle | active → committed/rolled-back | commit or roll back at block exit | never crosses; no irreversible effects inside (E0746) |
| `#Job fn` | a task whose spawner is the runtime | runtime joins it per run | entry-level: captures nothing |
| Service worker (unbuilt) | supervisor = a task owning a group | restart policy is data on the group | same rules as any task |

Protocols are the existence proof: their row is *already implemented* as generated typestate +
obligation source (`Protocol.rs`). The proposal extends the same treatment to every other row.

### The law

> **Work is a value. Its lifecycle is state, its completion is a duty, its movement is
> knowledge.**

The ratified rules turn out to be theorems of it:

- **D-NURSERY1** (children finish before the scope exits) — the group's join duty, discharged
  at scope end like every obligation.
- **Structured concurrency itself** — spawn authority (`g`) is a borrow of the scope, so no
  child can outlive it. E1110's "call-stack-only spawn authority" is the borrow escape rule.
- **D-DATARACE1=C** (a data race must fail to compile) — reach knowledge is total: every
  boundary crossing consults the same plane.
- **D-DETACH1** (`.detach()` consumes the handle) — obligation discharge by explicit
  consumption; the exact analogue of `consume(x)` for `#SingleUse` values.
- **D-CANCELMODEL1=C** (one unwind, preemptive at wait points) — cancel, deadline, and scope
  cleanup are one engine with three triggers; `#Shield` is a scoped ordering rule on it.
- **D-PROTO1/2** — session protocols are typestate + obligation on endpoint values. Already
  shipped that way.

### The "ohhh" connections, spelled out

1. **A task is a `#SingleUse` value.** The compiler already contains the proof: two identical
   passes, and E0140's error text describes an unjoined task.
2. **A taskgroup is a borrow.** "Cannot be stored, cannot escape, dies with its scope" is not
   a new rule — it is what a borrow is. The unnameable-type ceremony was hiding a loan.
3. **Protocols already are the unified model.** `protocol` compiles to `#SingleUse` structs
   with `state`/`#Transition` — typestate + obligation, generated. The rethink is finishing
   what `Protocol.rs` started, for every handle.
4. **Generators are tasks.** `yield` runs on the scheduler; dropping the iterator cancels the
   producer — D-CANCELMODEL1 restated in another decision's words. Jet has coroutines; it
   spells them `Stream<T>`.
5. **A job is a task whose spawner is the runtime.** `#Job`/`#Every` describe *who starts the
   work and when* — data about the same unit, not a second kind of thing.
6. **The missing type-system-v2 plane is sendability.** The one obvious knowledge-about-a-
   carrier fact the v2 inventory missed is sitting in `LocalInfo.sendable: bool`.

## The surface

### Spelling principles the model implies

1. **Facts have names.** A terminal outcome is an enum, not a `String`. A queryable state is a
   method returning a value, not a formatted trace.
2. **Anything a user can receive is a type a user can write.** `Receiver<T>` in a signature;
   `TaskGroup` in every legal borrow position, with a teaching error elsewhere.
3. **One word per concept.** "Task" is the unit of work; "job" is a scheduled task
   (aligns card #1448). No third word.
4. **Configuration is data on the value.** Group limits, schedules, deadlines are typed
   arguments, not markers or env vars — the marker rebuild (row data) already points here.
5. **Delete before adding.** No new keywords, no new sigils, no second spawn form.

### The concrete slate

| Surface | Status |
|---|---|
| `taskgroup g { }`, `g.task =>`, `g.all/race/any`, `g.select().recv(..).after(ms:, value:).wait()` | ratified, unchanged |
| `tasks.spawn/join/detach/cancel/pause/resume`, list twins (D-VERDICT-1323-1) | ratified, unchanged |
| `(tx, rx) :: tasks.channel<T>(capacity: N)`, `~tx`, `send`/`receive`/`close` | ratified, unchanged |
| `Shared.new/read/edit`, guards, `Condition`, `#Transact`, `#Shield`, `#Context(deadline:)` | ratified, unchanged |
| `#[Job, Every(5min)] fn` | ratified, unchanged |
| Unjoined `Task<T>` → obligation error (E0140 family), `.detach()` discharges | **proposed** (D-CONC-JOIN1) |
| `task.outcome()` → `TaskOutcome` enum; `task.status()` → `TaskStatus`; `trace()`/`exception()` strings retire | **proposed** (D-CONC-OUTCOME1) |
| `Receiver<T>` nameable; `loop v, rx { }` receives until closed; dead `Channel` table entry deleted | **proposed** (D-CONC-CHAN1) |
| `TaskGroup` legal in method position (or a teaching error there); still never stored | **proposed** (D-CONC-GROUP1) |
| `taskgroup g(limit: N) { }` — concurrency cap as group data | **proposed** (D-CONC-SCHED1) |
| One registered sendability plane; E1101/E1102/E1111/E1130 become one error family | **proposed** (D-CONC-CROSS1) |
| `#Every(...)` value becomes typed schedule data (literal spelling defers to D-TYPE2-TIME1) | **proposed** (D-CONC-SCHED1) |

## What it looks like

### Beginner — today's code, unchanged

```jet
use core.tasks as tasks

fn run() {
    taskgroup g {
        a :: g.task => sum_range(1, 25)
        b :: g.task => sum_range(26, 50)
        results :: g.all([a, b])
        print(results[0] + results[1])
    }
}
```

Every line is ratified surface. Under the proposal the compiler now *knows* `a` is a running
task with a join duty discharged by `g.all` — but nothing on the page moves.

### The middle — a bounded worker pool

Today this is 49 lines of hand-rolled channel tokens
(`examples/features/concurrency/bounded_workers.jet`). On the model:

```jet
use core.tasks as tasks

fn run() {
    (tx, rx) :: tasks.channel<Int>(capacity: 8)
    taskgroup g(limit: 4) {              // proposed: limit is data on the group
        g.task => {
            loop id, 1..20 { tx.send(id) }
            tx.close()
        }
        loop job, rx {                   // proposed: receive until closed
            g.task => handle(job)        // ratified; the limit throttles admission
        }
    }
}
```

And a typed outcome instead of string forensics:

```jet
h :: tasks.spawn(() => risky_fetch(url))
match h.outcome() {                      // proposed: consumes the handle, like join
    .Finished(body)   => print(body)
    .Cancelled        => print("stopped early")
    .Panicked(reason) => print("worker failed: " + reason)
    .DeadlineBlown    => print("out of time")
}
```

Dropping `h` without joining, detaching, or taking its outcome is the same error as dropping
any `#SingleUse` value — with the same fix text, naming `.detach()` for fire-and-forget.

### Expert — sessions, supervision, schedules on one substrate

```jet
use core.tasks as tasks

protocol Payment {                       // ratified: already typestate + obligation
    client: Charge(cents: Int)
    server: Receipt(id: Int)
}

fn drain(group: TaskGroup, rx: Receiver<Payment.Receipt>) {   // Receiver<T> proposed-nameable
    loop receipt, rx {                   // proposed
        group.task => archive(receipt)   // ratified: group param spawns own their captures
    }
}

#[Job, Every("03:00")]                   // ratified: a job is a task the runtime spawns
fn settle_accounts() {
    taskgroup g {
        h :: Payment.Client.client()
        r :: h.Charge(1200) ?? panic("charge refused")
        // typestate has already proven no message runs out of order
    }
}
```

The unbuilt service plane (D-SERVICE1=D) lands here without new mechanism: a supervisor is a
task that owns a group; a restart policy is data on that group; a rolling upgrade is a state
transition on the service value. The reserved `#Async { }` block, if it ever ships, is a
scoped-effect row — not a second concurrency system.

## What this unlocks

- **Errors teach one lesson.** "Undischarged duty", "wrong state", "cannot cross" — three
  sentences cover every concurrency error, with the same shapes beginners already met on
  files and `#SingleUse` values.
- **Cancellation becomes visible.** `task.status()` answers `is it cancelled?` — today's
  answer is parsing `"paused=false,cancel=true"`.
- **Worker pools become one line** (`limit:` on the group) instead of a 49-line token dance.
- **Channel-driven services get `loop job, rx`** — the single most common concurrency shape in
  real code (surface-frequency audit: channels hit 100% of Go projects).
- **New parallel surfaces are rows, not provers.** A future `para_sort` or a second kernel
  mode reuses the one crossing plane; today each would hand-roll prover number six.
- **Streams inherit the cancel law by construction**, retiring the whole class of
  generator-lifecycle drift (card #1392).
- **The service plane, workflows, and typed job scopes** (the largest ratified-unbuilt block)
  build once on values whose lifetimes are already proven.
- **Extremes hold.** Trivial one-liner: `nums.para_map((n: Int) => n * 2)` — unchanged.
  Critical simulation: deterministic scheduling, pool sizing, and priorities become data on
  the group/scheduler value — expert knobs on existing types, not carve-outs.

## What does not change

- Every ratified spelling in the slate table above; all 25 concurrency examples still run
  byte-identical.
- **No coloring, ever.** E0040/E0041 teaching errors stay law; no `async`, no `await`, no
  mutex surface, no actor syntax (ratified declines respected).
- **Walls stay.** No top type, no HKT, no macros; comptime never creates types. Protocols stay
  two-endpoint until a projection story exists. First-class storable `TaskGroup` stays
  rejected (the Tower decline's reasoning — "the structured guarantee quietly dies" — is this
  proposal's borrow law, agreed with and kept).
- **Zero cost.** All three planes erase before codegen (I3); TIR, AOT, JIT, interpreter, and
  web lowering see today's shapes. I9 parity is strengthened (the select/interp and
  `.read`-arm breaches get fixed or deleted), never traded.
- The M:N scheduler (D-ASYNCRT1), preemptive cancellation (D-CANCELMODEL1=C), and the STM
  effect wall (E0746) remain exactly as ratified.

## Decisions for the owner

Each ballot stands alone; any subset can be adopted. Amendments to ratified decisions are
named inside the ballot text.

| Ballot | Question | Direction options |
|---|---|---|
| D-CONC-UNIT1 | Adopt "work is a value": re-found task lifecycle/duty/reach on the typestate, obligation, and crossing machinery (internal; no surface change) | adopt / adopt without the crossing merge / decline |
| D-CONC-JOIN1 | What happens to a dropped `Task<T>`? (amends the L1101 lint choice) | obligation error, `.detach()` discharges / auto-join at scope exit / keep the lint |
| D-CONC-GROUP1 | Spawn authority spelling (respects the decline of first-class groups; fixes the method-position E0119 hole) | formalize borrow-of-scope, param-only / extend to method receivers, still never stored / status quo |
| D-CONC-OUTCOME1 | Typed outcomes and status (amends D-COROUTINE1's `trace()`/`exception()` strings) | shared `TaskOutcome`+`TaskStatus` enums / outcome enum only / keep strings |
| D-CONC-CROSS1 | One crossing plane registered in the fact registry; one error family for E1101/E1102/E1111/E1130/`Cell` rules | full merge + one family / merge internals, keep codes / decline |
| D-CONC-STM1 | Resolve D-STM1 drift: text says "retried on conflict", runtime ships ordered multi-lock | amend law to ordered commit / implement retry / amend now, retry as future card |
| D-CONC-SCHED1 | Scheduling is data: typed schedule value, `limit:` on groups, jobs = runtime-spawned tasks (aligns #1448; literal spelling defers to D-TYPE2-TIME1) | adopt / typed values only, markers untouched / decline |
| D-CONC-STREAM1 | One lifecycle law for `Stream<T>` = the task law (subsumes the #1392 drift class) | unify / keep separate laws |
| D-CONC-CHAN1 | Complete the channel surface: `Receiver<T>` nameable, `loop v, rx`, delete the dead `Channel` entry, build-or-delete the `.read` select arm | full completion / nameable + cleanup only / decline |

## Implementation shape

Design-only until S53 unfreezes; phases begin only after that gate and ratification.

**Phase A — internal re-founding, no surface change.** Express the task lifecycle on the
typestate engine and the join duty on the obligation pass (deleting `task_lint_span`,
`type_holds_task_handle`, and the mirrored scope check); register sendability as a fact plane
and fold the five provers into one; reify the outcome enum internally; delete the dead
`Channel` table entry; turn the `TaskGroup` method-position hole into the E1110 teaching
family; fix the interpreter select gap and delete or build the `.read` arm (I9). All existing
tests and goldens stay green.

**Phase B — land the ratified-unbuilt on the substrate.** Service plane
(D-SERVICE1..UPGRADE1): supervisors as tasks owning groups, restart policy as group data.
Typed job scopes (D-JOB-SUBCMD1) and the #1448 naming unification. Windows IOCP conformance
(#527/#1001) is orthogonal and unaffected.

**Phase C — balloted surface unifications**, each a coherent greenfield migration that deletes
the replaced form: typed outcomes (strings die), the join obligation, channel completions,
scheduling-as-data. Every migration updates spec, examples, goldens, snapshots, and docs in
the same change.
