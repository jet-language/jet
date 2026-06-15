# Post–Epoch 2 plans

Work deferred past **E2-M17 (Epoch 2 GA)** unless the owner promotes it
earlier. Epoch 2 may lay foundations (e.g. manual `extern c` in E2-M14) that
these features build on, but the items here are **not** Epoch 2 exit criteria.

| Plan | Goal | Blocked on |
|------|------|------------|
| [`c-header-bindings.md`](c-header-bindings.md) | Optional C-header → Jet binding tool (`jet bind` / `import c`) | E2-M14 manual C FFI + E2-M13 low-level tier; owner D-CBIND1…8 |

When promoting an item into a numbered epoch, add a milestone file under
`docs/plans/epoch-3/` (or amend the active epoch README) and move decision
ballots into docs/spec/decision-ballots.md.
