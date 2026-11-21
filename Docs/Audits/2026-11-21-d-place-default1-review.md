# Compiler-chosen placement (D-PLACE-DEFAULT1=A): adversarial review (2026-11-21)

Card #6980 c1 asks for this review before any build starts. On 2026-11-16 the
owner ratified option A of D-PLACE-DEFAULT1, "The compiler chooses":

- A growing value goes on a scope stack unless it is stored in something that
  outlives it or sent to another task.
- A returned value is built on its caller's stack.
- Each growing value gets its own stack and is freed at its last use with one
  pointer reset.
- A recursive function's growing locals use the heap.
- Tasks and the web use chunked stacks, which they take from the thread's pool
  on first use.
- Placement is a sema fact, and `jet inspect placement` names each heap value.

The decision's gains are "stack speed to the common shapes", "the fast path
reaches code nobody tuned", "freed at its last use", "a task that stacks
nothing costs nothing" and "the chunk check adds no new work". This review
takes each prior design at its best, then tests each claimed gain with a probe.

**Verdict: does not pass. The build must not start.** The rule is sound where
it falls back to the heap. Recursion, sends and the web cost no more than today.
But every gain the decision claims is either unmeasured, or measured smaller
than claimed:

- Today's nearest mechanism, an arena block, is 1.4 to 2.7 times slower than
  the heap on allocation-heavy kernels. Nothing in the runtime gives stack
  speed yet, so no probe can show it.
- On the decision's own story, the rule keeps the 12,000 word strings per
  round on the heap, because they are stored in the list. The list itself
  makes 12 of the 14,012 allocations in a round.
- The returned-value rule needs a calling convention that no ballot has chosen.
- Today's task scratch arena is the "reserve per task" design that the decision
  rejected: 256 MiB of address space per task.

The twelve gaps below each end on a proposed criterion for this card or on a
ballot.

## Method

The probes live in [PlaceDefault20261121/Probes](PlaceDefault20261121/Probes).
[run.sh](PlaceDefault20261121/Probes/run.sh) builds each one at the levels it
names, runs it pinned to cores 16-31, and prints each command with its output.
The log of the run this review cites is
[run-p400.log](PlaceDefault20261121/Probes/run-p400.log). It used
`jet@a0.0.46#f5afa328…` (gate p400) and node v24.21.0, at a load average of
0.83 at the start and 1.05 at the end.

No scope-stack runtime exists, so the probes bound A from both sides:

- **Today's heap** (`heap` mode) is what A replaces. Its cost is the most a
  stacked value can save.
- **Today's explicit routes** run the same call inside
  `#Context(allocator: context.scratch)` (`scratch`) or
  `#Context(allocator: arena)` (`arena`). Each serves values from one
  reservation by bumping a cursor and rewinding at the block's end
  ([spec](../Spec/spec.md) "context.scratch is the task's scratch arena").
  This is the closest thing the runtime has to a scope stack.
- **A kernel's no-allocation twin** gives the floor. `reuse` refills one list
  instead of making 12,000, and `reserved` reserves capacity before it pushes.
  The difference between a kernel and its twin is what allocation or growth
  costs.

Times are single runs of 2,000 rounds of 12,000 items unless a row says
otherwise. The web probe ran three times on each engine. Allocation counts come
from `jet perf run --lens=alloc`. That run makes 50 warm-up rounds and then 10
measured rounds, so its per-round figures divide by 60.

## Prior designs at their best

### Goose

Goose is the decision's model. Every growing value lives on one of N data
stacks, and the compiler derives N
([mining run](../Research/mine-for-jet-2026-10-23-goose-lobster.md) goose-001,
goose-064).

- **The stacks.** Each stack is an address reservation with a bump pointer.
  At most one resizable value is live on a stack, and it is always on top, so
  growth never moves it or checks capacity. Scope exit frees everything above
  a saved mark in one store (goose-023).
