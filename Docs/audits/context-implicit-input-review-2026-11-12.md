# Adversarial review: context as an implicit input (D-CONTEXT-EFFECT1=F)

Date: 2026-11-12. Reviewer: Claude Opus, lane G12Docs. The reviewer did not
write option F or its ballot, and did not build any part of it.

Code basis: origin/dev at 9c11372d2a. The probes ran on the seed-b13 compiler
with `jet run --optimize=none` and `--optimize=full`. Their sources are below,
so each one can be rerun as written. F is not built yet, so every
statement about F's behavior is a prediction from the code that F's cutover
changes, and is marked as one.

## What F claims

Option F (ratified 2026-11-12) makes the task context an implicit input of
every Jet function:

1. Reading context is not an effect. `Context` is no longer an effect name;
   nobody writes, grants or denies it.
2. A function is deterministic in its arguments plus its context, so a pure
   `-[]>` function may read context.
3. `#Memo` keys its cache on the arguments plus the context fields the
   function can reach, or the whole context where sema cannot bound the reach.
   A function that reaches no context read keys on its arguments alone.
4. A C callback receives no context. Reading context there is a registered
   error until the callback installs one.
5. The cutover removes the Context root, the named grant exemption, the
   package budget insertion and the deny path.

## How today's compiler treats a context read

A read is `THostCall.ContextRead` (Sema `Expressions/Places.jet:766-767`). Sema
records the intrinsic `Context` effect there. The IR lowers it to calls of the
runtime routes `jet_ctx_thread`, `jet_ctx_deadline_present` and
`jet_ctx_deadline_ms` (JetIR `BuildTransact.jet:285-306`). Those routes read the
current context record through the thread's GS-based control block
(JetRuntime `Context.jet:25`, `Thread.jet:92-96`).

Because the read is an effect, five consumers treat a function that reads
context as impure today. Each of these is a place where F changes behavior.

| Consumer | Today's rule | Probe result on seed-b13 |
| --- | --- | --- |
| `#Memo` | A memoized function must be pure (E0938). | `memo.jet`: E0938, "`left` uses `Context`". |
| `-[]>` rows | A context read under `-[]>` is E3401. | `pure.jet`: E3401, which names the read `call`, a known text defect. |
| Package deny | `deny: [Context]` refuses every function that reaches a read (E0749). | `deny.jet`: E0749 at `left` and at `run`. |
| Module values | A module value that reaches a read can't be built (E0643). | `modval.jet`: E0643, "it reaches the outside world (Context)". |
| Local-value fold | A local whose initializer has known arguments is computed while compiling, unless its closure is impure (Driver `Pipeline.jet:1039`, Sema `CheckProgram.jet:7553-7562`). | `fold.jet`: the local prints 95 under `#Context(deadline: 100)`. A `#run` value prints 0, from the build root. |

The IR doesn't merge or move context reads. All three routes are opaque runtime
calls; `ir_route_can_stop` (JetIR `Build.jet:2423-2440`) only clears them of
stop checks. `cse.jet` prints `0 95` then `0` at both O0 and O2, so a read
inside a `#Context` block isn't merged with one outside it. F doesn't change this
layer.

## Claim 1: reading context is not an effect

**Holds**, provided the cutover also changes the two consumers it doesn't name.

F names four removals: EffectRoots, the grant exemption at
`Policy/Authority.jet:42`, the budget insertion in Driver `Pipeline.jet` and the
deny path. Removing them is enough to take `Context` out of rows.

But two more consumers decide on the solved row: the module-value check and
the local-value fold. Both call `sema_comptime_effect_pure`, which counts only
`Panic`, `Mem` and `!X` entries as pure, and `sema_comptime_call_facts_impurity`.
Once the row no longer holds `Context`, both treat a context-reading closure as
pure. Claim 2 shows what follows from that.

## Claim 2: deterministic in arguments plus context

**Holds as a definition, but breaks two compile-time folds unless the cutover
adds a guard.**

The compiler folds in two places, and both assume a pure closure depends on its
arguments alone:

- **Local values.** `here: left(5)` has known arguments. Today the Context
  effect makes the closure impure, so the fold refuses it and the local is
  computed at run time (95 in `fold.jet`).
  - [Prediction] Under F the row is empty, so the fold computes `left(5)`
    while compiling, against the build root context. It bakes in 0, and
    `fold.jet` prints `local 0` inside a block whose deadline is 100.
  - `thread2.jet` shows the same failure for `context.thread`. A local in a
    function run by a task prints 1 today, and would print 0 under F
    [prediction].
  - The local-value fold already has this exact problem with `#phase`. The
    `#phase` fix makes the fold refuse a closure that reads `#phase`
    (`sema_comptime_body_unfoldable`, under `unsafe_runs`). A context read
    needs the same refusal.
