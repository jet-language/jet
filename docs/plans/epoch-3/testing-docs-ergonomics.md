# Epoch 3 — testing & docs ergonomics (property tests, doctests, coverage)

**Status:** tracked Epoch-3 milestone (owner, 2026-06-18). Moved out of the
Epoch-2 GA bar. The E2 testing core ships: `test "…" { require/require_eq }`
blocks, snapshot `expect(...).snapshot()` with `--update-snapshots`, the `todo`
typed hole, and `jet bench`. These three remaining items are ergonomics, not
core language, and each is gated on a syntax decision Jet hasn't made.

## Items

1. **Property testing (D-TEST1=A)** — ratified to ship *only if a small
   shrinking design exists*; shrinking (minimizing the failing input) is the
   value. **Blocked on surface syntax**: how a user declares a property and its
   generated inputs (e.g. `test "name" forall n: Int { … }` vs a `prop` keyword
   vs generated test params). Needs a ballot in `decision-ballots.md` with worked
   examples, then ratification, before any code (I7). Defer the whole feature if
   the shrinking design turns out large.

2. **Doctests (D-TEST4 = D-TOOL1, ratified A)** — doc examples run under `jet
   test`. **Blocked on surface syntax**: Jet has no `///` doc comment today (only
   `//` and `/* */`), so both the doc-comment marker and the example/expected
   format need a decision. E2901 ("doctest output mismatch") is reserved in
   `diagnostics.md`.

3. **Coverage** — per-line / per-function coverage from a `jet test` run. No new
   syntax; tooling only. Couples to the test runner in `Source/main.rs`
   (`run_test`).

## Exit criteria

- A property test with a shrinking failure report runs under `jet test`.
- A `///` doc example runs under `jet test`; a mismatch fires E2901.
- `jet test --coverage` (or equivalent) reports coverage.

## Prerequisite

Owner ratification of the property-test surface syntax and the doc-comment /
doctest convention (two ballots) before implementation. Coverage has no
prerequisite and could land independently.