- **The results.** Over 16 benchmarks Goose reports a geometric-mean speed of
  3.51 times idiomatic C++, 1.24 times hand-optimized C++ and 1.13 times safe
  Rust. It reports 1.25 to 1.95 times less peak memory (goose-015). Those
  results cover the whole language (packed layouts, inlining, its own
  lowerings), not data stacks alone.
- **What it pays.**
  - Resizable values can't be array elements, and a struct holds at most one
    (goose-024).
  - Recursion is opt-in, and while a growing local is live it can't call back
    into its cycle (goose-048).
  - Specialization keys on roots and stacks over the whole program
    (goose-051).
  - On WebAssembly every growth needs a capacity check, which Goose estimates
    at 1.1 to 1.5 times (goose-143).
  - About 60% of its 2,255 rejection lines are lifetime or aliasing errors
    (goose-220).

Lesson for A: Goose's speed comes from stacking *everything*, under
restrictions A rejects. A must show its own gain on the values its rule
actually stacks.

### The ML Kit

The ML Kit inferred regions for all of Standard ML (Tofte, Birkedal, Elsman,
Hallenberg, "A Retrospective on Region-Based Memory Management", 2004,
<https://elsman.com/mlkit/pdf/retro.pdf>).

- **At its best.** Multiplicity inference put the large majority of
  allocations (often 90% or more) in finite regions, which need no run-time
  size, and made programs about three times faster. In the 1,000-line
  benchmark `simple`, 99% of allocations went to finite regions.
- **Its record.** Space use ranged from 8% to over 3,000% of SML/NJ's. Making
  programs "region friendly" took experts and time. In the 58,000-line
  AnnoDomini, the region profiler found the leaks, and rewriting about ten
  lines fixed them. Infinite regions fragmented. The storage-mode analysis for
  tail recursion was complicated and fragile. A garbage collector was added
  later, and region waste reached 23% under it.
- **The authors' own conclusion.** Leaving region inference entirely to the
  compiler "is probably not a good idea" unless it is combined with a
  collector.

Lesson for A: A keeps the heap as its collector-equivalent, which is right. The
gain lived in values that are made and dropped one at a time. The fix for
leaks was a profiler that showed placement. A's report is `jet inspect
placement`, which names only heap values, and today's alloc lens can't see
stacked ones (G10).

### Cyclone

Cyclone had explicit, lexically scoped dynamic regions, `region r { ... }`,
freed in LIFO order. Each new page was twice the previous one (Grossman et al.,
"Region-Based Memory Management in Cyclone", PLDI 2002,
<https://www.cs.cornell.edu/Projects/cyclone/papers/cyclone-regions.pdf>).

- **At its best.** Inference was intraprocedural, and defaults covered
  separate compilation. Porting changed about 8% of the code, and region
  annotations were 6% of those changes. Across 110,000 lines, one function
  needed a non-default effect. Network programs ran as fast as C, and
  compute-bound ones up to three times slower.
- **Its defaults.** A function's return type defaults to the heap region,
  because a separately compiled caller can't know the callee's regions.

Lesson for A: Cyclone made the choice A reverses. A builds a returned value on
the caller's stack, so every function that returns a growing value needs to
know the caller's stack. That is a calling-convention decision (G4).

### Rust (bumpalo) and Zig (ArenaAllocator)

- **bumpalo** (<https://github.com/fitzgen/bumpalo>) is a bump arena that frees
  "en masse" and runs no `Drop` at reset. Its own `boxed::Box` runs `Drop`
  when it is dropped. That is the explicit route Jet already has with `#Context`.
- **Zig's ArenaAllocator** (`lib/std/heap/arena_allocator.zig`, master, lines
  7-10 and 226-230) frees an item only "if it was the most recent allocation".
  Resize can't extend an allocation that is not the most recent one.

Lesson for A: both are opt-in, so a programmer decides that the lifetimes nest.
Both can grow in place only for the newest allocation, which is the condition A
relies on when it says "extending in place when the value is on top". A gives
each value its own stack so that the condition always holds. That is why the
number of stacks and their reservations matter (G5).

## The claims tested

