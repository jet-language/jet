# Adaptive Runtime

## Goal

Card #136 records adaptive runtime research for battery, network, load, carbon, and fidelity. Current owner law declines a full adaptive runtime as a standardized product surface for now. The active useful slice is the ratified fidelity signal: apps can read and override one quality/performance value under load.

This doc keeps the full runtime deferred and gives a narrow implementation path that does not invent automatic policy.

## Current law

- D-ADAPTRT1=C: full adaptive runtime is declined/deferred as niche; apps handle adaptation ad hoc or through platform APIs.
- D-ADAPTFID1=A: a library fidelity signal with read and manual override is ratified.
- D-CARBON1=C: carbon/battery policy folds into adaptive runtime later; no standalone carbon feature.
- I8 rejects a second invisible scheduling/policy model. Automatic runtime degradation would be a second mechanism unless separately ratified.
- Beginner defaults must not change program behavior behind the user's back.

Exact module/API names for the fidelity signal and any platform signal exposure are not settled here.

## Vertical slices

1. Fidelity value floor: a Core/runtime value in the range 0.0 through 1.0, readable by application code and manually overrideable for tests.
2. Deterministic testing hook: tests can pin fidelity so examples and CI are stable.
3. Game/data integration: `core.game` and data/plot examples can read fidelity explicitly to scale optional work, with no automatic disabling.
4. Platform signal research notes: battery, thermal, network, load, and carbon are cataloged as possible providers, but no provider ships without a new decision.
5. Policy boundary: docs and diagnostics make clear that Jet exposes facts; application code decides behavior unless a future ballot ratifies automatic policy.

## Acceptance tests

- Unit or transcript test: default fidelity is stable and in range.
- Override test: manual override changes reads in current process and can be reset.
- Determinism test: pinned fidelity makes an example output stable.
- UI snapshot: out-of-range override is rejected in Jet terms.
- No-policy test: runtime does not automatically skip user functions based on fidelity.

## Dependency order

1. Ratify exact API surface for the fidelity signal.
2. Implement read, override, reset, and range validation.
3. Add one explicit consumer in a domain that already needs it, likely `core.game` budgets or plotting quality.
4. Record platform-signal provider requirements as future ballots, not implementation.

## Owner ballots needed

- D-FIDELITY-API1: exact module path, type, read/override/reset names, and whether the value is process-local, task-local, or runtime-global.
- D-ADAPT-PROVIDER1: any future platform signal provider surface for battery, thermal, load, network, or carbon.
- D-ADAPT-POLICY1: any automatic degradation or scheduling policy; not allowed by current law.

## Adversarial tradeoffs

- Safety first: runtime must not silently skip code, delay work, or change resource cleanup because of load.
- Beginner experience: a simple app runs the same way unless it explicitly reads fidelity.
- Runtime performance: reading fidelity must be cheap enough for render/data hot paths, and manual override must not add hidden synchronization costs to programs that never use it.
- One mechanical path: fidelity is one explicit signal, not a parallel annotation system or scheduler.
- Ecosystem breadth: future platform providers can serve games, mobile, data jobs, and services, but only after their semantics are ratified.
