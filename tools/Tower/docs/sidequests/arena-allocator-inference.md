# Plan: Arena allocator compiler inference

**Status:** planned as a far-horizon architecture slice.

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
