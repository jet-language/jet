# Sidequest: E2-M17 — Epoch 2 GA implementation

**Plan:** `docs/plans/epoch-2/m17-epoch2-ga.md`  
**Status:** all decisions ratified; implement last (depends on all other milestones)  
**Depends on:** E2-M1 through E2-M16, E2-M18

## All decisions ratified with important amendments

| Decision | Ratified pick | Delta from original rec |
|---|---|---|
| D-GA1 | **All 6 showcases mandatory** | **CHANGED**: rec was 4+1 stretch; owner chose B: all 6 are required gates |
| D-GA2 | **Hard CI perf/size gates** | **CHANGED**: rec was record-only; owner chose B: CI fails if budgets exceeded |
| D-GA3 | No public beta before GA tag | **CHANGED**: rec was a short beta; owner went straight to GA |
| D-GA4 = E2-D2 | Normal SemVer | Same as rec C |

## Critical: all 6 showcases are mandatory GA gates (D-GA1=B)

1. Fast CLI tool (M7/M9/M11/M8)
2. HTTP service with tasks, logging, TLS, sqlite (M1/M12/M10/M9)
3. Library package with API diffing, docs, doctests (M8/M11)
4. `jet dev` demo (M4 ✅)
5. **C interop example** (M14 ✅ — was originally a stretch)
6. **Low-level / freestanding smoke project** (M13 ✅ / M15 — was originally a stretch)

## Critical: hard CI perf/size gates (D-GA2=B)

Each showcase must have recorded perf/size budgets; CI fails if those budgets are exceeded. Budgets must be:
1. Measured and recorded before declaring GA
2. Encoded as CI assertions (not just documentation)

## DAP step-through debugger (from D-OBS1 split)

Full DAP for VS Code/Cursor is a **GA gate** (moved here from M12). M17 must include DAP integration working end-to-end in VS Code/Cursor at Jet source lines.

## Exit criteria

See `m17-epoch2-ga.md`. All 6 showcases build + CI smoke. Hard perf/size gates pass. DAP works in VS Code/Cursor. Every E2 diagnostic has `jet explain`. `nix develop -c cargo test` green; roadmap marks Epoch 2 done.
