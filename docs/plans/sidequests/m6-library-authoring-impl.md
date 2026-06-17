# Sidequest: E2-M6 — Library authoring implementation

**Plan:** `docs/plans/epoch-2/m6-library-authoring.md`  
**Status:** all core decisions ratified; D-ERR1 and D-FP1 still open (implement without them)  
**Depends on:** E2-M1 ✅, E2-M5 ✅  
**Unblocks:** E2-M8, E2-M9

## Ratified decisions (implement these)

| Decision | What to implement |
|---|---|
| D-LIB1 | S61 (optional argument labels + trailing defaults) AND S62 (trait delegation `impl T using field`) both land in M6 |
| D-LIB2 | Generics v1: associated types (`type Key` inside trait) + default method bodies |
| D-LIB3 = D-ERR2 | `Fallible` trait + `Error` carrier (msg + optional code + optional source) |
| D-OWN1-3 | Strengthen implicit-clone lint; add ownership mini-examples; suggest `take` at call site |
| D-JSON2 | Ignore unknown JSON keys by default; opt-in strict mode |

## Open (do not block on these)

- **D-ERR1** (grow `Error` carrier fields) — not yet ratified; use the D-LIB3 shape for now
- **D-FP1** (struct field punning `Source { name, upstream }`) — not yet ratified; skip for now

## D-JSON1 note

D-JSON1-decode (lenient coerce `"8080"` → `8080`) is ratified. Implement in jet.json (M9).
M6 only needs to know that `?` propagation of JSON errors must use the `Fallible` trait path.
The coercion visibility follow-up is tracked in `json1-coercion-visibility.md`.

## Diagnostics to register (E24xx)

E2401, E2402, E2403, L2401 — see milestone plan for what/why/fix text.

## Exit criteria

See `m6-library-authoring.md`. Key: `examples/features/36_library.jet` runs; `nix develop -c cargo test` green.
