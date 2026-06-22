# Plan: Reconcile first-party stdlib ring claims

**Status:** planned. No language decision required for documentation triage.

## Goal

Make first-party library docs match the compiler and loader reality.

## Implementation Steps

1. Inventory docs claims for `jet.http`, `jet.crypto`, `jet.archive`, `jet.db`, and related
   rings.
2. Cross-check against `Source/Sema/CheckerCoreLib.rs`, `Source/Loader.rs`, examples, and
   tests.
3. Reclassify each item as shipped, partial, deferred, or package-roadmap.
4. Patch docs to avoid implying TLS/HMAC/archive/db are available if they are not.
5. Add Tower links for deferred package work.

## Verification

- Targeted `rg` for library names after docs patch.
- `node tools/Tower/Tower.mjs status`
- Existing docs/decision tests if spec files are touched.
