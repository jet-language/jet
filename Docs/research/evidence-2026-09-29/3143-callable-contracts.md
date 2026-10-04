# #3143 — Callable identity, labels and capture contracts

Closer09, 2026-09-29. Binary via `~/.cache/jet-dev/safe-jet.sh` (jet-current
snapshot14 → snapshot16 during the session; all runs below after 02:30 are on
snapshot16). Scratch `~/.cache/jet-dev/scratch/Closer09/free/` (package
without an authority budget).

## Question

Do named functions, non-capturing lambdas, capturing closures, generic call
sites and nominal wrappers give identical results where their contracts agree,
reject incompatible label/effect/lifetime/access use, and keep Read/Write/Take
access through every call form, on every tier?

## Evidence

### Access-bearing function types (criteria 4, 6, 8)

`call/probe_write_fn_type.jet` and `probe_write_fn_type2.jet`
(`fn call_writer(f: fn(&[Int]), …)`, `f: fn(&buf: [Int])`, lambda
`(&b: [Int]) -> …`), `safe-jet.sh run` → exit 1:

```
Error [E0003]: Expected a type name, found `&`   (3:22, fn(&[Int]) and fn(&buf: [Int]))
Error [E0003]: Expected a type name, found `&`   (8:21, lambda (b: &[Int]))
Error [E0003]: Expected `)` to close this `(`, found `:`   (8:20, lambda (&b: [Int]))
```

The parser has no access slot in function types or lambda parameters, matching
the card's source reading (`JetParser/Source/Parser/Types.jet:339-368`,
`param_contract: None`). The `callable_access_contracts` golden cannot be
written in current syntax.

### Read-only contract `fn(Int) -> Int` through each category (criterion 1/5)

Minimal repros, `run3.sh <file> run,interp,aot`:

| category | file | default run | --interpret | AOT |
|---|---|---|---|---|
| named fn value to `fn(Int) -> Int` param | `categories2.jet` rows 1 | `named 42` | E0956 `MIR result test requires a result outcome carrier` (at `apply`) | rustc E0308 |
| non-capturing lambda | `categories2.jet` | `lambda 42` | same E0956 | same |
| capturing lambda (reads `base`) | `categories2.jet` | `capturing 42` | same | same |
| generic `apply_generic<T>(f: fn(T) -> T, x: T)` with a named fn | `ice_generic_fn_value.jet` | **ICE** `missing checked function target apply_generic` | — | — |
| generic with a lambda | `categories2.jet` | `generic lambda 42` | E0956 | rustc E0308 |
| nominal wrapper `struct Scorer { score: fn(Int) -> Int }` with a named fn | `wrapper_named.jet` | **wrong result `1`** (earlier run of `categories2.jet`: `9`) | `42` | rustc E0308 `expected fn … JetInt, found … Result<JetInt, JetErr>` |
| wrapper with a capturing lambda | `categories2.jet` | `wrapper capturing 42` | E0956 | rustc E0308 |
| closure returned from `make_adder(k)` | `closure_returned.jet` | **ICE** `MIR entry Err has an invalid checked carrier` | E0956 (at `apply`) | rustc E0308 |

Only the default run gives identical results for named / lambda / capturing /
generic-lambda, and even there the named-fn-in-wrapper case prints a wrong,
unstable value (1 and 9 on two runs; expected 42).

Rows marked `categories2.jet` come from one program: its `--interpret` run
stops at the first `f(n)` in `apply` (4:46) and its AOT build stops at the
first rustc error (the `Scorer` field), so those two columns are one
program-level failure, not per-row observations.

### Capture mutation, copy and move (criterion 2)

`closure_mutation.jet`, all three tiers agree (`same: …`):

```
captured count 0      <- bump(2); bump(3) on `count := 0` via `count += n` in the closure
hi ada
name still usable: ada
[2, 3, 4]
```

Expected `captured count 5`: the closure's write to the captured `count` is
silently lost with no diagnostic, while `tests/ui/lambda_mut_borrow_conflict`
(E0204, re-checked below) shows sema treats the same capture as a write
borrow.

### Rejections (criterion 1/5), `safe-jet.sh check` on copies of existing fixtures

| fixture | observed |
|---|---|
| `lambda_interface_effect_mismatch.jet` | E0112 `apply wants fn(Int) Int -[]> … this is fn(Int) Int -[IO]>` |
| `lambda_interface_error_mismatch.jet` | E2417 `ExpectedLambdaError is not an Error type` + E0113 lambda should fail with `ExpectedLambdaError` |
| `arg_label_missing.jet` | E0766 call to `connect` is missing `port` (direct call; function types carry no labels) |
| `lambda_mut_borrow_conflict.jet` | E0204 `total` is being changed in this call |
| `task_detach_view_capture.jet` | E2305, E1102, E1106 (escaped view in a detached task) |
| `lambda_escape_no_take.jet` | **`ok: no problems`** — the checked-in snapshot expects E0121 (`item` consumed by the closure, then reused) |

### Wrapper / callable family (criteria 3, 7)

Role identity uses an existing struct wrapper (`Scorer`); no new callable
family or auto-erasing conversion was introduced.

## Verdict

FAIL. Criteria 3/7 are met (nothing new introduced). Criteria 1/5 fail: the
categories do not give identical results across tiers (ICE, E0956, rustc
failures, a wrong value in the wrapper case). Criterion 2 fails: capture
mutation is silently lost and the move-into-closure rejection regressed.
Criteria 4/6/8 are blocked on the parser change the card plans (no
access-bearing function types or annotated lambda parameters exist).

## Defects (minimal repros under `free/`)

1. `wrapper_named.jet`: named fn stored in a struct field and called through
   it → default run prints `1` (another run printed `9`), `--interpret` `42`,
   AOT rustc E0308. Expected `42` on all tiers.
2. `closure_mutation.jet`: `count := 0; bump :: (n: Int) -> { count += n };
   bump(2); bump(3)` → `captured count 0` on all tiers. Expected 5 (or a
   diagnostic).
3. `closure_returned.jet`: returning a capturing closure and calling it
   through `apply` → default run ICE `MIR entry Err has an invalid checked
   carrier`, `--interpret` E0956, AOT rustc E0308. Expected `42`.
4. `ice_generic_fn_value.jet`: `apply_generic<T>(f: fn(T) -> T, x: T)` called
   with a named fn → ICE `missing checked function target apply_generic`.
5. `categories2.jet`: any `fn(Int) -> Int` parameter call under `--interpret`
   → E0956 `MIR result test requires a result outcome carrier`.
6. `tests/ui/lambda_escape_no_take.jet`: current binary reports no problems;
   snapshot expects E0121.