- **Module values.** `EARLY: left(5)` is E0643 today. [Prediction] Under F the
  closure is pure, so the value is computed while building against the build
  root and stored in the program: deadline None and thread 0.
  - That agrees with D-JAI-CONTEXT1 PREP (prepared reads see the build root).
  - But it silently changes a build error into a fixed value. A program that
    meant "the deadline of whoever runs this" gets a constant instead.

F's own sentence, "deterministic in arguments plus context", is exactly what
both folds violate: they substitute the build's context for the caller's.

The cutover needs one rule for both folds. Either the reach fact F already
plans for `#Memo` also refuses the local-value fold, or the spec says local
values that reach a read are prepared against the build root. The second
option contradicts the `#phase` precedent and the purpose of a deadline. The
first option costs nothing extra, because the fact exists for claim 3.

## Claim 3: `#Memo` keys on the reached context fields

**Holds in design, but isn't built, and its key is underspecified for two
kinds of field.**

Today's store keys only on the arguments. `ir_memo_enter` (JetIR
`BuildMemo.jet:22-60`) copies each bound parameter into the entry and compares
those copies. No context field is in the key. Today that is safe only because
E0938 refuses every memoized function that reads context. F removes that
refusal, so the key has to grow in the same change, or `#Memo` serves a result
computed under one deadline to a caller under another.

Two gaps in the key's definition:

- **Fields without value equality.** F's field list includes
  `allocator`, `logger`, `on_stop`, `print` and `trace`. Some of these are
  handles or function values. The "whole context" fallback, used where the
  reach is unbounded (an unknown function value, for example), needs an
  equality for each of them. F doesn't say whether that is identity or value
  equality.
- **The fallback defeats the cache.** The whole context includes `thread`, and
  `trace`, which differs at every call site. So a whole-context key makes a
  memoized higher-order function cache per thread and per call site, which is
  close to no cache at all. F's "pays nothing" holds only for functions whose
  reach is bounded. A bench row should show what the fallback costs.

The reach fact itself is sound in principle. Every read is one
`THostCall.ContextRead{field}` node, and sema already walks call facts
transitively for effects. Function values, trait objects and generic calls
are the places that need the whole-context fallback.

## Claim 4: a C callback receives no context

**Holds today, through a stricter rule F doesn't mention. The clause "until
the callback installs one" can't be reached.**

D-CABI-CALLBACK1=A limits a C callback to an explicit `-[]>` function or a
capture-free lambda. It must be safe on foreign threads, with no thread-local
state. Sema enforces this with a syntax subset (`Calls/Checking.jet:406-450`):

- A field read (`.Field`, `.Place`) is refused, and so is any call.
- A `#Context` block (`.ContextBlock`) is refused.

So a context read in a callback is already refused (E3203), and so is every
form that could install a context. This was checked by reading the code; no
probe ran, because a probe needs a C library that takes a callback.

The runtime agrees. A foreign thread has no control block, so its GS points at
the allocator's zero word (`Thread.jet:93`, `:98-100`). `context_current()`
would then read address zero plus an offset. A callback that got past sema
would fault, not see an empty context.

Gaps:

- "Until the callback installs one" names a form the callback subset refuses:
  a `#Context` block. There's also nothing for it to push onto, because the
  thread has no record. Either F's text drops the clause, or the cutover
  defines an attach-and-install form for callbacks. The second option amends
  D-CABI-CALLBACK1.
- F's C-callback check is "a calling-convention fact with a registered
  diagnostic". Today the refusal is the generic E3203 subset error. A dedicated
  diagnostic is only needed if the subset ever admits calls. At that point
  the reach fact from claim 3 has to cover callbacks too, or a callback could
  call a helper that reads context.

## Claim 5: what dropping the package deny gives up

**It gives up the only checked way to say "this code depends on its arguments
alone." F accepts that loss, and this review finds nothing else that depended
on it.**

What is lost:

- **A package-level guarantee.** `deny: [Context]` (E0749 in `deny.jet`) and a
  bare `#FX` ceiling are today the only checked ways to promise that a
  package's or module's results don't depend on the caller's deadline or
  thread. Under F, `-[]>` no longer promises that. No construct replaces it.
  A library that wants argument-only determinism, such as a hash, a codec or a
  layout engine, can only keep it by convention.
- **The pure-row reading of `-[]>`.** The spec says `-[]>` is "pure"
  (`spec.md:2131`) and that a bare `#FX` makes every function under it pure
  (`spec.md:2552`). After F, pure means "no machine effect". The spec has to
  say pure functions may read context, wherever it relies on pure meaning
  argument-only.
