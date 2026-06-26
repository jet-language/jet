# Plan: Units as a unit family (D-QUAL3)

**Status: implemented (2026-06-25).** The owner ratified **D-QUAL3** (option C),
which superseded the earlier `#unit(usd)`-tag framing: units ship as a
`#UnitFamily(name) { … }` declaration that mints one **distinct `#Numeric`
type per member** (`usd` → `Usd`, erasing to `Float`), so signatures read in
plain English — the "upgrade to D-DIST2" framing of D-UNIT1. It is pure sugar
over the already-shipped distinct-type machinery (D-DIST1/D-DIST3), not a
separate parameterized-tag representation.

## What shipped

- `#UnitFamily(currency) { usd, eur, gbp }` parses to an `UnitFamilyDef`
  (`Source/Syntax.rs` `ATTR_UNIT_FAMILY`; `Source/Parser/Items.rs`
  `unit_family_def` / `at_unit_family_def`; `pub` form supported).
- Sema registration (`Source/Sema/Registration.rs`, `Source/Sema/Bundle.rs`)
  and codegen (`Source/Codegen/Context.rs`, `mod.rs`, `Imports.rs`) lower each
  member to a `#Numeric` distinct `DistinctDef` over `Float`
  (`UnitFamilyDef::distinct_defs`), PascalCasing the member name
  (`UnitFamilyDef::type_name`: `usd`→`Usd`, `m_per_s`→`MPerS`).
- Each member rides the existing distinct path: construct `Usd(9.99)`,
  same-unit arithmetic stays in the unit, `.raw()` strips it.
- Cross-unit mixing (`Usd + Eur`) reuses the distinct same-type rule
  **E0127** (the spec named "E0129", but the diagnostics.md split assigns
  E0129 to distinct-over-distinct and E0127 to same-type arithmetic; reusing
  the distinct machinery keeps one rule). The base `Float` does not coerce
  into a member (D-DIST3).
- The family erases in codegen (I3) — members lower to
  `#[repr(transparent)]` newtypes, no marker artifact, no `unsafe`.
- Formatter emits the family verbatim (the sugar surface is preserved, not
  expanded).

## Verification

- Example `examples/features/112_unit_family.jet` + golden
  `expected/112_unit_family.out`.
- UI snapshot `tests/ui/unit_family_mix` (cross-unit mix → E0127).
- Integration tests `tests/unit_family.rs` (same-unit ok, cross-unit E0127,
  base non-coercion, PascalCase multi-word member, codegen erasure).
