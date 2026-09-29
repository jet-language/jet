# #3258 — Typed message and subscription lifecycle on core.event

Date: 2026-09-29. Card #3258 (CORE-F034). Binary: `jet-debug-snapshot14` via `~/.cache/jet-luna/safe-jet.sh`.

## Question

On `core.event`, how do duplicate registration, retained-recipient lifetime (scope-owned registration), unsubscribe and disposal, concurrent send, and request/reply with no responder or two responders behave? Is this one meaning across tiers?

## Method

The witness is `event_lifecycle.jet`, kept at `~/.cache/jet-test-scratch/Closer06/keep/ev/event_lifecycle.jet`, in six sections:

1. The same handler registered twice.
2. `unsubscribe` called twice, then `emit`.
3. A recipient owned by a `view` scope that is cancelled while the event lives.
4. `emit_async` from two `task`s on one `AsyncEvent`.
5. `decision_hook` request/reply with no responder and with two responders under `FirstCancelElseTransform`.
6. `app.cancel()` owner teardown.

`event_lifecycle_nohook.jet` is the same program without section 5, with a silent async handler so the delivery order doesn't leak into output. Commands:

```
~/.cache/jet-test-scratch/Closer06/run3.sh event_lifecycle.jet 60         # jet run + jet run --interpret
~/.cache/jet-test-scratch/Closer06/run3.sh event_lifecycle_nohook.jet 60
~/.cache/jet-luna/safe-jet.sh build event_lifecycle_nohook.jet
```

The example was **not** added to `Examples/features/ui/` with a golden. The tiers disagree and the request/reply section crashes, so no output is correct to bless (contract rule).

## Evidence

### JIT (`jet run`), sections 1–4 and 6 (nohook), rc=0

```
listeners 2 active 2
saved 1
saved 1
emit 1: delivered=2 queued=0 dropped=0
first active false second active true
saved 2
emit 2: delivered=1 queued=0 dropped=0
saved 3
view 3
emit 3: delivered=2 queued=0 dropped=0
view active 0 listeners 1
saved 4
emit 4: delivered=1 queued=0 dropped=0
left delivered=1
right delivered=1
after cancel active 0 listeners 0
emit 5: delivered=0 queued=0 dropped=0
```

This output is correct per the D-EVENT1/2 intent:

- Duplicate registration is not deduplicated: two handles, delivered=2.
- `unsubscribe` is idempotent.
- A cancelled scope disposes its recipient while the event keeps its other listeners (retained-recipient lifetime is scope-owned).
- Each concurrent `emit_async` reports its own delivery.
- Owner teardown disposes everything.

With the handler printing, the async order was `async 20` before `async 10` in one run. That confirms the order across tasks is scheduler-defined, so a golden must not print it.

### JIT, full program with section 5

Output matches the above through `right delivered=1`, then:

```
no responder: reply <invalid>
internal compiler error: JIT drop `HookOutcome` enum discriminant is invalid
```

### Interpreter (`jet run --interpret`)

- Full program: `E0956 'MIR enum argument type does not match its variant payload' isn't supported by the current evaluator yet` at `ask.on_priority(app, 10, (n: Int) -> HookDecision.Transform(n + 1))`.
- nohook program, rc=0: every counter line matches the JIT, but **no handler body output appears** (no `saved 1`, `view 3`, …), and `left delivered=0` / `right delivered=0` where the JIT says 1.

### AOT (`jet build event_lifecycle_nohook.jet`)

`internal compiler error: MIR nominal type "T" has no declaration row` at `crates/jet-codegen/src/Codegen/MIRRust.rs:2699:32`.

## Defects (minimal repros kept under `~/.cache/jet-test-scratch/Closer06/keep/ev/`)

| id | repro | tiers | observed | expected |
|---|---|---|---|---|
| EV-1 | `summary_min.jet`: `print(clicked.emit(1).summary())` | JIT, AOT | JIT prints `<invalid>`. AOT: rustc E0308 `expected (), found String` on `jet_std::JetEventTrace::summary`, meaning MIR types the result as Unit | `event delivered=1 queued=0 dropped=0` (the interpreter prints this) |
| EV-2 | `decision_min.jet`: `decision_hook<Int,String>(FirstCancelElseTransform)`, `on(scope, (n) -> HookDecision.Transform(n + 1))`, `run(7)` | JIT | `continue <invalid>` then ICE `JIT drop HookOutcome enum discriminant is invalid` | `continue 8` |
| EV-3 | same `decision_min.jet` | interpreter | E0956 `MIR enum argument type does not match its variant payload` | `continue 8` |
| EV-4 | `summary_min.jet`, `async_task_min.jet` | interpreter | handler bodies never observed (no `clicked 1`, `async 1`), while `delivered` counts 1. `emit_async` from inside a `task` reports `delivered=0` (JIT: 1) | handler output and counts equal to the JIT |
| EV-5 | `event_lifecycle_nohook.jet` | AOT | ICE `MIR nominal type "T" has no declaration row` (MIRRust.rs:2699) | a built binary |
| EV-6 | `event_lifecycle.jet` | JIT, intermittent | the first run hung after `emit 4` (section 4, two tasks calling `emit_async`) until the 300 s timeout. Three later runs of the same shape completed | no hang. Observed 1 in 4 runs; flake, not yet minimized |

## Example fix made (named)

`Examples/features/ui/events.jet:34,36,40` dropped the `&` mark on `clicked.emit(n)`. Snapshot14 rejects it with E0225 ("This call only reads its receiver, so it takes no `&` mark"). After the fix `events.jet` still fails: JIT prints `<invalid>` for `summary()` (EV-1) and then hits EV-2; the interpreter stops at EV-3.

## Cells (criterion 1 and plan item 2)

| job | Jet today | cell |
|---|---|---|
| weak registration | none. Recipient lifetime is owned by an `EventScope`; cancelling it disposes the registration (JIT evidence above) | covered by scope ownership; no weak form needed |
| strong registration | `on`/`once` return `Subscription`; `unsubscribe` is idempotent | proven on the JIT |
| duplicate registration | allowed, delivered twice | proven on the JIT |
| token channels | each `Event<T>` value is its own typed channel | proven on the JIT |
| concurrent send | per-dispatch `DispatchReport`; order across tasks not defined | proven on the JIT; EV-4/EV-6 open |
| request/reply, no responder / two responders | `decision_hook` + `HookPolicy` | **not provable**: EV-2, EV-3 |
| missing-response error | `HookOutcome` has `Continue/Cancel/Fail`. A no-responder run yields the policy's default, not an error | ballot only after EV-2/3 are fixed and the default is observed |

## Criterion 2 — no global bus

`Core/event/event.jet:20-46` forwards only scoped constructors (`scope`, `new`, `with_policy`, `hook`, `decision_hook`, `policy_sync`, `async_result`). There is no global or static bus and no service locator. Every event value and scope is explicit. Nothing was added. **Met.**

## Verdict

**PARTIAL.** Criterion 2 is met; criteria 1 and 3 are unmet because of the defects below.

- Criterion 1 is met on the JIT only for registration, unsubscribe, scope disposal and concurrent send. Request/reply and missing response fail on every tier (EV-2, EV-3).
- Criterion 2 is met.
- Criterion 3 (golden plus `run` and `--interpret` diff-empty) cannot pass until EV-1 through EV-4 are fixed.
