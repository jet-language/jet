# Docs cleanup and Docs-tab streamlining (proposal)

Proposal only. **No file moves in this sweep.** Owner picks archive vs delete
per row, then an implementer runs the actions through Tower
(`tower docs archive` / `tower docs delete`).

## Goals

1. Docs UI shows **current, important** markdown only.
2. Binding law (`docs/spec/*`) has its own collapsible Spec section.
3. Sidequest plans appear under **Plans**, not Other.
4. `docs/archive/**` and `docs/ballots/**` never appear in the Docs UI or its
   counts (ballots stay Tower-owned; archive stays on disk for archaeology).
5. Every non-spec file has two exits: **Archive** (move to `docs/archive/`) or
   **Delete** (remove). Spec stays binding — amend or ballot; do not archive.

## Already done in Tower (this change)

| Behavior | Status |
|---|---|
| Collapsible Spec section for `docs/spec/*` | Done (default collapsed) |
| `docs/sidequests/*` listed under Plans | Done (path unchanged) |
| Archive + ballots hidden from list/counts | Done |
| UI + CLI `archive` beside `delete` | Done |
| Spec archive blocked | Done |

Physical layout changes below wait for owner choices.

## Proposed layout (after owner pass)

```
docs/
  README.md, first-hour.md          # keep (thin index + beginner entry)
  spec/                             # binding — keep all
  plans/                            # durable program law + sidequests subtree
    sidequests/                     # optional later: move docs/sidequests here
  proposals/                        # open ideas only
  research/                         # open research only (dated, not exhausted)
  audits/                           # latest pulse per skill, or empty + README
  reference/                        # user-facing reference (keep)
  archive/                          # hidden from Docs UI; historical only
  ballots/                          # Tower durability scan; hidden from Docs UI
```

Do **not** invent new top-level buckets. Prefer delete over archive when the
skill can regenerate the report.

## Decision table — research

| Path | Action | Why |
|---|---|---|
| `research/surface-research-2026-07-23.md` | **Archive** (or trim + keep) | §1 lazy iter largely landed (D-ITERTOOLS1 / #743). Keep only still-open sections (tasks/channels, typed prefixes, reflection) *or* archive whole file and re-mine later via skill. |
| `research/lessons-learned-2026-07-23.md` | **Archive** | Point-in-time report; durable re-run is `.agents/skills/lessons-learned/`. Duplicate of archived lineage notes. |
| `research/language-shape-research.md` | **Archive** | Research for open ballots; large; not law. Keep only if a live ballot still cites it — else archive. |
| `research/2026-07-24-devenv-parity.md` | **Keep** (active) | Owner locks dated 2026-07-25; feeds jetpack/env work. |
| `research/2026-07-24-rust-struct-impl-colocation.md` | **Archive** | Verdict recorded; idea-generation complete. |
| `research/2026-07-24-verse-video-mining.md` | **Archive** | One-shot mine-video report. |
| `research/2026-07-24-logan-smith-rust-series-mining.md` | **Archive** | One-shot mine-video synthesis. |
| `research/_scripts/` | **Keep** (ignored by Docs UI) | Tooling; non-md already excluded. |

## Decision table — audits

| Path | Action | Why |
|---|---|---|
| `audits/*-2026-07-23.md` and `spec-compliance-audit-2026-07-22.md` | **Archive** (batch) | Dated pulse snapshots. Skills regenerate fresh reports. Keep `audits/README.md` pointing at the skills. |
| `audits/README.md` | **Keep** | Index for how to re-run. |

## Decision table — proposals

| Path | Action | Why |
|---|---|---|
| `proposals/yielding-loops.md` | **Archive** | Owner deferred 2026-07-23; revisit after #732. Not active. |
| `proposals/transactional-rollback-regions.md` | **Keep** | Open proposal. |
| `proposals/ecosystem-shape.md` | **Keep** or **split** | Live ecosystem thinking; if D-ECO-* ballots fully cover it, archive. Prefer keep until epoch-4 package law absorbs it. |
| `proposals/README.md` | **Keep** | Index. |

## Decision table — sidequests / plans

| Path | Action | Why |
|---|---|---|
| `sidequests/web-backend-wasm.md` | **Keep** (under Plans in UI) | Status PARTIAL; criterion 5 / #705 still open. Later: move file to `plans/sidequests/` when folding the directory. |
| `sidequests/generic-modules.md` | **Keep** until carded/spec'd | Still a plan, not shipped law. |
| `sidequests/README.md` | **Keep** (short) | Points at exceptional cross-epoch plans. |
| `plans/epoch-*`, `compiler-speed.md` | **Keep** | Active durable law. |
| `plans/epoch-6/*` canvas matrix docs | **Keep** or **trim** | Large; keep if still the canvas acceptance spine, else fold into one epoch-6 README + archive matrices. |
| Already under `docs/archive/` | **Leave** | Already hidden from Docs UI. Optional later: delete regenerable matrices if disk noise matters. |

## Decision table — root / reference / other

| Path | Action | Why |
|---|---|---|
| `docs/README.md`, `first-hour.md` | **Keep** | Entry points (Other is fine). |
| `reference/**` | **Keep** | User-facing; errors are generated product. |
| `ballots/**` | **Hide only** (done) | Not Docs-tab content; Tower lint still scans. |

## Integration rules (after owner marks the table)

1. Prefer `tower docs archive <path>` for research/audit/deferred proposals.
2. Prefer `tower docs delete <path>` only when the skill regenerates the same
   report and nothing live cites the path.
3. Before archive/delete, `rg` the path from `docs/spec`, Tower cards, and
   skills; update those pointers first.
4. Do not move `docs/spec/*`.
5. Optional follow-up (separate change): `git mv docs/sidequests docs/plans/sidequests`
   once the Docs alias can be dropped.

## Board hygiene note (related, not a docs move)

Burndown worktrees (`jet-bd-722`, `jet-bd-732-c8`, …) hold **local**
`plugins/tower/.tower/tower.json` edits that are not always merged back to the
shared board. Example: `e3-729-w1` claim/start logs for #729 exist on
`jet-bd-722` but were missing from master at proposal time; #732 shows
`done` in some worktrees while master still has `building`. Agents must
`--log` and phase-update on the **shared** board whenever a merge or
substantive slice lands — worktree-only board writes do not count.
