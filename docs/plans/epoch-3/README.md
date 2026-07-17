# Epoch 3

Tower owns live cards, order, claims, blockers, and decisions:

```sh
nix develop -c node plugins/tower/tower.mjs status
nix develop -c node plugins/tower/tower.mjs next --burndown
```

This directory keeps only durable cross-card law:

- [universal-language-core.md](universal-language-core.md) — binding
  truth-repair and product-parity acceptance.
- [syntax-law-source-status-matrix-2026-07-07.md](syntax-law-source-status-matrix-2026-07-07.md)
  — machine-checked syntax implementation matrix.
- [marker-plane-source-of-truth-matrix-2026-07-07.md](marker-plane-source-of-truth-matrix-2026-07-07.md)
  — machine-checked marker-plane inventory.

`plugin-api.md` remains pending owner routing: its application-plugin target
shipped, but its separate compiler-extension hooks have no live card.

Per-feature plans belong on Tower cards. Delete completed, superseded, or
fully carded plan files after durable law and acceptance evidence exist.

## Promotion

1. Queue every owner gate in Tower before implementation.
2. Record ratified syntax in `docs/spec/syntax-decisions.md`.
3. Put card-specific sequencing and executable exit criteria on the card.
4. Promote only cross-card durable law into this directory.