### Allocation cost: "stack speed for the common shapes"

[alloc.jet](PlaceDefault20261121/Probes/alloc.jet), ns per round of 12,000
items:

| kernel | O0 heap | O0 scratch | O0 arena | O1 heap | O1 scratch | O1 arena | O2 heap | O2 scratch | O2 arena |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| words (Noor's count_words) | 397,871 | 879,006 | 821,971 | 435,476 | 895,839 | 837,347 | 397,479 | 820,142 | 879,494 |
| count (split, nothing kept) | 286,330 | 723,690 | | 302,823 | 779,108 | | 268,924 | 724,966 | |
| small (12,000 two-item lists) | 201,788 | 280,668 | | 185,814 | 271,241 | | 178,613 | 308,753 | |
| reuse (one list refilled) | 41,875 | 43,478 | | 28,028 | 38,462 | | 31,572 | 31,896 | |
| ints (one list, 12,000 pushes) | 23,470 | 23,486 | 22,268 | 23,538 | 19,663 | 18,858 | 12,959 | 15,360 | 16,224 |
| reserved (same, capacity first) | 18,579 | 20,867 | | 18,968 | 17,849 | | 12,803 | 12,962 | |

What the table shows:

- **A heap allocation and free cost 12 to 13 ns.** `small` minus `reuse` is
  12.3 ns per list at O2, 13.1 at O1 and 13.3 at O0. The alloc lens shows each
  short list is one 40-byte block. One, two or four pushes, or the literal
  `[1, 2]`, all take one block ([pushes.jet](PlaceDefault20261121/Probes/pushes.jet),
  log lines 263-267). That 12 ns is the most a stacked short-lived list can
  save.
- **Growth itself is cheap on the heap.** `ints` minus `reserved` is 156 ns
  per 12,000 pushes at O2 (1.2%). The web probe below measures 11% for the
  same kernel. A's "growth never moves" saves at most that.
- **Today's bump routes lose.** At O2, `words` runs 2.06 times slower under
  `scratch` and 2.21 times slower under `arena` than on the heap. `count` runs
  2.70 times slower and `small` 1.73 times slower. The cause is in the
  runtime: while a block's allocator is current, the thread's cache rows are
  blocked, so every allocation takes the full path and asks the region record
  (Compiler/JetRuntime/Source/Alloc.jet, the comment before
  `ALLOC_USER_TAKE`). A cursor bump that costs less than 12 ns has never been
  measured in this runtime.

Verdict: **not shown.** The ceiling is about 12 ns per short-lived value. The
only bump path the runtime has costs more than the heap. G1 asks for a
prototype that beats the heap before the build.

### Noor's story: what the rule stacks in its own example

The lens on `words` (log lines 191-193) counts 840,000 allocations at the split
line (main.jet:22), which is 14,000 a round. The list's own growth is 720
allocations, 12 a round, and allocates 0.26 MB a round. `count`, which keeps no
words, makes the same 14,000 a round (line 211).

Under A's rule, the 12,000 words a round are pushed into `words`, which is
older than each word. So "stored in something that outlives it" sends them to
the heap. Only the list, and whatever the split makes and drops in each
iteration, can be stacked. The decision's `trimmed` example stores
`line.trim()` into `out` in the same way. Keeping the words costs 128.6 µs a
round at O2 (`words` minus `count`, 10.7 ns a word), and the rule leaves all of
it on the heap. Goose avoids this by forbidding resizable array elements
(goose-024). A's rule doesn't say what happens to an element of a stacked
container.

Verdict: **gap.** The rule must state where elements and fields of a stacked
value go (G2). A census must show what share of real programs' allocations
the rule stacks (G3). The ML Kit's 90% is a number of exactly that kind, and
A has none.

### Peak memory on a long scope: "freed at its last use"

[peak.jet](PlaceDefault20261121/Probes/peak.jet) runs 200 rounds, each of which
builds a 100,000-Int list (800 KB):

| mode | O0 peak RSS | O0 ns/round | O2 peak RSS | O2 ns/round |
| --- | --- | --- | --- | --- |
| heap, freed at last use | 2,396 kB | 146,616 | 2,408 kB | 163,955 |
| round: one arena block per round | 2,348 kB | 189,184 | 6,404 kB | 150,334 |
| scope: one arena block for all rounds | 264,252 kB | 231,904 | 264,264 kB | 203,000 |

- Freeing at the scope's end costs 110 times the peak (264 MB against
  2.4 MB). So freeing at last use is a requirement, not a refinement. A keeps
  it by giving each value its own stack.
