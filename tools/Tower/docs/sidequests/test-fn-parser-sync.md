# Plan: `#Test fn` parser/spec sync

**Status:** planned. No owner decision required if this only reconciles parser with spec.

## Problem

The spec says test blocks/functions have a `#Test` surface, but parser support and docs
have drifted around `#Test "name" { ... }` versus `#Test fn name() { ... }`.

## Implementation Steps

1. Confirm the ratified surface in `docs/spec/syntax-decisions.md` and `docs/spec/spec.md`.
2. Update parser dispatch for `#Test fn name() { ... }` if the spec requires it.
3. Decide whether formatter emits a canonical form or preserves both accepted forms.
4. Add UI snapshots for:
   - accepted top-level `#Test fn`
   - rejected nested `#Test fn`
   - malformed test function signature
5. Update docs/examples so there is one canonical beginner-facing form.

## Verification

- `cargo check`
- `nix develop -c cargo test --test ui -- --nocapture`
- `nix develop -c cargo test --test jet_test -- --nocapture`
- `nix develop -c cargo test --test decisions -- --nocapture`
