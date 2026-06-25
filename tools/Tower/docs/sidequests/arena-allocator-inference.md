# c26 — Arena allocator compiler inference (far horizon)

**Status:** READY (far-horizon). Coherent lint-first impl plan; no open owner decision
blocks the next step. Verified 2026-06-25 — the surface it builds on exists and is
ratified: `region r { … }` is `KW_REGION` (`Source/Syntax.rs:241`, D-REGION1 opt B, with
escape rule E0631 / reset-outlive E0632); D-ALLOC1 (allocator method style, ratified
2026-06-19, `docs/spec/syntax-decisions.md:1752`), D-ALLOC2 (arena `alloc` return +
reset/free safety, ratified 2026-06-21, `:2097`), D-REGION1 (allocation regions, ratified
2026-06-21, `:2113`).

## Goal

Explore arena placement inference without changing the shipped allocator surface. This
must not weaken D-ALLOC2's scope-bound view rule or D-REGION1's explicit expert regions.

## Scope

- Inputs: allocation sites, escape analysis, existing region scopes.
- Output: suggested or inferred arena placement where lifetime is statically bounded.
- Non-goal: replacing explicit `region r { }` or `core.mem.alloc` APIs.

## Implementation Steps

1. Document the current allocator/region invariants from D-ALLOC1, D-ALLOC2, and
   D-REGION1.
2. Add an internal analysis prototype that classifies allocation sites as local,
   scope-bound, escaping, or unknown.
3. Start as a lint/suggestion, not automatic lowering.
4. Add examples for clear wins: temporary parse buffers, request-local scratch data,
   frame-local simulation allocations.
5. Promote to actual inference only after diagnostics prove understandable.

## Verification

- Unit tests for escape classification.
- UI snapshots for suggestions.
- No codegen changes until the analysis is stable.

## Decision status

No open owner decision blocks the lint-first work (steps 1–4): it is pure additive
analysis that emits suggestions and changes no default placement, so it preserves
D-ALLOC2's scope-bound view rule and D-REGION1's explicit regions by construction.
**One future gate, not a current blocker:** step 5 (promote suggestion → automatic
inferred placement) changes a default and touches I1/I8, so it needs its own owner
decision *when it is reached* — explicitly downstream, after diagnostics prove
understandable. Until then the card is READY for the prototype + lint surface.
