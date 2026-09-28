# Docs cleanup and Docs-tab streamlining

**Status: accepted 2026-07-25** (owner: keep `proposals/yielding-loops.md`
active; otherwise execute the table). File pass executed the same day via
`tower docs archive`.

## Goals

1. Docs UI shows **current, important** markdown only.
2. Binding law (`docs/spec/*`) has its own collapsible Spec section.
3. Sidequest plans appear under **Plans**, not Other.
4. `docs/archive/**` and `docs/ballots/**` never appear in the Docs UI or its
   counts (ballots stay Tower-owned; archive stays on disk for archaeology).
5. Every non-spec file has two exits: **Archive** (move to `docs/archive/`) or
   **Delete** (remove). Spec stays binding — amend or ballot; do not archive.

## Tower Docs UI (shipped)

| Behavior | Status |
|---|---|
| Collapsible Spec section for `docs/spec/*` | Done (default collapsed) |
| `docs/sidequests/*` listed under Plans | Done (path unchanged) |
| Archive + ballots hidden from list/counts | Done |
| UI + CLI `archive` beside `delete` | Done |
| Spec archive blocked | Done |

## Layout after this pass

```
docs/
  README.md, first-hour.md          # keep
  spec/                             # binding — keep all
  plans/                            # durable program law (+ sidequests in UI)
  proposals/                        # open ideas (incl. yielding-loops)
  research/                         # active research only (devenv-parity)
  audits/                           # README + fresh skill output
  reference/                        # user-facing reference
  archive/                          # hidden from Docs UI
  ballots/                          # Tower durability scan; hidden from Docs UI
  sidequests/                       # exceptional cross-epoch plans
```

## Decision table — research

| Path | Action | Result |
|---|---|---|
| `research/surface-research-2026-07-23.md` | **Archive** | Archived |
| `research/lessons-learned-2026-07-23.md` | **Archive** | Archived |
| `research/language-shape-research.md` | **Archive** | Archived |
| `research/2026-07-24-devenv-parity.md` | **Keep** | Kept |
| `research/2026-07-24-rust-struct-impl-colocation.md` | **Archive** | Archived |
| `research/2026-07-24-verse-video-mining.md` | **Archive** | Archived |
| `research/2026-07-24-logan-smith-rust-series-mining.md` | **Archive** | Archived |
| `research/_scripts/` | **Keep** | Kept |

## Decision table — audits

| Path | Action | Result |
|---|---|---|
| Dated `audits/*-2026-07-2*.md` | **Archive** | Archived |
| `audits/README.md` | **Keep** | Kept |

## Decision table — proposals

| Path | Action | Result |
|---|---|---|
| `proposals/yielding-loops.md` | **Keep** (owner override) | Kept active |
| `proposals/transactional-rollback-regions.md` | **Keep** | Kept |
| `proposals/ecosystem-shape.md` | **Keep** | Kept |
| `proposals/README.md` | **Keep** | Kept |

## Decision table — sidequests / plans

| Path | Action | Result |
|---|---|---|
| `sidequests/*`, `plans/epoch-*`, `compiler-speed.md` | **Keep** | Kept |
| Prior `docs/archive/*` | **Leave** | Left |

## Still optional later

1. `git mv docs/sidequests docs/plans/sidequests` once the Docs alias can drop.
2. Trim epoch-6 canvas matrices if one README can carry acceptance.

## Board hygiene note

Burndown worktrees must `--log` and phase-update on the **shared** board when
substantive progress lands. Worktree-only `tower.json` edits do not count.
