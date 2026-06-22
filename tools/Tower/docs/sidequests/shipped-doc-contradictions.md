# Plan: Fix shipped-feature documentation contradictions

**Status:** planned. No language decision required; this is docs correctness.

## Goal

Resolve contradictions between docs and implementation for shipped features.

## Known Contradictions

- C bindgen described as functional in one place and deferred in another.
- README casing uses lowercase unsafe/audit while implementation uses `#Unsafe`/`#Audit`.
- TLS is advertised but HTTPS is rejected by the current core HTTP path.
- First-party package docs exceed implemented package surfaces.
- Terminator/semicolon docs drift from current parser behavior.

## Implementation Steps

1. Build a contradiction table: claim, source file, implementation reality, fix.
2. Patch docs to classify each feature as shipped, preview, deferred, or planned.
3. Prefer linking to the relevant Tower card for deferred pieces.
4. Add a short "feature status" note where a page mixes shipped and planned content.

## Verification

- `rg` targeted phrases after patch.
- `node tools/Tower/Tower.mjs status`
- `nix develop -c cargo test --test decisions -- --nocapture` if syntax docs change.
