# Probe prim-dsl — Embedded languages in a library: symbolic math, rules, filters, formulas

Read `docs/research/domain-foundations/probes/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: data, backend, science.

## Build this

Build, as library Jet, four tiny embedded languages and use each from a 15-line program: a symbolic expression type with simplification and differentiation; a rules engine (facts, conditions, actions, declaration-order evaluation); a jq-style structured filter over JSON; and a spreadsheet formula evaluator with a dependency graph and incremental recompute. Use examples/features/patterns/**, traits/**, generics/**, comptime/**, parsing/**, reflection/**.

## Answer these

1. What does a DSL author lack: pattern matching depth, operator overloading, comptime evaluation, reflection over types, custom literals, macros (none exist by design: say what replaces them)?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/symbolic-math/`, `~/.cache/jet-luna/dx2/business-rules-workflows/`, `~/.cache/jet-luna/dx2/logic-constraint/`, `~/.cache/jet-luna/dx2/spreadsheets-business-logic/`, `~/.cache/jet-luna/dx2/text-processing-parsing/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/prim-dsl/probe.md`, `gaps.json`, and the code under `~/.cache/jet-luna/dx3/prim-dsl/pkg/`. Gap ids start with `prim-dsl-G`.
