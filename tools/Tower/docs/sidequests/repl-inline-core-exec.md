# Inline CoreLib execution in the REPL interpreter

**Status:** READY — no owner decision (implementation task). REPL v2 (c55) shipped;
this is the one deferred fork it left. **Partially landed:** the whitelist
*execution* machinery already exists (built under c98/D-CTCORE1, commit 53f5ac9) —
`apply_core_call` runs `core.math` + `core.string` pure functions inline at comptime
(`Source/Comptime/Methods.rs:321-472`), with effectful modules routed to a teaching
diagnostic (`Methods.rs:473+`). The narrow remaining gap is **plumbing**: the
REPL→comptime evaluation path never feeds the alias map, so the dispatcher is
unreachable from the REPL.

**Verification (2026-06-25):** `printf 'use core.math as math\nmath.sqrt(16.0)\n' | jet repl`
still errors **E0956** ("the name `math` can't run at compile time yet"). Root cause:
the comptime evaluator entrypoints pass `empty_imports()` for `core_imports`
(`Source/Comptime/mod.rs:103,132,157,185,221`), so `apply_core_call`'s alias lookup
(`Methods.rs:323-330`, `if let Some(module) = self.core_imports.get(alias)`) never
matches — `math` falls through to the unknown-name E0956 path. Sema *resolves* the
alias across inputs via the bundle path (`Source/REPL.rs:887-893`,
`tests/repl.rs:336-361`), but that resolution is separate from the comptime
interpreter's `core_imports` field (`Source/Comptime/Interpreter.rs:101`).

## Context (what already ships)

`jet repl` (E2-M18, `Source/REPL.rs`) is complete: accumulating module, `...`
continuation, typed echo, meta-commands, fuel cap (E1801), move semantics across
inputs (D-REPL8), `:run` (D-REPL-FUEL), `--project` (D-REPL10), and cross-input
`use …` imports — all green in `tests/repl.rs`. The D-REPL* decisions are
recorded in `syntax-decisions.md`.

## The gap

Sema *resolves* a core call like `math.sqrt(x)` across REPL inputs and the comptime
tree-walker now *has* a pure-core executor (`apply_core_call`), but the REPL's comptime
evaluation path doesn't hand the alias map to the interpreter, so evaluating one inline
still errors **E0956** (`math` can't run at compile time yet). It works via **`:run`**
(native codegen has CoreLib — verified `print(math.sqrt(16.0))` → `4.0`), just not inline.

```
jet> math.sqrt(16.0)
error[E0956]: `math` can't run at compile time yet  (use :run to run natively)
jet> :run
compiling session… running…
4.0
```

## Approach (sketch)

The interpreter-side executor (`apply_core_call`, D-CTCORE1) already exists and is the
right one — inline-executable == comptime-legal, one whitelist, no drift. The work is now
narrower than originally scoped:

- **Plumb `core_imports` into the REPL→comptime path.** The REPL evaluator calls the
  comptime entrypoints in `Source/Comptime/mod.rs` with `empty_imports()`
  (lines 103,132,157,185,221); feed the live alias→module map (the same one the bundle
  checker builds, `Source/Sema/Bundle.rs:493-586`) so `apply_core_call`'s lookup
  (`Methods.rs:323-330`) resolves `math`, `string`, … This is the change that flips the
  verified E0956 to an inline result.
- **Grow the whitelist as needed.** `apply_core_call` currently covers `core.math` +
  `core.string` (`Methods.rs:400-472`); extend to other pure modules per tests. Effectful
  modules (`core.fs/env/io/net`) already route to a teaching diagnostic and keep `:run`.
- Keep `:run` as the escape for anything outside the whitelist or effectful.

## Test plan

- `tests/repl.rs` — add a transcript asserting **the value**: `math.sqrt(16.0)`
  evaluates inline to `4.0` and produces no E0956. (The existing tests at lines 336-361
  only assert the alias *resolves* — no E0107 — not that it *executes*, which is exactly
  the gap; this is the assertion that would currently fail.) Plus: a
  non-whitelisted/effectful core call still errors inline and works via `:run`.
- Reuse the D-CTCORE1 whitelist tests (`apply_core_call` in `Source/Comptime/Methods.rs`)
  so the two stay in lockstep.

## Invariants

- **I2/I4:** inline results must match what `:run`/native produces; the whitelist
  must not admit effectful calls (determinism).
- **Scope:** whitelisted pure calls only — do not pull effectful CoreLib (fs,
  net, tasks) into the interpreter.
