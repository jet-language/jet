# `$` splice + `comptime { }` block (Tower #94)

**Spec basis:** D-CTMARKER1=C, D-CTCODEGEN1=A, D-CTEFFECT1, D-WHEN1/2.

**Status:** Parser + sema + interpreter wiring exists for comptime-context use. Open work: runtime splice bridge. One owner decision (D-CTBLOCKEXPOSE1) gates build.

## Current state

**Wired (comptime-context only):**
- Parser: `$name` → `Expr::ComptimeSplice`; `comptime { }` → `Stmt::ComptimeBlock`
- Interpreter: `$name` resolves like bare ident in comptime scope (`Interpreter.rs:850`)
- Sema `comptime {}`: runs body at build time, surfaces errors (`CheckerCore.rs:1955`)
- Sema `$name`: guards with E2712 outside comptime context (`expr.rs:677`) — no type, no value
- Codegen: block erases to nothing; `$name` is walk no-op (no `lower_expr` arm)
- Works: `examples/features/125_comptime_block.jet`, `129_comptime_splice.jet`

**Missing (runtime-splice bridge):**
- `comptime {}` discards its result scope — `run_block_with_imports` returns `Result<(), _>`
- Sema `$name` never resolves type in runtime positions
- Codegen has no `lower_expr` arm for `ComptimeSplice`
- No E-code distinguishes "undefined comptime name" from "no comptime scope"

**Existing serialization path:** `CtValue::serialize()` covers all value kinds → Rust literal. `CtValue::jet_type()` gives the Jet type. The comptime-binding lowering at `lower.rs:789` is the exact path a runtime splice reuses.

## Owner decision needed — D-CTBLOCKEXPOSE1

How a `comptime { }` block exposes values to runtime code via `$name`:

- **A (recommended):** Block-local `comptime NAME` bindings leak into enclosing comptime scope; `$NAME` splices any in-scope comptime value into runtime code (lowered as serialized literal). Consistent with `comptime if` leak rule (D-WHEN1) and existing `comptime NAME` serialize path.
- **B:** Explicit `expose NAME` inside block marks which values escape. More explicit but adds second visibility mechanism (cuts against I8).
- **C:** Block exposes nothing; `$name` stays comptime-only. Conservative — nearly already shipped, but runtime-splice intent not delivered.

## Build order (after D-CTBLOCKEXPOSE1 ratified)

1. Comptime engine (`Comptime/mod.rs:373`): return block's computed bindings
2. Sema `check_comptime_block` (`CheckerCore.rs:1955`): insert returned bindings into `ct_scopes`
3. Sema `ComptimeSplice` (`expr.rs:677`): when not `in_comptime`, look up in `current_ct_globals()` → `value.jet_type()`; miss → E2713
4. Codegen `lower_expr`: add `ComptimeSplice` arm emitting `TExprKind::ConstInline(value.serialize())`; admit in subset gate
5. Diagnostics: E2713 (undefined comptime name), E2714 (reserved: no runtime form)
6. Tests/examples: extend `125_comptime_block.jet`; negative cases for E2712/E2713

## Acceptance

1. `comptime {}` block exposes value → `$NAME` in runtime code → correct literal output
2. `$NAME` type-checks via `jet_type()`
3. Block erases at codegen (no runtime Rust)
4. E2713 for undefined `$name` in runtime
5. Existing comptime-context examples unchanged

## Critical files
- `crates/jet-sema/src/Sema/CheckerCore.rs`
- `crates/jet-sema/src/Sema/CheckerInfer/expr.rs`
- `crates/jet-comptime/src/Comptime/mod.rs`
- `crates/jet-codegen/src/Codegen/TIR/lower.rs`
