# Plan: Align public maturity claims with implementation

**Status:** planned. No language decision required.

## Goal

Make README and public-facing docs honest about what is production-ready, preview-only,
or still stubbed.

## Implementation Steps

1. Inventory status claims in README, roadmap, architecture, package, and release docs.
2. Cross-check each claim against implementation files and tests.
3. Reword public copy around:
   - registry publish/upload
   - package GC/store maturity
   - production readiness
   - Epoch/GA status
4. Keep aspirational roadmap language separate from shipped-feature language.
5. Add a docs checklist for future launch edits.

## Verification

- `rg` for risky phrases: production, GA, shipped, stable, registry, TLS, upload.
- Docs review against Tower cards c106-c113.