- Opening an arena block moved the process's address space from 4.4 MB to
  16.0 GiB (VmSize 16,781,932 kB). Heap mode makes the same `mem.Arena` but
  never opens a block, and it stays at 4.4 MB. Each further arena reserves
  `ALLOC_ARENA_BYTES`, 256 MiB (Alloc.jet).
- A has one stack per simultaneously live growing value, on every thread. So
  the reservation per stack and the bound on the number of stacks decide the
  address space, and the decision states neither.

Verdict: **the claim is right and necessary, but its cost is unstated** (G5).

### Returned values built on the caller's stack

There is no probe, because no mechanism exists. A callee that builds its
result on its caller's stack has to learn which stack that is. Goose passes it
as a hidden argument and specializes over the whole program (goose-051,
goose-064). Cyclone gave up and defaulted returns to the heap.

In Jet the same function is reached through function values, trait objects,
closures, plugin guests, C exports, `jet dev`'s hot patch and the evaluator in
`jet debug`. Each one must pass the stack, or the value must fall back to the
heap.

Verdict: **gap, and it needs a ballot** (G4). It is a calling-convention
choice with user-visible cost at every indirect call.

### Tasks: "a task that stacks nothing costs nothing"

[tasks.jet](PlaceDefault20261121/Probes/tasks.jet) starts N tasks that each
wait on a channel (O2, log lines 306-327):

| tasks | none: VmSize / RSS | heap: VmSize / RSS | scratch: VmSize / RSS |
| --- | --- | --- | --- |
| 1,000 | 16.7 GB / 8.9 MB | 16.7 GB / 9.3 MB | 279 GB / 163 MB |
| 10,000 | 164 GB / 70 MB | 164 GB / 73 MB | 1.74 TB / 450 MB |

- A task's call stack reserves 16.3 MiB and keeps about 8 KB resident. That
  is what "costs nothing" means today, and A doesn't change it.
- A task that touches `context.scratch` reserves a further 256 MiB and keeps
  154 KB resident (1,000 tasks). That is the "reserve per task" design the
  decision rejected, and it is shipping today.
- [INFERENCE] At 256 MiB per task, about 500,000 scratch-using tasks fill a
  47-bit address space.

Verdict: **gap.** A's chunked stacks must replace the per-task scratch
reservation, not sit beside it, under the one-mechanism rule. The probe must
then show that stacked tasks stay near today's `none` row (G6).

### Values that cross tasks: "or sent to another task"

[sends.jet](PlaceDefault20261121/Probes/sends.jet), O2, ns per send:

| list size | heap | built in scratch, moved out at the send |
| --- | --- | --- |
| 16 | 889 | 1,472 |
| 1,000 | 3,971 | 3,000 |
| 100,000 | 118,815 | 264,182 |

A value that is stacked and then sent has to be copied out. That costs 1.66
times at 16 items and 2.22 times at 100,000. The 1,000-item row reversed in
this single run. So the rule must stay static and conservative: a value that
is sent on any path goes to the heap on every path. The rule as written says
this, and it costs exactly today's price.

Verdict: **pass, with a case to hold it** (G7).

### Recursion: "growing locals use the heap"

[recur.jet](PlaceDefault20261121/Probes/recur.jet), depth 14, 32,767 nodes,
65,546 allocations and 27.8 MB a walk (log line 387):

