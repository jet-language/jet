# Probe prim-units — Physical units: library versus type system

## What I built

I built a package-level units layer with `Meter`, `Centimeter`, `Second`, `Hertz`, `Speed`, and `Acceleration`. It has constructors, conversion, derived-unit functions, checked same-unit addition, formatting, and a 4-tick control loop that integrates acceleration and converts the final position. I also built focused negative and positive probes for compiler units, generic dimension encodings, const parameters, cross-type operators, and trait coherence.

Files: `pkg/package.jet`, `pkg/units.jet`, `pkg/run.jet`, `pkg/probes_builtin.jet`, `pkg/probes_checked.jet`, `pkg/probe_wrong_dimension.jet`, `pkg/probe_conversion.jet`, `pkg/probe_rounded.jet`, `pkg/probe_checked_result.jet`, `pkg/operator_probe.jet`, `pkg/generic_probe.jet`, `pkg/generic_probe_symbolic.jet`, `pkg/generic_operator_probe.jet`, `pkg/generic_operator_probe2.jet`, `pkg/const_probe.jet`, `pkg/const_dimension_probe.jet`, `pkg/foreign_type.jet`, and `pkg/probe_coherence_import.jet`. Isolated copies under `buildcheck/`, `clean/`, and `xid/` separate package/build behavior from the negative fixtures.

## What worked

- **Shipped compiler unit literals and derived dimensions:** works. `scripts/agent/jet-env jet run pkg/probes_builtin.jet` prints `12 meter`, `4 meter/ns`, `2 hertz`, `12 Meter`, `12`.
- **Static dimensional addition/multiplication/division:** works. `jet run pkg/probes_checked.jet` prints `120 centimeter`, `4 meter/ns`, `6 meter^2`; `1meter + 1s` is rejected with E0359.
- **Exact conversion and explicit rounded conversion:** works. `jet run pkg/probe_conversion.jet` with `300centimeter` prints `3.0`, `3 meter`, `3.0`; `jet run pkg/probe_rounded.jet` with `150centimeter` prints `2 meter`, `2.0` after the required `.NearestEven` policy.
- **Library wrappers and a control loop:** default execution works. `scripts/agent/jet-env jet run pkg/run.jet` prints:
  ```
  40.0 m
  4000.0 cm
  20.0 m/s
  2.0 Hz
  ```
  The library code can safely distinguish the wrapper structs and perform the arithmetic through named functions.
- **Phantom dimension parameters:** a library can encode a symbolic product tree. `jet run pkg/generic_probe_symbolic.jet` prints `2.0` for `Quantity<Product<MeterDim, SecondDim>>`; this is an encoding, not normalized dimensional algebra.
- **Const values in the supported generic-module path:** works for fixed lists. `jet run pkg/const_probe.jet` prints `3` for `fixed<Int>(3)` and `[Int#3]`.
- **Facts confirm the shipped boundary:** `jet inspect facts --json` reports `UnitFamily` as a marker with safe direction `none`, `Type.Dimension` as a plane with safe direction `gain`, and `UnitScaleProvenance` as a plane with safe direction `none`.

The standard library already covers the important physical-unit mechanism. `Prelude/Units.jet` declares base and derived families, exact scale/offset provenance, and affine temperature support. The archived control-systems, power-energy-systems, space-satellite, and electromagnetics-antenna reports all classify compiler-known dimensions/scales/affine quantities as shipped; their remaining gaps are domain models, not a missing units library.

## Gaps

