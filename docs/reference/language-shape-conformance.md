# Language-shape cross-surface conformance closeout (#560)

Integration ledger only. Child cards own each ratified law; this file records
where evidence already lives so the umbrella can close without absorbing child
scope.

## Classification authority

| Concern | Authority |
|---------|-----------|
| User-typeable keywords / sigils | `crates/jet-foundation/src/Syntax.rs` + decision IDs |
| Decision log | `docs/spec/syntax-decisions.md` |
| Ecosystem shape proposals | `docs/proposals/ecosystem-shape.md`, archived research under `docs/archive/` |

## Surface matrix (prediction → proof)

| Surface | Proof path |
|---------|------------|
| Parser / AST | grammar regen via `jet devtools grammars`; fuzz corpus under `tests/fuzz/` |
| Sema | UI snapshots `tests/ui/*.stderr`; checker modules under `crates/jet-sema/` |
| Formatter | fmt stability suites; STABILITY fixtures |
| Diagnostics | registered codes + UI snapshots (I4) |
| Examples / goldens | `examples/features/**` + `*.expected_out` / `expected/` (I5) |
| Package / Config | package tests; one-file vs split-file package graph tests |
| Inspect / LSP / Canvas | existing focused suites; Canvas remains E8 for unfinished UI |

## 2026-07-15 rulings

Recorded in `syntax-decisions.md`. Superseded spellings must not remain as a
second authority (I8). Migration behavior for retired role-files follows the
ratified Package law on the owning child cards.

## Honest remainder

Package-graph depth that depends on Epoch 4 metaprogramming umbrellas stays on
those E4 cards. This closeout does not invent a parallel Package mechanism and
does not claim E4 scope done.
