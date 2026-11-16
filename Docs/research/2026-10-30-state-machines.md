# State machines in Jet today

Date: 2026-10-30. Card #6326. Probes live in
`$JET_SCRATCH_ROOT/lanes/SurfaceDesign/probes/` (`m6326`, `m6326b`, `jt`).
Every probe printed the same output on o0, o1 and o2 with the base compiler at
bd83fe6361, except where a timing is given.

## What Jet offers

**Enums and exhaustive dispatch.** A closed enum with payloads plus
`if state == { … }` is the everyday machine. A missing case is E0307 "This
`if` doesn't cover every case — missing: Yellow" (`m6326/s02`). A traffic
light (s01) and a payload connection machine (`m6326b/t09`) run on all tiers.
An ordered guard table over one enum should get L0302 (D-IFGUARD1=A,
`syntax-decisions.md:819-831`). The row exists but no emitter fires (s09,
`--verbose` included).

**Loops.** `loop state != .Done { … }` and the two-slot state loop
`loop l := Light.Red, l != .Yellow { … }` both run (s13). The header form
is `loop i := 0, i < n` (`syntax-decisions.md:640`). L0529 should warn when
the body never changes the header (lines 658-662); no emitter was found.

**Timeouts.** A subjectless `if` whose arms are `value, receiver -> …` and
`after 20ms -> …` is one atomic readiness wait (D-CONC-CHAN2=D,
`syntax-decisions.md:3150-3158`). A machine that times out waiting prints
`TimedOut` (s11). `Examples/features/concurrency/select_channel.jet` is the
shipped example.

**Typestate.** D-STATE1 puts `state { … }` inside a struct
(`syntax-decisions.md:1525-1529`). `#State(S)` methods require a state and
`#Transition(From, To)` methods move between states. The markers erase at
run time. Probed with the `#Transition` spelling:

| Case | Probe | Result |
| --- | --- | --- |
| Call a `Confirmed` method on a `Draft` value | t01 | E0150, names the state and the transition to call |
| Call a `Draft` method after `confirm()` | t02 | E0150 |
| Typo in a state name | t07 | E0151 "`Fersh` is not a declared state" |
| A state no entry path reaches | t04 | L0153 under `--verbose` (D-STATE-TERMINAL1=A) |
| `if flag -> o.confirm() else -> o`, then a `Confirmed` call | t03 | E0150: `o` may be in state `Draft` |
| A parameter `o: Order` calling a `Confirmed` method | t06 | accepted; a parameter has no state |
| A `:=` typestate value inside a loop | t08 | accepted |

The checker tracks a local's state along straight-line code and through
joins: where paths that leave it in different states meet, it may be in any
of them, so a call that needs one is E0150 (t03). It accepts a parameter of
unknown state (t06). No spelling exists to name a state on a parameter or
result.

Three later rulings are not built:

- D-STATE-ARROW1=A: `#State(Draft -> Confirmed)` replaces `#Transition`. It
  is E0930 today (s04, s05).
- D-STATE-NS1=A: states at `Order.State.Draft` and in reflection. It is
  E0302 "`Order` has no static member `State`" (t05). L0153 already prints
  `Job.State.Lost`.
- `Docs/spec/reference/syntax-surface.jet:613-617` still shows `#Transition`.

**Protocols.** D-PROTO1/2 protocol blocks generate two endpoint handle types
over linear typestate (`syntax-decisions.md:833-846`). They were not probed.

**Services and live state.** A service worker keeps its state in a `&`
first parameter and handles one message per call (D-SERVICE-KEEP1=A,
2026-10-30). Core ships `Loadable<T, E>` with `Idle`, `Loading`, `Loaded`
and `Failed` (`Core/Reactive/Loadable.jet:4-63`, D-PENDING1).

**Diagrams.** Canvas receives `state_graphs` with checked typestate states,
edges, terminal flags and reachability
(`Docs/spec/reference/canvas-protocol.md:209-212`). No surface draws an
enum-and-match machine.

**Scale.** The extensibility census counted 598 function-local state
machines in Jet's own code (`Docs/research/2026-10-29-extensibility-every-layer.md:812`).

## Dispatch cost

Jet IR has structured control only: `Block`, `Loop`, `If`, `Else`, `End`,
`Br` and `BrIf` (`Compiler/JetIR/Source/IR.jet:77-128`). It has no multiway
branch. A match lowers to one `If` per arm, tested in order
(`Compiler/JetIR/Source/BuildExpr.jet:3552-3553, 4008-4047`).

A 16-case enum dispatched 200 million times at o2 shows the cost. When the
hot state is the first arm, a run takes 768-901 ms. When it is the last arm,
a run takes 2,009-2,378 ms (`probes/jt`, three runs each). This is a quick
timing, not a gated performance cell. No disassembler is installed, so the
machine code was not read.

Planning cards #3152 (enum dispatch on a real state machine) and #3154
(exhaustive functions versus enum-keyed tables) ask for this measurement
against other languages.

## Other languages

| Language | Machine shape | Transitions checked | Timeouts | Diagram |
| --- | --- | --- | --- | --- |
| Rust | enum plus `match`; typestate through generic marker types | exhaustive arms; typestate by types | library | none |
| Swift | enum with payloads plus `switch` | exhaustive arms | library | none |
| Zig 0.14 | labeled `switch` with `continue :label next` | exhaustive arms | none | none |
| Odin | enum plus `switch`, `#partial` to opt out | exhaustive arms | none | none |
| Kotlin | sealed classes plus `when` | exhaustive `when` | coroutines library | none |
| XState / SCXML | declared states, events, transitions | checked against the declaration at run time | `after` delays | generated from the declaration |
| Erlang gen_statem | one callback per state, `{next_state, …}` results | run time | state and event timeouts | none |

Zig's labeled `continue` was added for state machines. Each `continue
:label value` can compile to its own indirect jump, which helps branch
prediction. XState and SCXML add hierarchy (nested and parallel states),
entry and exit actions, and diagrams from the same declaration. gen_statem
adds postponed events and three kinds of timeout.

No language in the table checks at compile time which state may follow
which. Rust gets that only from typestate, one type per state.

## Ratified versus built

| Item | Ratified | Built |
| --- | --- | --- |
| Exhaustive enum dispatch (E0307) | yes | yes |
| L0302 same-enum guard table | yes | no emitter |
| L0529 unchanged state-loop header | yes | no emitter found |
| Readiness wait with `after` (D-CONC-CHAN2=D) | yes | yes |
| Typestate E0150 / E0151 / L0153 | yes | yes, straight-line locals |
| Typestate at joins and parameters | D-STATE1 implies it | joins yes (t03); parameters no (t06) |
| `#State(A -> B)` (D-STATE-ARROW1=A) | yes | no (E0930) |
| `Order.State.Draft` (D-STATE-NS1=A) | yes | no (E0302) |
| Multiway branch for enum dispatch | not decided | no; arms tested in order |
