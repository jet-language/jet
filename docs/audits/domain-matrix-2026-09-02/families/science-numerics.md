# Science and numerics family synthesis

## Verdict
Science and numerics has a strong typed substrate but lacks the domain mechanisms that make scientific work productive: generic algebra, stable and sparse solvers, fast transforms, equation/model systems, probability, symbolic values, distributed/GPU execution, standard formats, and domain inspection. Jet's strongest differentiator is one fail-closed, cross-tier evidence ledger; the family must not claim performance until each registry workload has a matched gauntlet cell.

## Mechanisms
| id | mechanism | domains | gate kind | Jet state | e14 overlap |
|---|---|---:|---|---|---|
| M-LINALG | Generic typed tensor and linear-algebra substrate | 10 | public-api | mixed | none |
| M-DENSE-FACTORS | Dense factorizations, eigenvalues, and SVD | 5 | public-api | real-gap | none |
| M-SPARSE-SOLVERS | Sparse assembly, Krylov methods, and AMG/preconditioners | 6 | public-api | mixed | none |
| M-FFT | Planner-backed FFT family | 5 | public-api | real-gap | none |
| M-ODE-DAE | Adaptive ODE/DAE integration and event results | 3 | public-api | real-gap | none |
| M-OPTIMIZATION | Optimization model surface and LP/MILP/NLP/CP-SAT backends | 7 | syntax | mixed | none |
| M-UNITS | Typed physical units and dimensional analysis | 4 | syntax | mixed | none |
| M-DISTRIBUTIONS | Probability distributions, samplers, and Bayesian inference | 5 | public-api | mixed | none |
| M-SYMBOLIC | Typed symbolic AST, assumptions, equations, and rewrite calculus | 6 | syntax | real-gap | none |
| M-TYPED-MODEL-RESULT | Typed model, fit, and result records | 8 | public-api | real-gap | none |
| M-DATA-TABLES | Typed tabular, relational, and columnar data boundary | 8 | public-api | mixed | D-DX-DEVTOOLS-UX1; e14-m10-data-notebooks |
| M-MPI-LAUNCH | Distributed execution, MPI collectives, and allocation-aware job launch | 9 | command | mixed | D-DX-JOBS-UX1 |
| M-GPU-KERNELS | GPU kernels, fusion, placement, and backend receipts | 7 | public-api | mixed | none |
| M-NOTEBOOK | Notebook/REPL evaluation and reactive inspection | 10 | ui | mixed | D-DX-LIVE1; D-DX-DEVTOOLS-UX1; e14-m10-data-notebooks |
| M-PLOTTING | Deterministic plotting, visualization, and publication export | 9 | ui | mixed | D-DX-DEVTOOLS-UX1; e14-m10-data-notebooks |
| M-SCI-FORMATS | Standard scientific formats and interchange | 12 | stdlib-dependency | mixed | none |
| M-VALIDATION-RECEIPTS | Cross-tier validation, reproducibility, and evidence receipts | 12 | invariant | mixed | D-DX-LIVE1; e14-m12-proof |
| M-MESH-FEM | Meshes, finite-element spaces, and geometric discretization | 2 | public-api | real-gap | none |
| M-AUTODIFF | Automatic differentiation and derivative propagation | 8 | public-api | mixed | none |
| M-EXACT-NUMERIC | Exact and arbitrary-precision numeric domains | 4 | none | mixed | none |
| M-SIM-EVENTS | Deterministic simulation events, lifecycle, and resource queues | 4 | public-api | real-gap | D-DX-LIVE1 |

The broadest candidates are M-SCI-FORMATS, M-VALIDATION-RECEIPTS, M-LINALG, M-NOTEBOOK, M-MPI-LAUNCH. All 21 candidates are new because _mechanisms.json was absent at synthesis start.