| mode | O0 ms/walk | O2 ms/walk |
| --- | --- | --- |
| heap | 4.26 | 3.30 |
| scratch | 4.52 | 4.75 |

Sending recursion to the heap costs nothing against today, and it keeps
recursion unrestricted, unlike goose-048. What it gives up is the gain on
recursive builders: parsers, tree walks, and most of this compiler.

Verdict: **pass.** How much reach it gives up belongs in the census (G3,
split by rule).

### The web: "the chunk check adds no new work"

[web.jet](PlaceDefault20261121/Probes/web.jet), 12,000 pushes from empty
against reserved capacity, three runs each:

| engine | grown µs/round | reserved µs/round | growth over reserved |
| --- | --- | --- | --- |
| native O2 | 16.37, 16.68, 16.42 | 15.09, 15.25, 14.20 | 11% |
| V8 (`--os=web`, node) | 30.39, 30.35, 33.59 | 27.05, 27.21, 30.00 | 12% |

Growth costs the same share on both engines. It already compares length with
capacity, so a chunk end can take the place of the capacity in that compare.
Extending in place adds one test, "is this value on top?", and only on the
growth path. Goose's 1.1 to 1.5 times (goose-143) is a cost against Goose's
unchecked native growth, and Jet doesn't have unchecked growth to lose.

Verdict: **pass, with a criterion to keep it true** (G9).

### I9 parity, `#Context` precedence and Mem.Stack budgets

- Placement as a sema fact makes O0, O1, O2 and the web place values alike
  (I3, I9). That holds only if last-use analysis is sema's, not the
  optimizer's (G11).
- A named `#Context` allocator winning matches the spec, under which a block's
  allocator serves the values it makes.
- Scope stacks separate from the native call stack leave Mem.Stack budgets as
  they are.

Verdict: **pass in design.** Cases must check parity (G11).

### Observability and checking

Stacked bytes don't go through the heap. So the alloc lens (which counts heap
calls at their call sites), heap snapshots and checked mode's quarantine stop
seeing them. Use after a last-use reset is a stale read inside a live stack,
and the heap quarantine can't catch it. Goose's record says the same of
sanitizers on its stacks (goose-166 in the mining run).

The ML Kit's leaks were found with a region profiler, so this is the tool A
needs most. `jet inspect placement` names only heap values.

Verdict: **gap** (G10). The criterion also covers soundness at last use (G12).

## Verdict table

| claim | probe | result | verdict |
| --- | --- | --- | --- |
| stack speed for common shapes | alloc, pushes | ceiling 12-13 ns per short-lived value; today's bump route is 1.4-2.7 times slower than the heap | not shown (G1) |
| the fast path reaches code nobody tuned | alloc lens | the story's 12,000 words a round stay on the heap; the list is 12 of 14,012 allocations | not shown (G2, G3) |
| freed at last use | peak | scope-end free is 110 times the peak, so last-use free is required; reservations unbounded | holds, cost unstated (G5) |
| returned values on the caller's stack | none possible | no calling convention exists | gap, ballot (G4) |
| a task that stacks nothing costs nothing | tasks | true today for `none`; task scratch reserves 256 MiB per task | gap (G6) |
| sent values use the heap | sends | stack-then-move costs 1.7-2.2 times | pass (G7) |
| recursion uses the heap | recur | heap 3.30 ms against scratch 4.75 ms at O2 | pass, reach in G3 |
| the chunk check adds no new work | web | growth is 11-12% on native and V8 alike | pass (G9) |
| placement is a sema fact (I9) | design | holds if last use is sema's | pass in design (G11) |
| no new surface but `jet inspect placement` | design | stacked values leave the alloc lens and checked mode | gap (G10, G12) |

## Gaps

Each gap names the proposed criterion for #6980, or the ballot it needs. The
build starts when G1 to G6 are met and this review is revised to pass.

- **G1. No stack-speed path exists.** Criterion: a prototype scope-stack
  allocation path, with the thread's cache rows left in place, runs
  alloc.jet's `small`, `count` and `words` at O0, O1 and O2 faster than
  `heap` by a stated margin. The run is logged beside this review.
