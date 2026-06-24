# Epoch 3 — testing & docs ergonomics (property tests, doctests, coverage)

**Status:** tracked Epoch-3 milestone (owner, 2026-06-18). Moved out of the
Epoch-2 GA bar. The E2 testing core ships: `#Test "…" { require/require_eq }`
blocks, snapshot `expect(...).snapshot()` with `--update-snapshots`, the `#Todo`
typed hole, and the whole-program `jet bench`. **All four ergonomics items below
are now ratified with no open syntax gate** (D-TEST1, D-TEST4, coverage=tooling,
D-BENCH1) — this plan is implement-ready for the burn-down.

## Items

1. **Property testing (D-TEST1 = B, ratified 2026-06-22)** — **unblocked, no open
   gate.** A parameterized `#Test fn` *is* a property test: inputs are generated
   from the param types, with automatic invisible shrinking; a no-param `#Test` is
   a plain unit test. **Zero new syntax** (matches S82) — the earlier "needs a
   surface ballot" concern is resolved by reusing test params. Shrinking
   (minimizing the failing input) remains the core value to implement.

2. **Doctests (D-TEST4 = A, ratified 2026-06-22)** — **unblocked, no open gate.**
   Code in `///` doc comments (S49) runs as tests under `jet test`; the expected
   output is a `// =>` trailing comment on the producing line; a mismatch fires
   **E2901** (reserved in `diagnostics.md`). Reuses `//` (S5) — no new tokens.

3. **Coverage** — per-line / per-function coverage from a `jet test` run. No new
   syntax; tooling only. Couples to the test runner in `Source/main.rs`
   (`run_test`).

4. **Region benchmarks (D-BENCH1 = A, ratified 2026-06-24)** — **unblocked, no
   open gate.** A first-class `#Bench "name" { … }` block, the exact sibling of
   `#Test "name" { … }`, lets an author benchmark a region in isolation. The
   **existing** `jet bench` verb (D-TOOL5, today times a whole program) discovers
   and runs every `#Bench` block, reporting per-region ops/sec + ns/iter — no new
   verb, no `jet test --bench` form (the owner Q on the runner verb resolved to the
   existing `jet bench`). `#Bench`/`KW_BENCH` is registered in `Source/Syntax.rs`;
   the PascalCase marker joins the `#Test`/`#Pure`/`#Todo`/`#Caps` family. Build
   order: parser (mirror `#Test` block parsing in `Source/Parser/Items.rs`) →
   discovery + per-region timing in `CmdDevTools.rs`'s `run_bench` (reuse the
   warmup/trial + mean±stddev harness) → an `examples/` entry with golden output
   (I5). No new diagnostic required at ratification.

## Exit criteria

- A property test with a shrinking failure report runs under `jet test`.
- A `///` doc example runs under `jet test`; a mismatch fires E2901.
- `jet test --coverage` (or equivalent) reports coverage.
- `jet bench` discovers and runs `#Bench "name" { … }` blocks, reporting
  per-region ops/sec + ns/iter; a golden example pins the output format.

## Prerequisite

**None — all four items are unblocked.** D-TEST1 (B), D-TEST4 (A), and D-BENCH1
(A) are ratified; coverage is tooling-only. Each item can land independently in
the burn-down; no item is gated on an unratified decision.
