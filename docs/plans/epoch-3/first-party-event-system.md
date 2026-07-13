# First-Party Event System

Status: D-EVENT1 ratified 2026-07-07; first compiler-known Core slice shipped.

This is not a Canvas feature. Canvas should project the event system once it exists. The event system is a first-party Jet language/runtime/library feature because it affects game loops, web UI, servers, plugins, observability, hot reload, testing, and structured concurrency.

## Goal

Jet should have the best event system in the language stack:
- Beginner path: declare or use a typed event, subscribe with a small block, never manage leaks manually.
- Expert path: choose sync/async dispatch, ordering, cancellation, backpressure, failure aggregation, lifetime owner, tracing, and hooks explicitly.
- One semantic model: source, Canvas, game dev, web dev, server hooks, and runtime internals all use the same event facts.

## Prior Art

C# events/delegates:
- Strength: type-safe delegates; publisher/subscriber split; many subscribers; events are a familiar first-party language concept.
- Weakness: synchronous multicast by default; lifetime leaks are common when subscribers outlive publishers; async errors are not part of the event type.
- Jet lesson: typed handlers and publisher/subscriber vocabulary are good. Subscription lifetime must be a first-class value, not a convention.

DOM EventTarget:
- Strength: `addEventListener` supports multiple handlers, event phases, `once`, passive listeners, and AbortSignal-based cleanup.
- Weakness: string event names, weak typing, return values ignored, propagation model tied to tree-shaped UI.
- Jet lesson: listener options and cancellation tokens are excellent, but Jet should make event names/types checked and keep propagation as a policy, not the only model.

Node EventEmitter:
- Strength: tiny API, ubiquitous server fit, `on`/`once`/`off`, deterministic insertion order, synchronous dispatch.
- Weakness: string/symbol names, duplicate listener hazards, special `'error'` behavior, async rejection edge cases, leak warnings as runtime repair.
- Jet lesson: tiny API matters, but error and async policy must be typed and visible.

Godot signals:
- Strength: first-class Signal values, editor integration, custom signals, object decoupling, good fit for games.
- Weakness: dynamic edges remain; signal argument mismatches can still be a runtime/user responsibility.
- Jet lesson: first-class event values plus editor projection are right. Jet should make payload typing and emit arity checked.

UnityEvent:
- Strength: callbacks persist in editor-authored scene data; inspector filters callbacks by compatible signature; good designer workflow.
- Weakness: persistent inspector bindings are engine-object state, not ordinary source; lifecycle is tied to Unity object model.
- Jet lesson: editor-authorable callbacks matter. Canvas can author subscriptions, but the persisted truth must still be Jet source.

Unreal Event Dispatchers:
- Strength: Blueprint-native event dispatchers support binding/unbinding/assign/call, inputs, and cross-Blueprint communication.
- Weakness: tied to Blueprint assets and engine class model; not a general source-first language mechanism.
- Jet lesson: Canvas should feel this good, but the source-visible Event/Hook model must own semantics.

Swift/modern concurrency:
- Strength: structured concurrency makes task lifetimes explicit and cancellation contagious.
- Weakness: async streams/events are often library-shaped and can fragment.
- Jet lesson: event handlers that suspend must live inside structured concurrency, not an untracked callback island.

## Shipped Hybrid Slice

Card #286 implemented the ratified hybrid center as ordinary Core values:

- `Event<T>`: typed many-subscriber occurrence stream.
- `Hook<T, R>`: typed ordered intervention point.
- `Subscription`: explicit unsubscribe/active handle.
- `EventScope`: owner/lifetime container for many subscriptions.
- `EventPolicy`: sync or explicit queued/backpressure policy.
- `EventTrace`: delivered/queued/dropped debug facts.

Public entrypoints:

```jet
use core.event as event

fn run() {
    scope :: event.scope()
    clicked :: event.new<Int>()
    sub :: clicked.on(scope, (n) => { print("clicked {n}") })
    clicked.once(scope, (n) => { print("once {n}") })
    print(clicked.emit(1).summary())
    sub.unsubscribe()

    before_save :: event.hook<Int, String>("allow")
    before_save.on_priority(scope, 10, (n) => "seen {n}")
    print(before_save.run(5, "fallback"))
    scope.cancel()
}
```

Default dispatch is synchronous, deterministic priority-descending then source
order. `once` auto-unsubscribes. `EventScope.cancel()` drops all owned
subscriptions. `with_policy<T>(policy_async(n))` exposes the explicit queued
entrypoint. Canvas/debugger projection should consume these compiler-known
types rather than inventing a separate graph model.

