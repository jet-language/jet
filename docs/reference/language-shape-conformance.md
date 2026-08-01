# Language-shape cross-surface conformance

E3 shipped the cross-surface match for syntax registry, parser, sema, formatter,
diagnostics, examples, and editor grammars. Package-graph depth that depends on
Epoch 4 metaprogramming lives on Tower card `#560` (now epoch `e4`, milestone
`e4-build-entry`) — not as a false-green E3 umbrella.

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
| Package / Config | E4 card `#560`; one-file vs split-file package graph tests as they land |
| Inspect / LSP / Canvas | existing focused suites; Canvas remains E8 for unfinished UI |

## 2026-07-15 rulings

Recorded in `syntax-decisions.md`. Superseded spellings must not remain as a
second authority (I8). Migration behavior for retired role-files follows the
ratified Package law on the owning child cards.

## Honest remainder (E4)

Package-graph depth that depends on Epoch 4 metaprogramming umbrellas stays on
`#560` / E4. This ledger does not invent a parallel Package mechanism and does
not claim E4 Package-graph scope done.