## Domains
| domain | persona | incumbent | P0 count | performance incumbent | gauntlet coverage | biggest beat vector |
|---|---|---|---:|---|---|---|
| numerical-computing | Computational scientist, engineer, analyst, or student trained in MATLAB, Python, Julia, or Fortran who starts with a live array session. | Julia LinearAlgebra / Fortran BLAS-LAPACK | 15 | Julia LinearAlgebra and Fortran BLAS/LAPACK for dense algebra; NumPy/SciPy with vendor BLAS and FFTW for arrays, FFT, ODE, and optimization. | numerics.float-kernel (nbody proxy), numerics.script, and numerics.notebook; no dedicated FFT, ODE, dense-linalg, or optimization cell | One typed source-to-result ledger with shape, effects, authority, placement, and provenance (foundation shipped; numerical APIs incomplete). |
| symbolic-math | Mathematician, scientific programmer, engineer, or researcher who alternates Mathematica notebooks, SymPy, Julia Symbolics, and Maxima when formulas must remain inspectable. | Mathematica + FLINT + Symbolics.jl | 22 | Mathematica kernel plus FLINT/Arb primitives and Symbolics.jl/SymbolicUtils | none | Exact-by-default scientific numerics with explicit Float boundary (shipped, unbenchmarked). |
| statistics | Statistician or data scientist trained in R, Python, Julia, or Stata who starts with tidy tables, formulas, and model summaries. | R data.table + R stats/lme4/brms | 23 | R data.table plus R stats/lme4/brms; Julia GLM/MixedModels are the second implementation family. | none (existing datasummary covers descriptive numerics only and is marked perf=false) | One typed data/model ledger with schema facts, bounded plans, and narrow diagnostics. |
| optimization-or | Operations researcher, scheduler, allocator, or engineer who starts with JuMP/HiGHS, Gurobi, OR-Tools, Pyomo/CVXPY, or MiniZinc. | Gurobi / HiGHS / OR-Tools CP-SAT | 16 | Gurobi, HiGHS, and OR-Tools CP-SAT | none — current gauntlet has no LP/MILP/VRP optimization cell (22 entries, 25 matrix cells) | One executable Prelude across tiers (shipped law; optimizer absent). |
| simulation-modeling | Simulation engineer or researcher trained in Modelica, MATLAB/Simulink, Python, or Julia who needs equations, events, agents, queues, and repeatable experiments. | Dymola Modelica / ModelingToolkit.jl | 32 | Dymola Modelica compiler/runtime, with ModelingToolkit.jl as the strongest open symbolic-numeric comparator | numerics.float-kernel (nbody exists; no 1e4-equation DAE or 1e6-agent ABM cell) | One typed model-to-result ledger with source, state, solver, and evidence (unbuilt). |
| hpc-parallel | HPC developer or computational scientist trained in C/C++/Fortran, MPI/OpenMP, Chapel, or Julia who targets cluster stencil/N-body workloads. | MPI+OpenMP C++ BookLeaf | 20 | MPI+OpenMP C++ reference BookLeaf on the ARCHER Cray XC30, with Kokkos and RAJA ports as the portability challengers | numerics.float-kernel | Ownership-safe local parallelism rejects unsafe captures before codegen. |
| gpu-compute | GPU kernel author or ML/scientific programmer trained in CUDA, Triton, JAX, CUDA.jl, OpenCL/SYCL, or wgpu. | CUDA/cuBLAS/cuDNN | 28 | CUDA hand-tuned kernels with cuBLAS/cuDNN on NVIDIA GPUs; Triton is the custom-kernel challenger, CUDA.jl the single-language kernel ladder, JAX the transformed-array and sharding rail, and Vulkan/WebGPU the portability rails | missing: gpu.compute.gemm-reduction-scan-fusion; related existing cell numerics.float-kernel/nbody is CPU and must not substitute for a GPU result | One Tensor/operation meaning with CPU-oracle differential law across providers. |
| quantum-computing | Quantum researcher or algorithm engineer trained in Python/Qiskit/Cirq/PennyLane, Q#, or OpenQASM who starts with a Bell circuit and simulator. | qsim + cuStateVec | 20 | Google qsim state-vector simulator (qsimcirq/QSimSimulator) on CPU, with NVIDIA cuStateVec as the GPU incumbent | none | Memory-safe ownership and explicit device/precision/seed receipts can make quantum state handling auditable. |
| proof-formal | Formal methods engineer or mathematician trained in Lean, Coq/Rocq, Isabelle, Agda, TLA+, Dafny, or F* who needs source-linked proof state and trusted evidence. | Lean 4 + Mathlib/Lake | 19 | Lean 4 kernel with Mathlib and Lake incremental/olean artifacts; Isabelle/PIDE is the interactive-checking comparator and TLC is the finite-state-model comparator. | none | One evidence ledger combines checks, contracts, tests, budgets, replay, and solver evidence. |
| logic-constraint | Program-analysis engineer, scheduler, allocator, ASP researcher, or relational programmer who reaches for Souffle, SWI-Prolog, MiniZinc, clingo, or miniKanren. | Souffle compiled Datalog | 37 | Soufflé compiled Datalog with OpenMP | none — gauntlet/matrix.json has numerics and general workloads but no logic, Datalog, ASP, CLPFD, or CP-scheduling cell | Evidence-first correctness via bounded native Presburger certificates and source-local counterexamples. |
| probabilistic-programming | Statistician, data scientist, epidemiologist, economist, or ML researcher trained in Python/R/Julia/Stan who thinks in priors, likelihoods, posterior uncertainty, and predictive checks. | NumPyro NUTS / Stan NUTS | 18 | NumPyro NUTS (JAX/XLA) with Stan NUTS as the compiled CPU baseline | none | One ledger for model, execution, diagnostics, and provenance (unbuilt). |
| pde-fem | Simulation engineer or applied mathematician trained in continuum mechanics, numerical PDEs, or scientific computing who starts with FEniCSx, deal.II, MFEM, COMSOL, or FreeFEM. | deal.II / MFEM + PETSc | 28 | deal.II and MFEM (C++), with PETSc as the scalable solver substrate | none | Typed shape/effect/error identity can make fields, dimensions, backends, and failures one checked model. |

