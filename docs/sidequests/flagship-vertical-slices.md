# c123 — Ship flagship vertical slices per domain

**Superseded 2026-07-02.** The owner converted this card into an Epoch 3 exit
criterion (each pillar ships its slice: CLI, server, low-level/freestanding,
web, game). The plan is
[`../plans/epoch-3/flagship-slices.md`](../plans/epoch-3/flagship-slices.md)
(marked historical: `examples/apps/` + `tests/slices.rs` removed 2026-07-14).

This file's earlier draft predated that conversion: it targeted the retired
`examples/showcase/` tree and a data-pipeline slice instead of the web pillar.
Its surviving ideas (per-slice proof bar, jetgrep hardening, deterministic
`--replay` goldens, host-runnable tests for freestanding logic) are folded into
that plan.
