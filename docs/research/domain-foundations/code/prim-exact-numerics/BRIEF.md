# Probe prim-exact-numerics — Exact and arbitrary-precision numbers as a library

Read `~/.cache/jet-luna/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: data, backend, science.

## Build this

Implement, as library Jet: a Rational type over the core BigInt, a Decimal with a scale, and money arithmetic with rounding modes; use them with literals, operators, comparisons, formatting, JSON round trip, and a sum over 1e6 values. Check what the language gives a library author: operator traits, literal suffixes or constructors, generic numeric bounds, const evaluation. Mine the deleted ballot for M-EXACT-NUMERIC (~/.cache/jet-luna/dx2/ballots/ if present) and examples/features/operators/**, examples/features/traits/**, examples/features/comptime/**.

## Answer these

1. Can a library type feel like a built-in number (literals, operators, printing) or which primitive is missing?
2. Is bigint speed in the band against Python int / Rust num-bigint?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/fintech-payments/`, `~/.cache/jet-luna/dx2/accounting-erp/`, `~/.cache/jet-luna/dx2/cryptography-engineering/`, `~/.cache/jet-luna/dx2/quant-trading/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/prim-exact-numerics/probe.md`, `gaps.json`, and the code under `~/.cache/jet-luna/dx3/prim-exact-numerics/pkg/`. Gap ids start with `prim-exact-numerics-G`.
