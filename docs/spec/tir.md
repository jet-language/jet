# TIR semantic core (for #668 freeze)

**Status:** living inventory for D-ONECORE1 / #779. Canonical definitions stay in
`crates/jet-codegen/src/Codegen/TIR/mod.rs` and its exhaustive engine matches.
This page records which surface forms desugar away before engines see them.

## Law (R12 / D-ONECORE1=A)

One structured semantic core. Every engine (AOT emit, Cranelift, interpreter)
consumes that core exhaustively. Surface sugar expands in lowering (or earlier
sema rewrite) into core nodes engines already handle.

## Desugared at lowering (engines must not special-case)

| Surface | Core form | Card |
|---|---|---|

## Still wide (ranked #779; next shrink slices)

| Construct | Why expensive | Intended core form |
|---|---|---|
| `ForIn` (+ method kinds) | many emit/JIT arms | `While` / `CountedLoop` + iterator protocol |
| `MapLit` | block builder in emit | host/builder call or block expr |
| `ListSpread` | block builder in emit | push/extend sequence |
| `TupleDestructure` / `StructDestructure` | per-engine unpack | `Let` + `Borrow` + `Field` + `Clone` |
| `ListDestructure` | `jet_unpack_vec` spelling | host unpack + `Let`s |
| `StrLit` with `Interp` | string builder block | concat of lit + show |

## Core keepers (engines must handle)

Literals (`IntLit`/`FloatLit`/`BoolLit`/`CharLit`/`StrLit` plain), `Local`,
`Call`, `MethodCall`/`BuiltinMethod`/`CoreCall`, `ListLit`, `TupleLit`,
`StructLit`, `EnumLit`, `Field`, `Index`, `Binary`/`Unary`, `If`/`IfExpr`,
`Match`-shaped stmts, `Let`/`Assign`, `Loop`/`While`/`CountedLoop`/`Range`,
`Return`/`Break`/`Continue`, `Clone`/`Borrow`/`Drop`, `Print`, `Lambda`,
`Inline` (empty comptime elision), and the host/select/task nodes already in
`mod.rs`. Prefer deleting a wide node over adding a new one.

## Proof

- No new `tests/jit_gaps.txt` entries on this wave.
