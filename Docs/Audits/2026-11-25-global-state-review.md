# Adversarial review: changeable module globals for group workers, pure code, caches and tests

Date: 2026-11-25. Card: #7032, criterion 1.

Subject: owner ballot D-GLOBAL-SAFE1 (greenfield proposal item P10). The
owner direction of 2026-11-25 is binding: keep changeable module globals,
because games and simple programs need them, and engineer away every
downside instead of replacing them. The draft proposed one touch fact per
function, group workers that borrow globals, purity and `#Memo` rules that
see globals, and a per-test reset. Sites were read at dev `8f4309b5023`
(`.agent-worktrees/Main-dev-collectall`); the draft cited `4b48f2bc485`, 57
commits older. Probes ran on the installed `jet a0.0.46`, which spells the
ratified `^`/`^^` marks as `&`/`&&`, from single files in a scratch directory
that was deleted afterwards. Decisions and cards were read with
`node Tools/Tower/tower.mjs decision show` and `card show`. A fresh agent made
this review and repaired the ballot in place; each verdict below is for the
repaired text, and the finding that forced each repair is stated.

## The strongest existing solutions, at their best

- **Rust** (Edition Guide, "Disallow references to static mut",
  https://doc.rust-lang.org/edition-guide/rust-2024/static-mut-references.html;
  `std::thread::scope`, https://doc.rust-lang.org/std/thread/fn.scope.html).
  A `static mut` is touched only in `unsafe`; since the 2024 edition the
  `static_mut_refs` lint denies taking a reference to one by default (it can
  be allowed). Safe globals are `Mutex`, atomics or `thread_local!`. Scoped
  threads "can borrow non-'static data" until they join, which is the shape
  A gives Jet's group workers, but no safe code can lend a `static mut` to
  them. Cost: every safe global pays a lock or an atomic, every access.
- **Swift 6** (Swift 6 migration guide, "Common compiler errors",
  https://www.swift.org/migration/documentation/swift-6-concurrency-migration-guide/commonproblems/).
  A plain global `var` is "not concurrency-safe because it is non-isolated
  global shared mutable state" unless it is isolated to a global actor such as
  `@MainActor`, or marked `nonisolated(unsafe)`. Other code reaches it with
  `await MainActor.run { … }`. Strength: checked while compiling. Cost: an
  annotation on every global and an `await` hop from every other context.
- **D** (D language spec, "Pure functions",
  https://dlang.org/spec/function.html#pure-functions). A `pure` function may
  not read or write mutable global or static state ("error, reading mutable
  global state"), and may read immutable globals. This is exactly A's rule
  for `-[]>` code, with decades of use.
- **Kotlin/Native** ("Migrate to the new memory manager",
  https://kotlinlang.org/docs/native-migration-guide.html). The legacy memory
  manager confined top-level state to the main thread unless marked
  `@SharedImmutable`, and checked it while running. The new manager, the
  default since Kotlin 1.7.20, lets any thread change top-level properties.
  It is the strongest evidence that main-thread confinement causes friction;
  the legacy model failed at run time and needed freezing, which Jet's
  build-time check and group loans avoid.
- **Go** (race detector, https://go.dev/doc/articles/race_detector). Package
  variables are plain and shared by parallel tests; the race detector "only
  finds races that happen at runtime", so untested paths stay unchecked.
- **Zig, Odin and Jai** (Zig language reference, "Container Level Variables";
  Odin overview, https://odin-lang.org/docs/overview/; Jonathan Blow,
  https://www.youtube.com/watch?v=OHZwYYW9koI at 00:18:04, and
  `Docs/Audits/jai-parity-2026-10-16.md`). Globals are plain and fast; a
  thread-local attribute gives one copy per thread; nothing checks tasks or
  tests. Jai copies globals changed while compiling into the executable, as
  Jet bakes start values (D-JAI-COMPTIME1=A).
- **Bevy** (Bevy cheatbook, "System order",
  https://bevy-cheatbook.github.io/programming/system-order.html). Game-wide
  state is a resource; systems that only read it run in parallel, and one
  that writes it runs alone. A's group loans are the same rule, decided while
  building instead of by a scheduler.
- **cargo-nextest** ("Why process-per-test?",
  https://nexte.st/docs/design/why-process-per-test/). Each test runs in its
  own process, so "singletons become separated per-test" with no
  coordination. Stated costs: process creation is slow on Windows, and tests
  cannot share in-memory state.
- **xUnit** ("Running tests in parallel",
  https://xunit.net/docs/running-tests-in-parallel). Test classes run in
  parallel by default; static fields are one per process, so classes that
  share statics must be put in one collection, which runs them serially.
- **Unity** ("Domain reloading",
  https://docs.unity3d.com/Manual/DomainReloading.html). With domain reload
  off, static fields keep their values between Play mode runs, and scripts
  reset them by hand or with `[RuntimeInitializeOnLoadMethod]`.
- **Fork-per-test snapshots** (considered by this review, not proposed by
  any option). Forking after start-up gives every test a pristine copy for
  free on Linux, but fork does not exist on Windows or wasm, and forking a
  process with live runtime threads is unsafe. Rejected for one meaning
  across tiers.

## What was ratified before this ballot

D-MODULE-VAR1=A (ratified 2026-11-10) has six parts: a file-scope `:=` is a
module variable; one inferred touch fact shown in hover and docs; the main
task owns it and a task touching it, "even to read", is a build error naming
the variable and path; function values carry the fact; no write behind a
reader's back; libraries belong to the main program and each test gets its
own copy through its world ("one extra load per access", "tests stay
parallel"). Card #6859 builds it with criteria C1-C11, all open: touch fact
(C1, C9), task refusal (C2), function writes (C3), cross-call view rule (C4),
generic modules (C5), per-test copies including module Shared holders (C6),
in-place edits and E0223 retirement (C7), `#Persist` races (C8), C callbacks
(C10), and the E0338 row, docs and reference page (C11). D-BIND-MARK1=A
(2026-11-25) spells the global `^score: 0`. D-DEV-STATE1=A retired live code
swap: `jet dev` restarts and keeps only `#Persist` values.

## Claim 1: the ballot decides only what is not ratified

Attack. The draft's table marked the touch fact, in-place edits, C callbacks,
generic modules, the E0338 text and `#Persist` races as "new", and its
cutover "amends card #6859's criteria C1-C5" when #6859 has eleven. Every one
of those is already a #6859 criterion. The draft re-balloted ratified work,
which AGENTS.md forbids, and hid what it actually amends.

Repair. The detail now has an "Already ratified" block citing each rule and
criterion, and a "Decided here" block with five items, each naming what it
amends: group loans (D-MODULE-VAR1 rule 3), other tasks and `#Export(c)`
(new), `-[]>` reads (spec.md:3075-3082), `#Memo` (card #7032 c3), and the test
mechanism (rule 6's mechanism, one sentence of D-CORE-GLOBAL-STATE1, and
output-check behaviour in `Docs/Spec/Reference/cli.md:1169`). The cutover now
lists only this ballot's work.

Verdict: **failed in the draft; holds after repair.**

## Claim 2: the "Today" block is accurate

Attack. Probes on `jet a0.0.46`:

- p1, a function writing `score := 0`: `Jet.ImmutableAssignment` (E0111),
  "`score` is module state, so a function can't change it directly".
  Matches `Compiler/JetSema/Source/Sema/Statements/Bindings.jet:1352-1357`.
- p2/p7, `enemies.push(e)` in a function, with or without `#Unsafe`:
  `Jet.PersistentMutableBorrow` (E0223). Matches.
- p6, `frame[i] = 7` on a `:=` global, even inside `#Unsafe`: E0111 saying
  "`frame` is a constant, so it can't be changed". The draft said E0223. The
  text is also wrong for a `:=` global; #6859 C7 removes the case.
- p4, two `#Test`s that each call `hit(5)` (written inside `#Unsafe`) and
  assert 5: the second fails with "expected: 5, got: 10". Leak confirmed.
- p3, `#Memo cost: fn(x: Int) -[]> Int { x * level }` printed 2 before and 2
  after `level` changed from 1 to 10. The draft said this was "read from the
  code, not run"; it is now run.
- s1, the same with a module `Shared` holder written by `run`: the `#Memo`
  function printed 2 and 2, while the same body without `#Memo` printed 20.
  Stale. `-[]>` reading a holder is accepted today.
- s2, a `#Memo` function with a `Shared` parameter: refused with
  `Jet.MemoArgumentNotHashable`, so parameters are not a hole.
- p5, group children reading a global, a write in the group body and a task
  writing it all compile and print 5. p9, a `#Persist` global written by two
  group children, compiles and prints 3.

Further attacks: the draft said the runner's "per-core children already"
isolate tests. `Docs/Spec/Reference/cli.md:1153-1161` and
`Compiler/JetCLI/Source/CLI/NativeTests.jet:461-539` start children only for
reruns after a fatal trap, isolated (env/chdir) tests and time limits; a
normal batch runs serially in one process. The draft's "the existing world
reset retires them" missed that `jet_rt_global_reset_registered`
(`Compiler/JetRuntime/Source/Shared.jet:614`) is called by no harness, only by
`Tests/shared-publication-fixture.c`.

Repair. The block is restated at `8f4309b5023` with current line numbers,
the probes, the corrected E0111 case, the real runner behaviour and the
uncalled reset.

Verdict: **failed in parts; holds after repair.**

## Claim 3: group workers can borrow globals soundly

Attack. The rule reused is spec.md:4923-4948: reads are unrestricted, write
borrows need non-overlap, distinct fields and constant indexes are disjoint,
dynamic places overlap, two children reaching one place is E1101, and borrowed
windows need sendable elements. Siblings checked:

- Writes through calls. The draft promised "each write separate places". A
  touch fact names a global, not a place, so a write inside a called function
  can only hold the whole global. Repaired: a write in the child's own body
  holds the field or constant index it names; a write through a call holds
  the whole global.
- Types that can't cross tasks. A global holding a `Cell` read by two
  children is a data race through interior change. Repaired: a global is lent
  only if its type may cross tasks, as a borrowed local's must.
- The opener's body runs beside the children; it already joins the E1101
  check for locals, and now for globals.
- Nesting: a child lends only what it holds. Covered.
- `task.all`, `task.race`, `task.any`: spec.md:4934-4935 says branches borrow
  reads until the combinator returns. The draft did not mention them; added.
- `para_map`, `para_filter`, `para_partition`, `para_fold` (spec.md:4824-4847,
  D-PARCAPTURE1=D) join before returning. The draft did not mention them.
  Repaired: callbacks may read globals; a write is E1101, as an ordinary
  changeable capture is. 116 calls in 64 files on dev.
- Lazy start: module `Shared` holders build on first read and keep the first
  published cell (`Compiler/JetIR/Source/BuildSharedSites.jet:5-7`), so two
  children never race on a start. `:=` globals are baked (E0643), never lazy.
- Web: a browser refuses task groups (spec.md:4889-4891).

Verdict: **holds within stated limits** (sendable types; whole-global holds
through calls). Not built; no probe can show it.

## Claim 4: refusing other tasks is sound and the friction is proportionate

Attack. The draft said "a detached task" is refused. In Jet "detach" is
`handle.detach()`, and a handle task started inside a group body is a group
child (`Examples/Concurrency/ScopedBorrowBands.jet`). The true refused set is:
a handle task started outside a group body (it can outlive its starter), a
detached task, and a child spawned through a `Group` parameter. The draft's
reason for the last, "joins in another frame", does not by itself apply to
program-lifetime globals; the real reason is that the opener can't see what
children a callee spawns. Friction: the ratified rule refused every task, so
this ballot only loosens it, and the error offers a third fix, a task group.

Siblings the draft missed. A function exported with `#Export(c)` may be
called by a C host on any thread, exactly the C callback case (#6859 C10).
240 `#Export(c)` sites in 61 files exist outside the runtime, and none touches
a global today. Repaired: it joins the C callback row unless every global it
touches is a raw word. Core callbacks that run on other threads (interrupt
handlers on the dispatcher thread and realtime audio callbacks, per
D-CONTEXT-THREAD1) are function values, so ratified rule 4 already requires
them to touch nothing; the ballot now names them.

Alternative considered: a Swift-style hop, where a task sends a write to the
main task. Jet's main task has no event loop to receive it, and a channel
already spells this explicitly. Rejected.

Verdict: **holds after repair.**

## Claim 5: pure code may not read changeable state

Attack 1, the sibling. The draft limited the read rule to `:=` globals. Probe
s1 shows a module `Shared` holder gives the same stale result, and E3401
already lists holders, Cells and `#Persist`/`#Unsafe` bindings for writes
(spec.md:3075-3082). Repaired: the read rule covers the same list.

Attack 2, collateral. The debugger checks a watched expression "as
`fn() -[]>`" and refuses only writes
(`Compiler/JetSema/Source/Sema/EvalProbe.jet:436-447`); `jet eval --pure`
runs the same check. Widening E3401 would stop the debugger from showing any
expression that reads a global. The draft's "no rule walks bodies a second
time" was also false: `sema_eval_held_state_writer`
(`EvalProbe.jet:531-551`) is its own call-graph walk. Repaired: the debugger
and `--pure` keep a writes-only check, and the duplicate walk reads the touch
fact and is deleted.

Attack 3, friction. A pure function that needs a global takes it as a
parameter. D has required that for decades. No `-[]>` or `#Memo` body names a
same-file global outside the E3401 tests (drafter's census); holder reads
across files were not counted.

Verdict: **holds after repair; cross-file holder reads unmeasured.**

## Claim 6: `#Memo` can never return a stale result

Attack. Card #7032 c3 asks the ballot to choose "refusal or keying the cache
on the global". The draft offered only refusal, and its B whyNot said "stale
`#Memo` results remain", although c3 requires a fix whatever wins. Repaired:
B now carries the refusal; a new option C keys the cache on a write count per
global, extending how a `#Memo` key already holds context values
(`Examples/Concurrency/ContextReads.jet`). C's honest limits: one add per
write to a cached global in shipped builds, holders and Cells still refused
(their writes travel through clones the compiler can't follow), and a `-[]>`
function can answer differently for equal arguments. A also clears `#Memo`
caches between tests, which the draft missed (`cost.cache()` statistics
would leak).

Verdict: **holds after repair, with refusal (A) and keying (C) both offered.**

## Claim 7: each test starts fresh, with no cost in shipped builds

Attack 1, ratified text. D-CORE-GLOBAL-STATE1 says no process-wide "clear
before each test" workaround is allowed; the restore is one.
D-TEST-SCHEDULES1 runs reruns "side by side on the profile's workers". The
draft amended neither. Repaired: both amendments are stated, and the workers
are processes.

Attack 2, the runner. Under A one process runs one test at a time. Parallel
tests need a file's tests split across per-core child processes, which is
new work, not "as the runner's per-core children already do". The ratified
world pointer would also have needed new parallel-runner work, so this is not
a loss against B.

Attack 3, output checks. `cli.md:1169` documents that output checks see the
batch's state, and `OutputCheckObservesTheJoinedBatchState.fixture` checks a
`#Persist` global the tests wrote. The draft claimed the fixture already reads
files; it reads the global. With tests in several processes there is no batch
state. Repaired: a check starts fresh and reads files; the fixture is
rewritten.

Attack 4, mechanism gaps. Restoring needs the whole baked value, including
the data it points to, or an in-place `frame[i] = v` survives. The live value
must be dropped first (I1 leak freedom). A detached task left by one test
could re-read a holder after the reset and publish a cell into the next test;
repaired, a test ends when its tasks end. The reset is uncalled today. A
`testing.world` (`Docs/Spec/Reference/core-library.md:5387`) shares its
test's globals under A. Core's 18 lifetime sites must each be classed World
or Process; `core-library.md:6394` calls the app queue "one process-wide
cell" in test builds.

Attack 5, cost. Shipped builds: nothing; the restore and copies exist only in
test builds. Test builds: one copy per dirtied, touched global per test, and
one child start per core. Neither was measured.

Verdict: **holds within stated limits; costs unmeasured.**

## Claim 8: hot reload and start order are covered

Attack. The draft's row "a swap keeps a live value unless its start value
changed" contradicts D-DEV-STATE1=A: live code swap is retired, `jet dev`
restarts on every save, and "an edited starting value does not replace a
kept value". Repaired: plain globals start over at each save; `#Persist`
survives; a future swap engine must wait until no group lending a global is
open. Start order: start values are baked (D-JAI-COMPTIME1=A), E0643 refuses
code before `run`, and E0338 covers cycles (#6859 C11). Nothing new to decide.

Verdict: **failed in the draft; holds after repair.**

## Claim 9: libraries, generic modules, C callbacks and raw words

Attack. "A global written inside `#Unsafe` becomes process state" was
ambiguous, and E3401 already refuses `#Unsafe` binding writes in `-[]>`.
Repaired: a global whose every write sits inside `#Unsafe` is a raw word; it
skips the task check and the test restore, purity and `#Memo` still see it,
and one plain write makes it tracked. Libraries: one copy per program
(ratified); the touch fact must travel in a package's published interface
[inference; the package format was not read]. Plugins never see host globals
[inference from D-PLUGIN-ENGINE1=A]. Generic modules and C callbacks are
#6859 C5 and C10.

Verdict: **holds; two inferences stated.**

## Claim 10: the example code is valid Jet

Attack. `spawn: fn(e: Enemy) { enemies.push(e) }` is refused even on a local
list: probe r5 gives `Jet.MissingMoveMarker`, "`e` cannot be moved from
borrowed storage … declare the parameter `e: &&Enemy`" (`^^` on dev). A
worker loop `loop e in enemies -> task think(e)` over a local list ran and
printed `n 1` (probe r4). `{ hit(5); assert_eq(score, 5) }` is legal under
D-SEMI-LINE1 but refused by today's compiler (`Jet.ExplicitSemicolon`), and
D had the same. Repaired: `e: ^^Enemy` everywhere, line breaks in bodies.

Verdict: **failed in the draft; holds after repair.**

## Claim 11: the options are fair and none is missing

Attack. Option C (context slots) was the greenfield item the owner rejected
on 2026-11-25 (card #7032 body); it filled a slot a live alternative should
take. No option tracked writes instead of refusing reads, although c3 names
keying. B lacked the `#Memo` fix and misdescribed in-place edits as unruled
(they are #6859 C7). Repaired: C is now "as A, but caches track writes"; B is
the ratified rule plus the cache fix; D (worlds everywhere) stays. Rust's
scoped threads, D's pure functions, Kotlin/Native's abandoned confinement and
cargo-nextest are added to the comparisons; Austral, which only illustrated
the dropped option, is removed.

Verdict: **holds after repair.**

## Claim 12: no cost against the performance gate

Attack. Under A every new rule runs while building; release, run and dev
builds carry no counter, no indirection and no copy. Compile cost: one more
set per function in the walk E3401 already does, skipped when no changeable
global exists (`Compiler/JetSema/Source/Sema/Effects/Checks.jet:7069-7085`).
C adds one add per write to a cached global in shipped builds; D adds one
load per access. Neither is measured, and A's claim needs no measurement
because nothing is emitted.

Verdict: **holds for A.**

## Gaps

1. Parallel tests need new runner work (per-core child processes splitting a
   file's tests). Unbuilt, unmeasured.
2. Restore cost per test and child-process start cost: unmeasured.
3. Holder reads in `-[]>` and `#Memo` functions across files: uncounted; the
   built check finds them in the sweep.
4. The touch fact in a package's published interface: inference.
5. Plugins never seeing host globals: inference from D-PLUGIN-ENGINE1=A.
6. Any future code-swap engine must respect open group loans; none exists.
7. Each of Core's 18 lifetime sites must be classed World or Process.
8. The E0111 text "is a constant" for a `:=` global is wrong today; #6859 C3
   and C7 remove the case rather than reword it.
9. This audit must move to `Docs/Audits/` for card #7032 criterion 1.

## Cutover owned by this ballot

Group loans in `Compiler/JetSema/Source/Sema/Ownership/TaskGroups.jet` and
`Ownership/Crossings.jet`, for groups, combinators and para adapters; the
`#Export(c)` case in `Calls/Checking.jet`; E3401 and E0938 widened in
`Effects/Checks.jet`; the debugger's duplicate walk in `EvalProbe.jet:531-551`
deleted; the restore, reset call, leftover-task join and `#Memo` clear in
`Compiler/JetIR/Source/BuildTests.jet` and the runtime test harness; World
sites for module holders in `BuildSharedSites.jet`; per-core test children in
`Compiler/JetCLI/Source/CLI/NativeTests.jet`, whose batch-state special cases
go; the registry rows in `Compiler/Prelude/Diagnostics.jet`; spec, reference
and an example. Everything else is #6859's.

## Summary

The draft had sound core ideas but re-balloted ratified work, contradicted
two ratified decisions and the runner, missed holders, combinators, para
adapters, exported C entries and the debugger, misdescribed hot reload, and
shipped invalid example code. All of it is repaired in the ballot, which now
lints with both gap lists empty. A stays recommended: it is the only option
that lets group workers read globals, keeps caches honest without a shipped
cost, and starts each test fresh with nothing added to release builds. The
ballot is ready to post once this audit lands in `Docs/Audits/`.
