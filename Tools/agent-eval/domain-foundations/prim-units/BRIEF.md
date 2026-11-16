# Probe prim-units — Physical units: library versus type system

Read `$JET_STATE_ROOT/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: embedded, science, games.

## Build this

Find what Jet ships for units today (search spec, Prelude, Examples/Types/**, and `jet inspect facts`). Then build, as library Jet, a units layer: meters, seconds, Hz, derived units through multiplication/division, checked addition, formatting, conversion; use it in a 20-line control-loop program. Record where the type system stops you (type-level arithmetic, phantom parameters, const generics, trait coherence).

## Answer these

1. Is dimensional analysis buildable as a library with today's generics, or which type-system primitive is missing?
2. What is the call-site ceremony compared with F#, Rust uom, or Julia Unitful?

## Research to mine

Domain census and reports: `$JET_STATE_ROOT/dx2/control-systems/`, `$JET_STATE_ROOT/dx2/power-energy-systems/`, `$JET_STATE_ROOT/dx2/space-satellite/`, `$JET_STATE_ROOT/dx2/electromagnetics-antenna/` (report.md, census.json, claims.json). Deleted ballots with worked code: `$JET_STATE_ROOT/dx2/ballots/` (grep the mechanism name). Family syntheses: `$JET_STATE_ROOT/dx2/_families/*/synthesis.md`.

## Output

`$JET_STATE_ROOT/dx3/prim-units/probe.md`, `gaps.json`, and the code under `$JET_STATE_ROOT/dx3/prim-units/pkg/`. Gap ids start with `prim-units-G`.
