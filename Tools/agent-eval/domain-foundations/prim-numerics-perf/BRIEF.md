# Probe prim-numerics-perf — Numeric kernels from library code

Read `$JET_STATE_ROOT/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: data, ai-ml, games, science.

## Build this

Write, as plain library Jet (no core changes): a radix-2 FFT (complex, 4096 and 65536 points), a dense 512x512 matmul, a sparse CSR matrix-vector product (1e6 nonzeros), and a 5-point stencil over a 1024x1024 grid. Time each at `jet run` default tier and `jet build` release, and time numpy/scipy (or Rust if present) on the same inputs. Use Examples/LowLevel/SimdWide.jet, LinalgSimd.jet, layout_columnar.jet, Examples/Tooling/Performance/**, and the deleted ballots D-M-SIGNAL1, D-M-LINALG1, D-M-SOLVERS1.

## Answer these

1. Where does library Jet fall outside the speed band, and what primitive closes it (explicit SIMD types, layout control, allocation-free loops, fusion, in-place views)?
2. Which of these are already shipped but undiscoverable?

## Research to mine

Domain census and reports: `$JET_STATE_ROOT/dx2/signal-processing/`, `$JET_STATE_ROOT/dx2/numerical-computing/`, `$JET_STATE_ROOT/dx2/hpc-parallel/`, `$JET_STATE_ROOT/dx2/pde-fem/`, `$JET_STATE_ROOT/dx2/cfd/` (report.md, census.json, claims.json). Deleted ballots with worked code: `$JET_STATE_ROOT/dx2/ballots/` (grep the mechanism name). Family syntheses: `$JET_STATE_ROOT/dx2/_families/*/synthesis.md`.

## Output

`$JET_STATE_ROOT/dx3/prim-numerics-perf/probe.md`, `gaps.json`, and the code under `$JET_STATE_ROOT/dx3/prim-numerics-perf/pkg/`. Gap ids start with `prim-numerics-perf-G`.
