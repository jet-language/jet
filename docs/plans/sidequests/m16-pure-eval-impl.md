# Sidequest: E2-M16 — Pure evaluation and package layer 3 implementation

**Plan:** `docs/plans/epoch-2/m16-pure-eval-layer3.md`  
**Status:** all decisions ratified; ready to implement after M8 + M4 ✅  
**Depends on:** E2-M8 (store/lockfile), E2-M4 ✅ (interpreter for evaluation)

## Critical amendment: D-PURE3

**Ship the signed cache in M16** — the owner chose B against the original recommendation (A = design-only, ship later).

This means M16 must implement, not just design:
- Generation tracking for the package cache
- Signed cache entries (signature scheme to be specified in the implementation)
- Rollback to a prior generation
- `jet store rollback <gen>` or equivalent CLI surface

The design-only approach was explicitly rejected.

## Other decisions (no amendments)

| Decision | What to implement |
|---|---|
| D-PURE1 | `pure fn` checked modifier + `jet eval --pure` + sandboxed package build blocks |
| D-PURE2 | Sandbox: no ambient I/O or network during pure evaluation |
| D-PURE3 | **Ship** signed cache + generations/rollback in M16 |
| D-FP3 | `module name { }` typed declaration (already recorded in M6 D-FP3) |

## JetOS boundary

JetOS stays research-only (E2-V12). M16 builds the pure-eval foundation; jetos Phase 2 (system builds) unlocks after M16 ships. Do not conflate the two — M16 delivers pure eval + signed cache, not a usable OS.

## Diagnostics to register (E34xx)

E3401 (impure call inside `pure fn`), E3402 (ambient I/O in package build), E3403 (non-deterministic construct in pure eval).

## Exit criteria

See `m16-pure-eval-layer3.md` — plus: signed cache and generation rollback ship (D-PURE3=B). `nix develop -c cargo test` green.
