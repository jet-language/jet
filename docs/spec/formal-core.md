# Formal Core / Desugaring Map

**Status: Deferred to Epoch 6** (D-FORMALCORE1=C, ratified 2026-06-28)

This document will describe every surface form that Jet lowers and the TIR
node it lowers to. It is reserved for when the sema feature set is frozen for
v1 API stability. Maintaining it before that point adds overhead without
guaranteeing accuracy.

## Why it will matter (Epoch 6)

- Enforces I8: a visible map makes it immediately obvious when a new surface
  form duplicates an existing TIR path.
- Anchors crate-seam contracts: each crate author knows what it receives and
  what it emits.
- Onboarding reference: one page instead of four-file grepping.

## Desugarings already stable (non-authoritative until Epoch 6)

| Surface form | TIR node / lowering |
|---|---|
| `x :: expr` / `x := expr` binding | `TStmt::Bind` |
| `if cond { … } else { … }` | `TExprKind::If` |
| `if subject == { arm -> … }` dispatch | `TExprKind::Match` |
| `expr?` / `expr?? fallback` | `TExprKind::Propagate` / `TExprKind::WithDefault` |
| `loop x; iter { … }` | `TExprKind::ForIn` → iterator protocol |
| `f.[a, b, c]` (S75) | `ListLit` of `Call`s (`FixedList`/`List`) — #779 |
| `[f.[a, b], c]` flatten | sema rewrite → flat `Call`s in `ListLit` — #779 |
| `loop { … }` | `TExprKind::Loop` |
| `#MustUse` / `#SingleUse` markers | `TMarker::MustUse` / `TMarker::SingleUse` |
| `consume(x)` (D-DROP-WORD1=A) | `TExprKind::Drop` |
| struct/enum auto-derive | comptime derive pass → normal TIR |

Do not treat this table as authoritative — it is a sketch to reduce cold-start
work in Epoch 6, not a compiler contract.

## Promotion criteria (Epoch 6)

Before this document becomes authoritative:
1. Feature set for v1 is frozen.
2. CI enforces consistency between the table and `jet-codegen` lowering paths.
3. All desugarings are snapshot-tested (I4/I5).