- **D-DISCARD1 (E0433).** A pure call whose result is ignored is E0433. Under
  F a context read is pure, so ignoring `left(5)` becomes E0433. That is
  harmless, since dropping a read changes nothing.

What isn't lost:

- **IR optimization.** It never relied on the row for context reads; see
  `cse.jet`.
- **Spawn inheritance and block restore.** These belong to the runtime record,
  not the effect, and F keeps them.

Claim 2's folds are the one consumer whose safety came from the deny path and
the row together. F only needs the reach fact to keep them correct.

## A wording note on "implicit parameter"

F's technical text says context "becomes an implicit parameter of every
Jet-convention function, which matches how the runtime already threads the
task context."

The runtime doesn't pass a parameter. It reads a per-thread current-record
pointer in the control block (`Context.jet:25`), which the scheduler and
`#Context` blocks swap. If the cutover read "implicit parameter" as an ABI
change, every function would gain an argument. If it keeps the control-block
record, nothing changes in codegen.

The spec text should say the input is the task's current context record, not
a parameter.

## Verdict

**F holds**, on two conditions:

- The cutover builds the reach fact once and uses it in all four consumers that
  today rely on the Context row:
  1. the `#Memo` key;
  2. the local-value fold, which refuses a closure that reaches a read, as
     it does for `#phase`;
  3. the module-value check, which either refuses the value or documents
     the build-root value;
  4. the C-callback check, if its subset ever admits calls.
- F's C-callback sentence drops "until the callback installs one", or a card
  defines that form.

Nothing found contradicts F's central claim. Context reads are cheap, the IR
doesn't reorder them, and with the reach fact in place, every consumer can key
or refuse precisely.

## Gaps to put on cards

Each gap is written as a criterion. It goes on #5025, beside c12, unless Main
links an existing criterion.

1. **Local-value fold.** A local value whose initializer reaches a context read
   stays run-time code, as one that reads `#phase` does.
   - Proof: `fold.jet` prints `local 95 prepared 0`, and `thread2.jet` prints
     `task local 1`, at O0, O1 and O2.
2. **Module values.** A module value that reaches a context read is either
   refused (E0643, as today) or stored as its build-root value. The spec says
   which.
   - Proof: a case for `modval.jet`.
3. **`#Memo` key.** The key names an equality for every context field,
   including handles and function values. A bench row measures the
   whole-context fallback for a memoized higher-order function.
4. **C callbacks.** F's text drops "until the callback installs one", or a card
   defines an attach-and-install form for C callbacks as an amendment to
   D-CABI-CALLBACK1.
   - Either way, a UI snapshot shows a context read in a C callback refused
     with a context-specific message.
5. **Spec.** Wherever it calls a `-[]>` function or a bare `#FX` "pure", the
   spec says that pure code may read context. It also describes the input as
   the task's current record, not a parameter.

## Probe sources

Every probe runs as `jet run --optimize=<none|full> <file>` with no package
manifest. `deny.jet` is the only one with a manifest.

`memo.jet`:

```jet
#Memo
left: fn(now: Int) -> Int { (context.deadline ?? now) - now }

run: fn() {
    print(left(5))
    #Context(deadline: 100) {
        print(left(5))
    }
}
```

`pure.jet`:

```jet
left: fn(now: Int) -[]> Int { (context.deadline ?? now) - now }

run: fn() {
    print(left(5))
}
```

`deny.jet`:

```jet
package: {
    name: "denied"
    authority: { deny: [Context] }
}

left: fn(now: Int) -> Int { (context.deadline ?? now) - now }

run: fn() {
    print(left(5))
}
```

`modval.jet`:

```jet
left: fn(now: Int) -> Int { (context.deadline ?? now) - now }

EARLY: left(5)

run: fn() {
    #Context(deadline: 100) {
        print("module value {EARLY}")
    }
}
```

`fold.jet` prints `local 95 prepared 0` at O0 and O2:

```jet
left: fn(now: Int) -> Int { (context.deadline ?? now) - now }

EARLY: #run left(5)

run: fn() {
    #Context(deadline: 100) {
        here: left(5)
        print("local {here} prepared {EARLY}")
    }
}
```

`cse.jet` prints `0 95` then `0` at O0 and O2:

```jet
left: fn(now: Int) -> Int { (context.deadline ?? now) - now }

run: fn() {
    a: left(5)
    #Context(deadline: 100) {
        b: left(5)
        print("{a} {b}")
    }
    c: left(5)
    print("{c}")
}
```

`thread2.jet` prints `task local 1` at O0 and O2:

```jet
which: fn() -> Int { context.thread }

inner: fn() -> Int {
    here: which()
    here
}

run: fn() {
    worker: task inner()
    if worker.join() == {
        .Ok(value) -> { print("task local {value}") }
        .Err(_) -> { print("failed") }
    }
}
```