The synchronous law is snapshot-based and depth-first. A listener removed
before its turn is skipped; one added during delivery enters only a later or
nested snapshot. `once` deactivates before invocation. Owner cancellation is
terminal and idempotent: tracked listeners are removed, retained inactive
handles are released, and later registrations through that owner are inactive.
D-EVENT2=A scopes typed handler failure aggregation to `AsyncEvent<T, E>`;
`Event<T>` stays the infallible beginner path.

## Best Hybrid

One core semantic family:
- `Event<T>`: a typed many-subscriber event stream for occurrences.
- `Hook<T, R>`: a typed interception point where handlers may transform, cancel, veto, or contribute results according to a declared policy.
- `Subscription`: a linear/disposable handle tied to an owner lifetime by default.
- `EventScope`: an owner/lifetime container for many subscriptions.
- `EventPolicy`: sync/async dispatch, ordering, duplicate handling, backpressure, failure behavior, tracing, and reentrancy.

Beginner surface:
```jet
let clicked: Event<Click>

clicked.on(owner: button) { click ->
    button.highlight()
}

clicked.emit(Click.{x: 12, y: 20})
```

Expert surface:
```jet
let damage = Event<Damage>.{
    dispatch: .Async(buffer: 256, overflow: .DropOldest),
    order: .PriorityThenSource,
    failures: .Collect<DamageError>,
    reentrant: .Queue,
    trace: .On,
}

let sub = damage.on(
    owner: enemy,
    priority: 100,
    once: false,
) { event -> Result<Unit, DamageError> ? async {
    enemy.apply(event.amount)?
}
```

Hook surface:
```jet
let before_save = Hook<SaveRequest, SaveDecision>.{
    combine: .FirstCancelElseContinue,
}

before_save.on(owner: plugin) { req ->
    if req.path.ends_with(".secret") {
        .Cancel("secret files require explicit export")
    } else {
        .Continue
    }
}
```

Canvas projection:
- Event values render as dispatcher/source nodes.
- `on` subscriptions render as listener nodes with owner/lifetime pin.
- `emit` renders as dispatch node.
- Hook policies render as configuration pins.
- Debugger shows event trace, listener order, payload value, cancellation path, handler failure, queue/backpressure state.

## Required Semantic Rules

Typing:
- Event payload is one type, preferably a named struct for public events.
- Handler parameter type must match payload.
- Emit arity/type checked at compile time.
- Event names are values/symbols with types, not free strings.

Lifetime:
- Every subscription has an owner or explicit `Subscription` handle.
- Owner destruction/cancel drops owned subscriptions automatically.
- Dropping a handle unsubscribes unless explicitly detached.
- Detached/global subscriptions require visible expert spelling and diagnostics.

Dispatch:
- Default dispatch is synchronous in registration order for predictability.
- Async dispatch is explicit and requires queue/backpressure policy.
- Reentrant dispatch policy is explicit: reject, queue, or allow.
- Duplicate subscription policy is explicit: reject, allow, or replace.

Failures:
- Infallible handlers are the beginner default.
- Fallible events declare failure policy: stop first, collect, log, ignore, or route to a typed error event.
- Async handler failures cannot become unhandled background errors.

Hooks:
- Events report that something happened.
- Hooks are ordered intervention points before/during/after an operation.
- Hook combination policy is typed: first cancel, collect all, fold transform, last wins, require all approve.

Security/safety:
- Event handlers inherit capability/effect constraints from their declaration.
- Cross-thread events require Send/Sync-style proof once Jet has that surface.
- Unsafe event handlers require the normal unsafe gate.

Diagnostics:
- Leaked detached subscription.
- Owner mismatch.
- Handler may suspend inside sync event.
- Event recursion rejected by policy.
- Unhandled handler failure.
- Payload type drift after refactor.
- Canvas graph subscription without source owner.

Tests:
- Type-check subscribe/emit.
- Owner drop unsubscribes.
- Manual handle unsubscribe.
- Once listener.
- Priority/order determinism.
- Reentrancy policy.
- Async queue/backpressure.
- Failure aggregation.
- Hook cancellation/transform.
- Canvas graph JSON projection.
- Debug event trace.

## Ballot Shape

The main owner decision should choose the semantic center:
- A: Library-only `Event<T>` / `Hook<T, R>` values.
- B: Language-level `event` declarations.
- C: Actor/mailbox/channel-first event model.
- D: Hybrid first-party event family: library values plus reserved source sugar only if later proven.

Recommendation: D. Build the semantic model as ordinary typed values first, make it compiler/tooling-known for diagnostics/projection, and reserve syntax only after examples prove a library spelling is too noisy. This gives beginners a tiny path and experts the whole control plane without duplicating mechanisms.
