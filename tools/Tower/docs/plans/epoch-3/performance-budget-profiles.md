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
  `jet dossier`; there is one budget model, not separate bench/build policies.

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
7. Dossier/prove integration: `jet dossier budget` and `jet prove` include
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

## Ballots To Queue

### D-PERFBUDGET-SURFACE1 - Budget declaration surface

Group: tooling.

Gist: choose where performance budgets are declared.

Story: Mei owns an HTTP service and a small game. She wants service latency,
startup, binary size, and frame budgets checked in CI without teaching every
new contributor benchmark internals.

In wild:

```jet
module perf.server {
    budgets: {
        startup: 20ms
        alloc_per_request: 0
        p99_latency: 5ms
        binary_size: 3MiB
    }
}
```

Options:

- A: Role modules such as `module perf.server { budgets: ... }`, with package,
  env, service, and scene attachments resolving to the same typed budget facts.
  Recommended.

```jet
module perf.server {
    budgets: {
        startup: 20ms
        p99_latency: 5ms
        binary_size: 3MiB
    }
}
```

- B: `pkg.jet` target fields only. Clear for packaged builds, but one-file and
  scene/service budgets need extra ceremony.

```jet
targets: {
    server: executable {
        entry: "src/server.jet"
        budgets: { startup: 20ms, p99_latency: 5ms }
    }
}
```

- C: `@Budget(...)` markers on functions. Local and visible, but poor for
  package/service budgets and statistical baselines.

```jet
@Budget(p99_latency: 5ms)
fn handle(req: Request) -> Response { ... }
```

- D: external policy file. Easy to generate, but creates split-brain state.

```text
perf-budget.jetpolicy
server.startup = 20ms
server.p99_latency = 5ms
```

Comparisons:

- Lighthouse and web tooling use budget config files.
- Go/Rust rely mostly on benchmarks and external CI policy.
- Game engines expose frame/memory budgets in project settings.

Rec: A. Role modules preserve source review, work for beginner defaults once a
budget exists, and still give experts exact typed facts.

### D-PERFBUDGET-BASELINE1 - Statistical baseline policy

Group: tooling.

Gist: choose how statistical budgets avoid flaky CI.

Story: Mei's p99 latency shifts by machine. CI should catch regressions without
failing every time a runner has background load.

In wild:

```text
jet budget update --baseline ci/linux-x64
jet bench --budget ci/linux-x64
```

Options:

- A: Pinned baseline artifact with hardware/toolchain identity, trend window,
  confidence policy, and explicit update command. Recommended.

```text
jet budget update --baseline ci/linux-x64
jet bench --budget ci/linux-x64
```

- B: Absolute thresholds only. Simple, but noisy for latency and throughput.

```jet
module perf.server {
    budgets: { p99_latency: 5ms }
}
```

- C: Advisory-only statistical budgets. Avoids flakes, but weakens CI value.

```text
jet bench --budget ci/linux-x64 --warn-only
warning: p99_latency above trend budget
```

Comparisons:

- Criterion stores baselines and compares distributions.
- Web performance tooling distinguishes budgets from lab variance.

Rec: A. Deterministic budgets remain hard gates; statistical budgets become
evidence-bound trend checks instead of raw one-run thresholds.
