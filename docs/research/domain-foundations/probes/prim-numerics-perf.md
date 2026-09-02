# Probe prim-numerics-perf — Numeric kernels from library code

Read `docs/research/domain-foundations/probes/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: data, ai-ml, games, science.

## Build this

Write, as plain library Jet (no core changes): a radix-2 FFT (complex, 4096 and 65536 points), a dense 512x512 matmul, a sparse CSR matrix-vector product (1e6 nonzeros), and a 5-point stencil over a 1024x1024 grid. Time each at `jet run` default tier and `jet build` release, and time numpy/scipy (or Rust if present) on the same inputs. Use examples/features/lowlevel/simd_*.jet, linalg_simd.jet, layout_columnar.jet, examples/features/performance and examples/performance/**, and the deleted ballots D-M-SIGNAL1, D-M-LINALG1, D-M-SOLVERS1.

## Answer these

1. Where does library Jet fall outside the speed band, and what primitive closes it (explicit SIMD types, layout control, allocation-free loops, fusion, in-place views)?
2. Which of these are already shipped but undiscoverable?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/signal-processing/`, `~/.cache/jet-luna/dx2/numerical-computing/`, `~/.cache/jet-luna/dx2/hpc-parallel/`, `~/.cache/jet-luna/dx2/pde-fem/`, `~/.cache/jet-luna/dx2/cfd/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/prim-numerics-perf/probe.md`, `gaps.json`, and the code under `~/.cache/jet-luna/dx3/prim-numerics-perf/pkg/`. Gap ids start with `prim-numerics-perf-G`.
