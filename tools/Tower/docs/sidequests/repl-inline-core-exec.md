# Inline CoreLib execution in the REPL interpreter

**Status:** parked — REPL v2 (c55) shipped; this is the one deferred fork it
left. No owner decision (implementation task). Large: touches the comptime
interpreter, not the REPL, and affects every core module.

## Context (what already ships)

`jet repl` (E2-M18, `Source/REPL.rs`) is complete: accumulating module, `...`
continuation, typed echo, meta-commands, fuel cap (E1801), move semantics across
inputs (D-REPL8), `:run` (D-REPL-FUEL), `--project` (D-REPL10), and cross-input
`use …` imports — all green in `tests/repl.rs`. The D-REPL* decisions are
recorded in `syntax-decisions.md`.

## The gap

Sema now *resolves* a core call like `math.sqrt(x)` across REPL inputs, but the
comptime tree-walker (`Source/Comptime/`) can't *execute* core-module calls
inline, so evaluating one in the REPL errors **E0956** (`math` can't run at
compile time yet). It works via **`:run`** (native codegen has CoreLib — verified
`print(math.sqrt(16.0))` → `4.0`), just not inline.

```
jet> math.sqrt(16.0)
error[E0956]: `math` can't run at compile time yet  (use :run to run natively)
jet> :run
compiling session… running…
4.0
```

## Approach (sketch)

Teach the comptime interpreter (`Source/Comptime/`) to execute the curated pure
CoreLib surface inline, reusing the D-CTCORE1 whitelist (the same set comptime
already allows). Non-whitelisted / effectful core calls keep erroring inline and
route to `:run`. This is the interpreter-side companion to D-CTCORE1's sema
gate — sema already says *which* calls are pure-comptime-legal; this makes the
tree-walker actually run them.

- `Source/Comptime/` — add CoreLib runtime handlers for the whitelisted pure
  functions (math/string/etc.), dispatching on the resolved core path.
- Reuse the D-CTCORE1 whitelist so inline-executable == comptime-legal (one list,
  no drift).
- Keep `:run` as the escape for anything outside the whitelist or effectful.

## Test plan

- `tests/repl.rs` — transcript: `math.sqrt(16.0)` evaluates inline to `4.0`; a
  non-whitelisted/effectful core call still errors inline and works via `:run`.
- Reuse the D-CTCORE1 whitelist tests so the two stay in lockstep.

## Invariants

- **I2/I4:** inline results must match what `:run`/native produces; the whitelist
  must not admit effectful calls (determinism).
- **Scope:** whitelisted pure calls only — do not pull effectful CoreLib (fs,
  net, tasks) into the interpreter.
