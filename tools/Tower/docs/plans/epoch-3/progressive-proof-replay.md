# Progressive Proof And Replay

Card: #240 / cbq06v8j. Decision anchor: D-WD12 = B.

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

## Implementation Slices

1. Fact inventory: define `ProofFact` schema over tests, contracts, effects,
   refinements, typestate, coverage, replay, and budgets.
2. CLI surface after ballot: `jet prove <target>` renders human report and
   `--json` emits stable schema.
3. Beginner slice: run unit/property/doctests, contract checks, coverage summary,
   and replay determinism checks without solvers.
4. Replay slice: typed replay artifact reader/writer, deterministic capability
   injection, trace comparison, and panic replay.
5. Budget slice: consume D-WD14 budget reports as proof facts.
6. Expert lenses: `--lens effects`, `--lens replay`, `--lens refinements`,
   `--lens budgets`, `--lens solver` once each producer exists.
7. CI integration: stable exit codes, JSON schema snapshots, and dossier links.
8. Solver bridge: only after a ballot for solver backend, timeout policy,
   proof-obligation shape, and diagnostics.

## Test Strategy

- CLI transcript tests for pass, failing contract, failing property, replay
  mismatch, effect violation, and budget failure.
- JSON schema snapshots for `ProofReport`.
- Replay deterministic transcript tests: same trace under dev and AOT.
- Integration with existing `tests/dev.rs` parity once replay facts can carry
  stdout/stderr/exit/panic.
- Solver lens tests use fixture solver output laundered into Jet diagnostics;
  no raw solver text reaches users.

## Ballot To Queue

### D-PROVE-REPLAY1 - Proof and replay surface

Group: static-guarantees.

Gist: choose the exact command and artifact shape for proof and replay.

Story: Omar receives a production panic trace for payment code. He wants one
command to replay the failure, check contracts and budgets, and hand CI a stable
JSON report.

In wild:

```text
jet prove src/payments.jet --replay traces/panic.jreplay
jet prove src/payments.jet --json > proof.json
```

Options:

- A: `jet prove` umbrella with `--replay`, `--lens`, `--json`, and typed
  `.jreplay`/`.jproof` artifacts. Recommended.

```text
jet prove src/payments.jet --replay traces/panic.jreplay
jet prove src/payments.jet --lens effects --json > payments.jproof
```

- B: Separate `jet prove` and `jet replay` top-level commands sharing artifacts.
  Clear verbs, but users must learn two entrypoints for one correctness report.

```text
jet replay traces/panic.jreplay --target src/payments.jet
jet prove src/payments.jet --from-replay traces/panic.jreplay
```

- C: Test-runner integration only: `jet test --prove --replay`. Familiar, but
  hides effects, budgets, and replay behind a testing-only mental model.

```text
jet test --prove --replay traces/panic.jreplay
jet test --prove --json > proof.json
```

- D: Dossier-only proof lens. Good for inspection, weak for CI action and
  beginner "prove this" workflow.

```text
jet dossier proof src/payments.jet --replay traces/panic.jreplay
jet dossier proof src/payments.jet --json > proof.json
```

Comparisons:

- Rust: tests plus type safety, no unified proof command.
- SPARK/Dafny: strong proof commands, heavier proof language.
- Property-test tools: practical confidence, usually separate from replay and
  effects.

Rec: A. One command keeps the beginner path direct while lenses/artifacts give
experts exact control over proof depth and replay data.
