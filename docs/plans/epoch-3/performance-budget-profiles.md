# Performance Budget Profiles

Card: #241 / ccq07pav. Decision anchor: D-WD14 = B.

## Goal

Make performance expectations typed, checkable product facts. Budgets attach to
packages, envs, services, scenes, and CI. Deterministic budgets are hard gates;
statistical budgets use pinned baselines and trend policy.

## Beginner/Expert/Hybrid Pass

- Beginner: no budget required. When a project declares one, failures explain
  the specific exceeded budget and the smallest fix path.
- Expert: exact metrics, baselines, hardware identity, warmup/trial policy,
  variance, artifact provenance, and CI trend rules are inspectable and JSON
  exportable.
- Hybrid: budgets feed `jet build`, `jet bench`, `jet dev`, `jet prove`, and
  `jet inspect dossier`; there is one budget model, not separate bench/build policies.

## Current Anchors

- D-WD14: typed budgets tied to build, bench, dev, dossier, and CI.
- D-BENCH1: `jet bench` and `#Bench` exist.
- D-EFFBUDGET1: package effect budgets prove manifest-level budget precedent.
- D-GAME-BUDGET1: scene budgets use the same model for games.
- D-WD2/D-WD12: dossier/prove consume budget facts.

## Budget Kinds

- Deterministic: binary size, generated unsafe count, allocation count in
  no-alloc regions, public API size, dependency/effect ceilings, compile output
  artifact size.
- Statistical: latency percentiles, throughput, startup time, frame time, memory
  high-water, benchmark ns/iter, service readiness.
- Context: target triple, profile, hardware baseline, OS, toolchain, cache state,
  warmup/trials, confidence, and trend window.

## Implementation Slices

1. Fact schema: `BudgetSpec`, `BudgetMeasurement`, `BudgetReport`, and stable
   JSON.
2. Surface after ballot: one declaration path for package/env/service/scene
   budgets.
3. Deterministic gate slice: binary size, generated unsafe count, and allocation
   budget over existing facts.
4. Bench integration: named `#Bench` results map to statistical budgets.
5. Dev integration: `jet dev` reports budget drift without changing semantics.
6. CI baseline slice: checked-in baseline artifact with update command and
   hardware identity.
7. Dossier/prove integration: `jet inspect dossier budget` and `jet prove` include
   budget facts.
8. Diagnostics: budget failures get registered codes, what/why/fix text, and
   snapshots.

## Test Strategy

- Unit tests for deterministic budget evaluation.
- CLI transcript tests for pass/fail/update-baseline.
- JSON snapshots for budget report schema.
- Bench fixture with stable fake clock/measurement provider for statistical
  budget tests.
- Dossier/prove snapshots showing budget facts consumed from the same report.
- No-regression: projects without budgets compile/run/test unchanged.

## Ratified Surface

`D-PERFBUDGET-SURFACE1=A`: budget declarations live in role modules, for example
`module perf.server { budgets: ... }`. Package, env, service, and scene budgets
all resolve to the same typed budget facts.

`D-PERFBUDGET-BASELINE1=A`: statistical budgets use pinned baseline artifacts
with hardware/toolchain identity, trend window, confidence policy, and an
explicit update command.

Accepted command shape for the implementation plan:

```text
jet budget update --baseline ci/linux-x64
jet bench --budget ci/linux-x64
```

Deterministic budgets remain hard gates. Statistical budgets compare against
evidence-bound baselines so CI catches real regressions without raw one-run
threshold flake.
