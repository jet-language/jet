# #3243 — Observable state and generated-property parity (core.reactive)

Date: 2026-09-29. Card #3243 (CORE-F016). Binary: `jet-debug-snapshot14`.

## Blocker

`core.reactive` does not parse on snapshot14 or on the current source tree, so no reactive program runs on any tier.

- `Core/reactive/reactive.jet:46` `pub fn effect(...)` fails with `E0003 Expected a name after 'fn', found the keyword 'effect'`. `effect` became a lexer keyword (`Compiler/JetLexer/Source/Lexer/Scan.jet:154`, `K_EFFECT`, commit `02d733d11`).
- Every user of the module fails the same way: `Examples/features/ui/reactive.jet` and `reactive_scope.jet` both fail on `jet run` and `jet run --interpret` with the E0003 above, reported against the user file.
- Renaming `reactive.effect` is an owner API question. Main recorded it as D-REACT-EFFECT-NAME (rename to `watch`).

## Core fixes made (named, parse-verified only)

1. `Core/reactive/reactive.jet` `effect_run`: the multi-line `if … -> Effect{…}` / `else -> ~e` tail did not parse (E0003 at :95/:96). It is rewritten as `if … -> return Effect{…}` followed by `~e`, the same early-return shape as `effect_run_if`.
2. `Core/reactive/reactive.jet` `set` and `computed_set`: removed the `if version >= 2147483646 -> return ~sig/~c` cap. It silently dropped every write after about 2^31 updates; `Int` is exact, so the cap guarded nothing and broke the no-lost-update invariant.

Proof: `safe-jet.sh check Core/reactive/reactive.jet` now reports only the two `:46` `effect` errors (before the fix it reported four). **Runtime behaviour of both fixes is unexercised** until the keyword clash is resolved.

## Criterion 1 — unchanged-value notification, derived updates, owner disposal

**Not provable today.** When the module parses again, `reactive_observable.jet` (below) is the witness to run on JIT, interpreter and AOT.

Source reading shows what the witness must settle:

- **Provider path.** The provider replaces `signal`/`derived`/`computed`/`effect` with native cells: `Compiler/JetFoundation/Source/Registry/CoreCallRows.jet:19` for `core.reactive.signal` → `jet_std::JetSignal::new`, and `Compiler/JetCodegen/Source/Codegen/Expressions.jet:2436-2442` for derived and effect.
- **Native method routes.** Only `Signal.get`, `Signal.set` and `Derived/Computed.get` have them (`Compiler/JetCodegen/Source/Codegen/HandleMethods.jet:250-252`); the web mapping has the same three (`Emit/Web/JavaScriptOps.jet:53-55`). `unsubscribe`, `set_if_changed`, `update`, `version`, `effect_run_if` and `computed_update` have no provider route. They are source functions over the carrier struct.
- **Risk.** Disposal through `reactive.unsubscribe(e)` returns a new `Effect{active: false}` record and does not reach the native subscription. The witness must show whether a disposed effect still re-runs.

## Criterion 2 — generated-member diagnostics and AOT constraints (accounted)

| aspect | source evidence | cell |
|---|---|---|
| reactive "generated members" | Sema treats `reactive.effect/derived/computed` as compiler-known retained closures (`Compiler/JetSema/Source/Sema/Calls/ReactiveHandles.jet:117-159`); no user-visible generated accessors exist | no MVVM-style `[ObservableProperty]` generation; nothing to diagnose beyond the closure rules |
| diagnostics | E2910 (needs a lambda, `:96`, `:111`), E2911 (zero-parameter lambda, `:101`), E2912 (derived must return a value, `:156`), E2913 (a reactive value can't hold a function, `:39`) | present in source; **unexercised** (module does not parse) |
| AOT | the native routes above are the only provider mappings. The source-level ops compile as ordinary Jet over the carrier and have no provider equivalent | AOT/JIT/web meaning of `unsubscribe` and `set_if_changed` on provider cells is **unknown** until run |
| reflection-only paths | none: the reactive family uses no reflection | n/a |

## Criterion 3 — missing state API: ballot draft (Pip files it)

**D-REACT-SETCHANGED1 — equality-gated set for any signal**

Today `set_if_changed` and `changed` accept only `Signal<Int>` (`reactive.jet:61,66`); a `String` signal cannot use them.

- **A.** Generic `set_if_changed<T: Equatable>(sig: Signal<T>, value: T)` plus `changed<T: Equatable>`, with a provider route so the native cell skips notification on equal values.
  - Beginner: `&name.set_if_changed("Ada")`.
  - Expert: the same call, with no custom comparer.
- **B.** Make `set` itself skip equal values for `Equatable` `T` and drop `set_if_changed`.
  - Beginner: nothing new to learn.
  - Expert: loses forced notification on equal values.
- **C.** Keep it `Int`-only.

Recommendation: **A**. It keeps `set` a plain write, and it is one generic function rather than an ObservableObject hierarchy.

## Criterion 4 — example and golden

Planned witness, to be added once the module parses (not added now: no output can be observed or blessed):

```jet
use core.reactive as reactive

fn run() {
    n := reactive.signal(1)
    doubled :: reactive.derived(() -> n.get() * 2)
    plus_one :: reactive.derived(() -> doubled.get() + 1)
    watcher :: reactive.effect(() -> print("effect n={n.get()} doubled={doubled.get()} plus_one={plus_one.get()}"))
    &n.set(1)            // same value: does the effect re-run?
    &n.set(5)            // derived chain order
    loop i in 0..<3 -> &n.set(i + 10)
    print("version {n.version}")
    reactive.unsubscribe(watcher)
    &n.set(99)           // disposed: must not print
    print("after dispose {plus_one.get()}")
}
```

## Verdict

**BLOCKED**, on the `effect` keyword clash (D-REACT-EFFECT-NAME).

- Criterion 2 is met as an accounting.
- Criterion 3 is met by the ballot draft.
- Criteria 1 and 4 require running behaviour and stay open.
