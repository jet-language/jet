# Language-shape cross-surface conformance

E3 shipped the cross-surface match for syntax registry, parser, sema, formatter,
diagnostics, examples, and editor grammars. The package-graph depth that depends
on Epoch 4 metaprogramming is closed on Tower card `#560` in epoch `e4`, with
production workspace-driver and focused build-entry proof.

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
| Package / Config | E4 card `#560`; workspace-driver and split-file package graph tests |
| Inspect / LSP / Canvas | existing focused suites; Canvas remains E8 for unfinished UI |

## 2026-07-15 rulings

Recorded in `syntax-decisions.md`. Superseded spellings must not remain as a
second authority (I8). Migration behavior for retired role-files follows the
ratified Package law on the owning child cards.

## E4 closeout

Card `#560` closes the package-graph remainder through the production workspace
driver. Members run in dependency order with separate authority contexts; root
CLI grants do not authorize members, and explicit package/workspace grants still
apply through the same policy resolver. Focused proof lives in
`tests/build_entry.rs`, `tests/build_entry_epoch4.rs`, and `tests/build_graph.rs`.