## Owner gates
| suggested decision id | topic | scope |
|---|---|---|
| D-SCI-LINALG1 | core.linalg, dtype, factors, FFT, BLAS/provider boundary | owner decision before domain cards |
| D-SCI-SOLVERS1 | ODE/DAE integration, sparse/Krylov/AMG, optimization classes | owner decision before domain cards |
| D-SCI-UNITS1 | typed physical quantities, dimensions, affine units, serialization | owner decision before domain cards |
| D-SCI-PROB1 | distribution inventory, model sites, samplers, posterior diagnostics | owner decision before domain cards |
| D-SCI-SYMBOLIC1 | symbolic AST, assumptions, rewrite/calculus/equation semantics | owner decision before domain cards |
| D-SCI-PARALLEL1 | MPI/Slurm/worker/job placement and communication evidence | owner decision before domain cards |
| D-SCI-GPU1 | safe kernels, fusion, vendor bridges, precision/fallback law | owner decision before domain cards |
| D-SCI-LIVE1 | domain notebook/REPL, reactive cells, plotting and inspector hosts | owner decision before domain cards |
| D-SCI-FORMATS1 | HDF5/netCDF/MAT/NPZ/MPS-LP/SBML/OpenQASM/XDMF scope | owner decision before domain cards |
| D-SCI-MESH1 | mesh/FE/geometry/provider ownership and PDE model boundary | owner decision before domain cards |
| D-SCI-DATA1 | typed table/relation/model input and missingness/schema law | owner decision before domain cards |
| D-SCI-PROOF1 | proof versus relation runtime scope and evidence contract | owner decision before domain cards |

These twelve family gates consolidate 481 owner-gated census rows. They are suggested decisions only; no Tower ballot or syntax design is made here.

## Contradictions between miners
| mechanism | source A | source B | synthesis treatment |
|---|---|---|---|
| Units and quantities | numerical-computing marks stock typed physical units as owner-gate and says no public Quantity family (numerical-computing/census.json, Typed physical units and dimensional analysis). | simulation-modeling and pde-fem mark physical dimensions/quantities already implemented or ratified (simulation-modeling/census.json, Physical units and dimensions; pde-fem/census.json, Physical dimensions, units, and quantity kinds). | Treat ratified dimension/quantity foundations as reusable, but keep stock numeric integration and serialization an owner gate. |
| Notebook depth | numerical-computing calls notebook/REPL support ratified-in-progress (numerical-computing/census.json, Executable notebook sections). | symbolic-math says no notebook evaluator/cell surface, while probabilistic-programming calls generic REPL/notebook entrypoints implemented but has no PPL kernel (symbolic-math/census.json, Notebook direct evaluation; probabilistic-programming/census.json, REPL and notebook entrypoints). | Separate generic host/entrypoint shipment from domain evaluator, reactive execution, and result-inspector depth. |
| FFT state | numerical-computing says core.compute.fft is implemented but a naive DFT and missing planner (numerical-computing/census.json, Planner-selected O(n log n) FFT). | pde-fem calls dense linear algebra and DFT already implemented (pde-fem/census.json, Dense linear algebra and DFT). | Count rank-1 DFT as baseline only; planner, inverse/real/ND/complex behavior remains the shared M-FFT gap. |
| Solver meaning | numerical-computing and logic-constraint describe core.compute.solve and solve.Solver as a finite Bool checker (numerical-computing/census.json, Finite deterministic constraint-checking solver; logic-constraint/census.json, Finite Bool-check Solver handle). | simulation-modeling and PDE rows require ODE/DAE, event, sparse, or FEM solvers and report those absent (simulation-modeling/census.json, ODEProblem and solver ecosystem bridge; pde-fem/census.json, Direct/iterative sparse solvers). | Keep the checker as a narrow invariant; do not let its name satisfy numerical, optimization, or PDE solver rows. |
| Proof reachability | proof-formal says ProofReport/certificate producers exist in source and design but live jet prove exits at E2105 (proof-formal/report.md:118-120). | logic-constraint says solver lens can emit certificates or E2950 counterexamples but its live probe also returned E2105 (logic-constraint/report.md:143-145). | Record E2105 as a defect and leave proof-lens runtime claims unverified until identity hashing is repaired. |
| Data/statistics boundary | statistics has typed core.data descriptive stats but its model probe imports missing core.stats and returns E1001 (statistics/claims.json, OLS/model gap). | numerical-computing marks deterministic core.data summaries already implemented while requiring model-aware stats separately (numerical-computing/census.json, Statistics, compensated reductions). | Treat core.data summaries as shipped substrate, not evidence of a statistical model namespace or GLM/mixed-model support. |

## Cross-reference notes
Every P0 census row is copied into domains.json with its source feature name, needed behavior, and a mechanism id where it belongs to a general shared candidate; domain-only or intentionally rejected rows retain null. defects.json records the ratified core.linalg import failure and each observed E2105 prove probe. Standard formats include HDF5, netCDF, MAT, NPZ/NPY, MPS/LP, SBML/CellML, OpenQASM, XDMF, VTK, and .facts; these are grouped under M-SCI-FORMATS rather than hidden in domain cards.
