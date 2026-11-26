# Jet performance budget contracts

This document states the durable contracts for Jet's performance budgets,
measurement evidence, and budget diagnostics (`E2903`–`E2908`). Exact schemas
and implementations live in `Compiler/`, `Tools/Perf/`, and `Tools/Gauntlet/`;
full ballot history lives in Tower. Use the [Jet vocabulary](vocabulary.md) for
domain terms. (Full historical prose lives in git history:
`git show 20be9d938a5:Docs/Spec/performance-budget-decisions.md`.)

## Performance gate and declaration surface

- **Gate policy:** See `AGENTS.md` (Performance standard): `Jet/peer < 1.00`
  against every peer (including Rust) on wall time and peak memory per cell and
  metric; `Jet/ceiling <= 1.00` vs. `Tools/Perf/AsmParity` when ceiling gating
  opens; cold compile speed target 30M LOC/s with compile caches off by default.
- **One declaration surface:** Performance budgets use one typed declaration
  model (`BudgetSpec`) specifying metric, provider, unit, direction, limit,
  applicability (scope/profile/target), and severity (`Warn` or `Fail`).
- **Disjoint effective keys:** Duplicate or overlapping effective keys within
  intersecting applicability are rejected before measurement (`E2904`).

## Evidence, baselines, and cost transparency

- **Canonical wire objects:** Measurements, baselines, and verdict reports use
  versioned canonical JSON records with explicit statistical bounds, estimator,
  confidence, and baseline IDs.
- **Immutable receipts:** Historical receipts stay immutable under the policy
  they were recorded with and never weaken the active gate. Missing, mismatched,
  zero, stale, or inconclusive evidence never passes silently.
- **No forced bypass:** Corrupted artifacts, provider failures, CAS/permission
  refusals, or disallowed baseline updates fail cleanly (`E2908`) with no
  `--force` override.
- **Cost transparency:** Budget checks measure cold/real workloads honestly and
  report provider/measurement overhead alongside results.

## Exit codes and diagnostics

- **Exit law:** `0` pass (or `Warn`-only); `1` budget failure, invalid spec,
  missing evidence, or operation error (distinguished in `--json` via
  `failure_kind`); `2` CLI usage error; `101` ICE only. Runtime panic (`70`) is
  never produced by budget tooling.
- **Diagnostic codes** (exact What/Why/Fix copy lives in
  `Compiler/Prelude/Diagnostics.jet` and `Tests/UI/perf_budget_*`):
  - `E2903` — invalid budget declaration, unit, direction, or applicability.
  - `E2904` — overlapping effective budget keys.
  - `E2905` — unresolved scope, profile, target, or provider.
  - `E2906` — missing, mismatched, zero, stale, or unavailable evidence.
  - `E2907` — regression or inconclusive evidence (severity follows `Warn`/`Fail`).
  - `E2908` — operation failure (`performance budget operation failed`: corrupt
    artifact, provider/CAS/permission/write refusal).

## Provider and precedence boundary

- Providers supply raw measurements only; comparison, statistical verdicts, and
  gate policy live in one shared evaluator. Precedence follows `AGENTS.md`:
  explicit owner/CLI selection overrides workspace defaults, never weakening
  required gate invariants.
