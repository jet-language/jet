# TIR semantic core

This page is the compiler contract at the boundary between semantic analysis and
executable lenses. It covers totality, the read-only fact channel, lowering,
and coverage obligations. The current host implementation is
`crates/jet-codegen/src/Codegen/TIR/mod.rs` and its exhaustive matches; the
staged self-host port under `Compiler/JetFoundation/Source/TIR/TIR.jet` must
preserve the same contract.

TIR is a semantic contract, not a commitment to Rust as a permanent
implementation and not a claim that the self-host port is already the active
compiler. A port may change representation, but it may not move semantic
choices into an executable lens or create a second meaning.

## One semantic core (R12 / D-ONECORE1=A)

There is one structured semantic core. Every executable lens—Rust AOT emit,
Cranelift, interpreter, and web—consumes that core exhaustively. Surface sugar
expands during lowering or an earlier sema rewrite into core nodes that those
lenses already handle. A coverage miss is an internal compiler error, never a
fallback.

TIR is post-sema and total: every fact codegen needs is carried concretely on
the node, never re-inferred through a fallback. The emitter pattern-matches
those fields and formats output; it does not re-run semantic decisions such as
expression typing or overflow classification. `tir_covers` is the coverage
proof for reachable functions.

## Read-only fact channel

The frozen TIR exposes one typed, borrowed `TFactChannel` view. It projects facts
already selected by sema from existing TIR carriers; it is not a side table, a
third IR, or a third executable lens, and it adds no per-node heap allocation.
The initial fact/carrier contract is:

| Fact | TIR carrier | Missing fact |
|---|---|---|
| Type | `TExpr.ty`, typed locals and parameters, `TFunc.ret` | Keep the conservative typed operation. |
| Integer bounds | `Type::integer_range`, exact integer literals, `TNumericOp::InlineRange`, fixed-list proof | Keep the checked range or index operation. |
| Exclusivity | sema `AccessConvention` lowered to `TCallArg` borrow flags and `Borrow` nodes | Keep shared or unknown memory dependencies and wrappers. |
| Purity | sema `Func.is_pure` and `AutoVectorizationFacts.effect_free_body` | Do not apply a reorder or vector hint. |
| Comptime value | sema comptime binding facts lowered to `TExprKind::CtLit` | Keep runtime evaluation or serialization. |

Generic call evidence belongs to the caller's lexical scope, not the callee's
registration row. Both local and cross-module calls must carry that scope's
type parameters and canonical trait bounds into inference, including explicit
type arguments. Lowering consumes the checked substitutions and bound evidence;
it must not reconstruct a forwarded parameter's bounds from its spelling.

Both executable lenses consume the channel read-only. A missing field means
“not proven”; it never authorizes codegen to re-derive sema policy. An optimized
lens may derive a private SSA form internally, but that form is not a third
semantic representation or source of truth. Every TIR construct remains
exhaustive for AOT, Cranelift, interpreter, and web.

## Lowering rules

Engines must not special-case these surface forms:

| Surface | Core form | Authority |
|---|---|---|
| `freeze(x)` | `Clone`, `MaterializeView`, or `ExplicitCopy`, selected by the sema-approved source type; frozen provenance stays in capture metadata. | D-CONC-FREEZE1=A |
| `task ^name { … }` | The existing task lambda with an explicit consuming capture; the task-crossing prover owns legality. | D-CONC-FREEZE1=A |

The freeze example is a lowering rule, not an engine-side capture check. A
self-host implementation must carry the same already-proved ownership facts to
each lens.

## Core keepers

Engines must handle literals (`IntLit`, `FloatLit`, `BoolLit`, `CharLit`, and
plain `StrLit`), `Local`, `Call`, `MethodCall`/`BuiltinMethod`/`CoreCall`,
`ListLit`, `TupleLit`, `StructLit`, `EnumLit`, `Field`, `Index`,
`Binary`/`Unary`, `If`/`IfExpr`, `Match`-shaped statements, `Let`/`Assign`,
`Loop`/`While`/`CountedLoop`/`Range`, `Return`/`Break`/`Continue`,
`Clone`/`Borrow`/`Drop`, `Print`, `Lambda`, `Inline` (empty comptime
elision), and the host, select, and task nodes defined by `mod.rs`. Prefer
deleting a wide node over adding a new one.

## Proof obligations

- Keep `jit_coverage_audit` as the coverage check; a hand-maintained gap list
  is not a substitute for the structural proof.
- `Examples/features/concurrency/freeze_capture.jet` is the cross-lens fixture
  for AOT, default `jet run`, and forced interpretation. Comptime returns an
  already-owned `CtValue` for `freeze`.
- The REPL task boundary remains E1802, and web has no separate freeze policy or
  engine-side capture check.

These checks protect the contract's boundaries. They do not turn the current
Rust host into a permanent architecture commitment, and they do not let a
future `Compiler/` implementation silently introduce a parallel semantic core.