1. **[defect, blocks] Native code generation can ICE on an ordinary cross-file fallible library function.** `scripts/agent/jet-env jet build --verbose /home/nate/.cache/jet-luna/dx3/prim-units/buildcheck/run.jet` reaches front-end success and then reports `runtime cache bypassed (split crate rejected — inline retry)` followed by `internal compiler error: the generated Rust did not compile` (generated `/home/nate/Projects/Github/jet/build/run.rs`). The isolated package has only the units layer/control loop and grants `IO, Mem.Alloc, Panic`; default `jet run` succeeds. Shared by embedded, science, and games when a units package exposes fallible validation.
2. **[impossible, hurts] Library generics cannot perform type-level dimension arithmetic or normalization.** `jet run pkg/generic_probe.jet` rejects `Quantity<A * B>` with E0003: `Expected ">" after Quantity<…>, found *`; the generic grammar accepts types only. A library can carry `Product<A,B>` as a phantom marker, but cannot make `Meter / Second` normalize to a canonical `Speed`, cancel exponents, or prove equivalent product trees. Shared by embedded, science, and games.
3. **[impossible, call-site, boilerplate, hurts] Library operator hooks cannot express a cross-type operation with a new result type.** `jet run pkg/operator_probe.jet` reports E0907 (`div` doesn't match `Div`; impl methods must match the trait signature exactly), then E0360 (`No / operator is defined for Meter`; dispatch uses one `Meter.Div` hook). The workaround is a named `meters_per_second(Meter, Second) -> Speed` function for every useful pair. Shared by all three areas.
4. **[impossible, boilerplate, hurts] Ordinary generic structs cannot take numeric/const dimension parameters.** `jet run pkg/const_dimension_probe.jet` rejects `Quantity<MeterDim, 1>` with E0003: `Expected a type name, found a number`. Numeric parameters work only in the special generic-module/fixed-list path (`[T#capacity]`), not as a general dimension index. Phantom marker types or hand-written wrappers are the workaround. Shared by embedded, science, and games.
5. **[impossible, annoys] A package cannot add a trait implementation for an imported quantity type.** `jet run pkg/probe_coherence_import.jet` reports E0902: `This impl can't live here` and explains the orphan rule: at least the trait or type must be defined in the program; E0360 then reports no `+` operator for the imported type. A library must own both its quantity type and operator impl, or expose a wrapper/local trait. Shared by embedded, science, and games when composing independently authored unit packages.

## Friction

The shipped compiler path has low call-site ceremony: `12meter / 3s`, `print(distance)`, and explicit `Meter.from_centimeter_rounded(...)` match the intent. The historical D-M-UNITS1 comparison records F# `[<Measure>]` types and ordinary arithmetic, Rust `uom` constructors/operators, and Julia Unitful literals such as `12u"kN"`; Jet's built-in literal path is comparable, while a library-only implementation is not.

The pure-library control loop needs six nominal structs, four constructors, six arithmetic/conversion helpers, four formatting helpers, and explicit calls at every operation. In `pkg/run.jet`, the user writes `units.meter`, `units.second`, `units.meters_per_second_squared`, `units.acceleration_step`, `units.speed_per_second`, `units.checked_meters_add`, two conversions, and four formatters. This is author boilerplate and user call-site ceremony, but it is a workaround for missing generic arithmetic—not a reason to duplicate the compiler unit mechanism in a library.

A fallible library function uses `!Err` and `??`; `jet run pkg/probe_checked_result.jet` correctly reports `Error: meter sum is not finite` with an E3002 trail. Same-family static checking itself is already handled by the shipped unit system.

## Defects

- **Native build ICE:** the exact isolated command and output are recorded in Gap 1. Front-end checking and default execution pass; the failure is in generated-Rust compilation after split-crate rejection. Adding the missing `Panic` authority does not change the failure.
- **Stale example fixture, not counted as a unit primitive gap:** `jet run examples/features/types/affine_unit_types.jet` fails at its own `run()` body with E0405 (`?? return can't return a value here`); the unit syntax is not the reported cause.

## Battery notes

Not applicable: this is a primitive probe, and the brief explicitly requires no `batteries.json`. The three informed areas need no new first-party unit battery for the mechanism itself; their archived probes already show compiler units working.

## Verdict

**Buildable today for physical units:** Jet's shipped `#UnitFamily`/unit-literal system provides dimensions, exact scales, derived units, and affine quantities.
A pure library can wrap values and provide named conversions, but cannot reproduce type-level dimension arithmetic or ergonomic cross-type operators.
The default-tier control-loop program runs and produces the expected values.
Native AOT/build remains blocked by the isolated generated-Rust ICE for a fallible cross-file helper.
Fix the compiler defect first; consider generic dimension algebra/result-typed operator hooks only if library-defined units must match the built-in call-site experience.