- **G2. Elements and fields of a stacked value.** Criterion: the spec states
  "outlives" for list elements, map values, struct fields and boxed links.
  Cases show a word pushed into a stacked list, a field of a returned struct,
  and a list of lists, each with the placement `jet inspect placement`
  reports. If the answer stacks elements, it needs Goose's element
  restriction or a second mechanism, and that goes to a ballot.
- **G3. A census of reach.** Criterion: `jet perf run --lens=alloc` on three
  real programs (a compiler package's `jet check`, a Core test, a web
  example) splits allocations by the rule that places them: stacked,
  stored-in-older, sent, recursive, returned through an indirect call. The
  stacked share is published with the census. The gain claims stand or fall
  on that share.
- **G4. The calling convention for returned values.** Ballot: how a callee
  learns its caller's stack. The choices are a hidden argument on every
  function that returns a growing value, specialization by destination, or
  the heap for any value returned through an indirect call (function values,
  trait objects, closures, plugin guests, C exports, hot patch, the
  debugger's evaluator). The ballot states the cost of each choice at an
  indirect call.
- **G5. Stack count and reservation.** Criterion: the design states the most
  stacks a thread holds and each stack's reservation. A 64-thread probe
  reports VmSize and RSS. Today the first arena block a process opens costs
  16 GiB of address space, and each further arena 256 MiB.
- **G6. Task stacks replace task scratch.** Criterion: chunked task stacks
  replace `context.scratch`'s 256 MiB per-task reservation as the one
  mechanism. tasks.jet with 10,000 tasks that each stack a value stays within
  twice the `none` row's VmSize and RSS.
- **G7. Sent values stay on the heap on every path.** Criterion: a case where
  a value is sent on one branch only shows it placed on the heap, with
  `jet inspect placement` naming "sent".
- **G8. Recursion's reach.** Folded into G3's split by rule. No separate
  criterion.
- **G9. Web growth.** Criterion: web.jet's grown-against-reserved share on V8
  stays within one point of today's 12% once stacks are chunked.
- **G10. Stacked values stay visible.** Criterion: the alloc lens and the
  heap view report stacked bytes by call site and by placement rule.
  `jet inspect placement` names stacked values as well as heap ones. Checked
  mode poisons a stacked value's bytes at its reset and reports a later read
  as a use after free, with the value's line.
- **G11. Parity.** Criterion: `jet inspect placement` prints the same
  placement for a fixed program at O0, O1, O2 and `--os=web`, and a case
  holds that output.
- **G12. Last use includes every view.** Criterion: a stacked value's last
  use counts every view, borrow and iterator over it. Every exit (return,
  break, `?` propagation, a stop) resets its stack. Cases cover a view kept
  past the owner's last plain use and an early return.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| G1 no stack-speed path exists | card | #6980 (proposed criterion: prototype beats heap on alloc.jet) |
| G2 elements and fields of a stacked value | card | #6980 (proposed criterion: spec states "outlives" with cases) |
| G3 census of reach | card | #6980 (proposed criterion: alloc-lens census split by rule) |
| G4 calling convention for returned values | card | #6980 (ballot: how a callee learns its caller's stack) |
| G5 stack count and reservation | card | #6980 (proposed criterion: stated bound and 64-thread probe) |
| G6 task stacks replace task scratch | card | #6980 (proposed criterion: one mechanism, tasks.jet bound) |
| G7 sent values stay on the heap | card | #6980 (proposed criterion: one-branch send case) |
| G8 recursion's reach | no-action | folded into G3's split by rule |
| G9 web growth | card | #6980 (proposed criterion: web.jet growth share) |
| G10 stacked values stay visible | card | #6980 (proposed criterion: alloc lens, heap view, checked mode) |
| G11 placement parity | card | #6980 (proposed criterion: inspect placement equal on every level) |
| G12 last use includes every view | card | #6980 (proposed criterion: views and exits cases) |
<!-- /audit-dispositions -->
