# TIR semantic core (for #668 freeze)

**Status:** living inventory for D-ONECORE1 / #779, amended by #2301. Canonical
definitions stay in `crates/jet-codegen/src/Codegen/TIR/mod.rs` and its
exhaustive engine matches. This page records which surface forms desugar away
before engines see them and the optimizer fact-channel contract.

## Law (R12 / D-ONECORE1=A)

One structured semantic core. Every engine (AOT emit, Cranelift, interpreter)
consumes that core exhaustively. Surface sugar expands in lowering (or earlier
sema rewrite) into core nodes engines already handle.

## Fact channel (#2301 / #668 amendment)

The frozen TIR exposes one typed, read-only `TFactChannel` view. It projects
facts already selected by sema from existing TIR carriers; it is not a side
table, a third IR, or a third executable lens. The view has no per-node heap
allocation. Its first classes and carriers are:

| Fact | TIR carrier | Missing fact |
|---|---|---|
| Type | `TExpr.ty`, typed locals/parameters, `TFunc.ret` | Keep the conservative typed operation. |
| Integer bounds | `Type::integer_range`, exact integer literals, `TNumericOp::InlineRange`, fixed-list proof | Keep the checked range/index operation. |
| Exclusivity | sema `AccessConvention` lowered to `TCallArg` borrow flags and `Borrow` nodes | Keep shared/unknown memory dependencies and wrappers. |
| Purity | sema `Func.is_pure` and `AutoVectorizationFacts.effect_free_body` | Do not apply a reorder or vector hint. |
| Comptime value | sema comptime binding facts lowered to `TExprKind::CtLit` | Keep runtime evaluation/serialization. |

The channel is consumed read-only by both executable lenses. A missing field
means “not proven”; it never authorizes codegen to re-derive sema policy. A
private SSA implementation may be derived inside an optimized lens, but it is
not a third semantic representation or source of truth. Every TIR construct
remains exhaustive for AOT, Cranelift, interpreter, and web.

## Desugared at lowering (engines must not special-case)

| Surface | Core form | Card |
|---|---|---|
| `freeze(x)` | `Clone`, `MaterializeView`, or `ExplicitCopy` selected by sema-approved source type; frozen provenance stays in the capture metadata | D-CONC-FREEZE1=A |
| `task ^name { … }` | existing task lambda with an explicit consuming capture; the task crossing prover owns legality | D-CONC-FREEZE1=A |

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

- No hand-maintained JIT gap entries. `jit_coverage_audit` must stay green.
- `examples/features/concurrency/freeze_capture.jet` is the executable proof
  for AOT, default `jet run`, and forced interpretation. Comptime returns an
  already-owned `CtValue` for `freeze`; the REPL task boundary remains E1802,
  and web has no separate freeze policy or engine-side capture check.
