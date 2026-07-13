# Progressive Proof And Replay

Card: #240 / cbq06v8j. Decision anchor: D-WD12 = B. Status: ratified, not
implemented end to end.

## Goal

Turn Jet's existing correctness facts into one product surface: `jet prove`
over contracts, refinements, effects, budgets, property tests, replay facts, and
optional solver lenses. Proof is progressive: ordinary tests and contracts are
the beginner face; deeper proof engines stay opt-in and Jet-diagnostic-shaped.

## Beginner/Expert/Hybrid Pass

- Beginner: run `jet prove` and get a clear pass/fail report with failing test,
  contract, replay, or budget evidence. No solver vocabulary by default.
- Expert: select lenses, export JSON, inspect proof obligations, replay traces,
  effect paths, budget baselines, and solver diagnostics.
- Hybrid: one fact graph feeds tests, contracts, replay, budgets, dossier, and
  CI. No separate proof language before evidence demands it.

## Current Anchors

- D-WD12: `jet prove` becomes the progressive proof/replay product.
- D-REPLAY1: replay rejects non-mockable nondeterminism; unbuilt.
- D-REFINE1: refinement prover direction; unbuilt.
- D-PREPOST1, D-STATE1, D-EFF1, D-TEST1, D-COV1, D-WD14: existing fact sources.
- D-WD2/D-SEMINDEX1: dossier and semantic index own inspectable facts.
- D-GAME-REPLAY1: game replay artifacts are typed and deterministic.

## Normative law

The reconciled current contract is
[`../../spec/proof-replay-decisions.md`](../../spec/proof-replay-decisions.md).
It preserves the exact D-PROVE-REPLAY1, D-PROVE-SEM1, D-JPROOF1, D-JREPLAY1,
D-PROVE-SOLVER1, and D-PROVE-LENS1 law. Later specialized decisions override
the umbrella's provisional examples.

## Remaining implementation slices

1. Fact inventory: define `ProofFact` schema over tests, contracts, effects,
   refinements, typestate, coverage, replay, and budgets.
2. CLI surface: `jet prove <target>` renders the ratified human report and
   `--json` emits stable schema.
3. Beginner slice: run unit/property/doctests, contract checks, coverage summary,
   and replay determinism checks without solvers.
4. Replay slice: typed replay artifact reader/writer, deterministic capability
   injection, trace comparison, and panic replay.
5. Budget slice: consume D-WD14 budget reports as proof facts.
6. Expert lenses: implement D-PROVE-LENS1's presentation-only facets without
   changing execution, report identity, artifacts, result, or exits.
7. CI integration: stable exit codes, JSON schema snapshots, and dossier links.
8. Solver: implement D-PROVE-SOLVER1's std-only deterministic bounded
   Presburger engine, certificate checker, counterexample validation, and
   step/resource policy.

## Test Strategy

- CLI transcript tests for pass, failing contract, failing property, replay
  mismatch, effect violation, and budget failure.
- JSON schema snapshots for `ProofReport`.
- Replay deterministic transcript tests: same trace under dev and AOT.
- Integration with existing `tests/dev.rs` parity once replay facts can carry
  stdout/stderr/exit/panic.
- Solver lens tests use fixture solver output laundered into Jet diagnostics;
  no raw solver text reaches users.

## Ratified Surface

`D-PROVE-REPLAY1=A`: `jet prove` is the umbrella proof and replay command.

- `jet prove <target>` renders the beginner pass/fail report.
- `jet prove <target> --replay trace.jetproof-replay` replays a typed trace against the
  target.
- repeated `--lens all|refinements|effects|taint|contracts|tests|budgets|replay|solver`
  selects presentation-only views; the full producer run and complete machine
  report remain unchanged.
- `jet prove <target> --json` emits stable `.jetproof`-shaped data for CI.
- `.jetproof-replay` and `.jetproof` use the exact typed/versioned/security/migration law
  linked above; raw solver/runtime text never reaches users.
