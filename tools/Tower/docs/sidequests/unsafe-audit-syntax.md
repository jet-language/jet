# Plan: Unsafe and audit marker syntax

**Status:** planned. Depends on owner decision D-UNSAFE2.

## Goal

Resolve the low-level syntax question without weakening the safety audit story.

## Recommended Path

If D-UNSAFE2 lands as recommended, keep the current two-marker model:

- `#Audit("reason")` records the human safety argument.
- `#Unsafe { ... }` marks the expert-tier region.

## Implementation Steps

1. Audit parser support for `#Audit` and `#Unsafe` casing.
2. Ensure diagnostics and docs consistently use `#Audit` / `#Unsafe`, not lowercase
   legacy forms.
3. Add or refresh UI snapshots for missing audit, misplaced audit, and unsafe outside
   `use core.mem`.
4. Add a small docs example showing the two-marker pattern.
5. If the owner picks a merged spelling instead, update parser, formatter, docs, and
   snapshots in one narrow patch.

## Verification

- `cargo check`
- `nix develop -c cargo test --test ui -- --nocapture`
- `nix develop -c cargo test --test decisions -- --nocapture`
