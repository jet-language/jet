# ca3ynsl — Reversible computation / layout constraint solver

**Status:** IMPLEMENTED 2026-07-02 — both gates ratified (D-LAYOUT-GATES1=A),
Path A built in full. See `docs/spec/syntax-decisions.md` D-LAYOUT1 for the
implementation log (files touched, adaptations from this doc's illustrative
flavor text, deferred items).

## Goal

Implement `core.layout` as a Cassowary-style incremental linear constraint solver,
enabling first-class layout expressions in the `layout {}` block introduced by
D-LAYOUT1=A.

## Scope

The full D-LAYOUT1=A design (axis-typed variables, `layout {}` block, operator overloading
for constraint production) requires two compiler language gates (D-LAYOUT-GATES1).
If the gates are ratified, implement option A in full. If either gate is declined,
implement option B (method-only builder) instead — that path is unblocked today.

**Non-goal:** general constraint solving outside of layout; SMT/theorem provers;
D-REVERSE1 general reversibility (separate scope).

## Paths

### Path A — full typed-axis layout (gates ratified)

1. Register `HVar`, `VVar`, `LengthVar`, `Constraint`, `LayoutHandle` as closed built-in
   types in `Source/Syntax.rs` and `core_type_known`.
2. Wire comparison operators (`>=`, `==`, `<=`) on `HVar`/`VVar`/`LengthVar` to return
   `Constraint` in sema (`CheckerInfer.rs`, `CheckerCoreLib.rs` tables).
3. Parse `layout { ... }` block as `LayoutBlock` AST node (`Source/Parser/Items.rs`).
4. Sema: each line in `layout {}` must produce a `Constraint`; cross-axis mixes
   (`HVar op VVar`) → E-LAYOUT-AXIS-MISMATCH.
5. Port Cassowary incremental simplex solver to Rust (stdlib, no external crate per I6).
   Pure Rust under `Source/Prelude/` or `stdlib/core/layout/`.
6. Codegen: `layout {}` desugars to `LayoutHandle` creation calls + `solver.add()`.
7. Expert: captured handles, `.priority(.medium)`, `solver.suggest()`.
8. Static layout detection: all values comptime-known → evaluate at compile time,
   emit zero runtime solver.
9. Diagnostics: E-LAYOUT-AXIS-MISMATCH, E-LAYOUT-INFEASIBLE (conflict report),
   E-LAYOUT-REDUNDANT (warning).

### Path B — method-only builder (fallback if either gate declined, unblocked today)

1. Add `core.layout.Constraint` as an opaque struct (no closed-type ops needed).
2. Builder methods: `.width_gte(n)`, `.eq(other)`, etc. on `LayoutBuilder`.
3. Solver still runs at runtime (step 5 above applies to both paths).
4. No `layout {}` block syntax.

## Verification

- `examples/features/NN_layout_basic.jet` — simple form layout, golden output.
- UI snapshots: E-LAYOUT-AXIS-MISMATCH, E-LAYOUT-INFEASIBLE.
- Unit tests for solver convergence on common patterns.
- `nix develop -c cargo test` fully green.

## Decision status

**D-LAYOUT-GATES1 is open** — owner must ratify both language gates (or decline them)
before Path A work starts. Path B is implementable immediately if gates are declined.
No other decision blocks either path.
