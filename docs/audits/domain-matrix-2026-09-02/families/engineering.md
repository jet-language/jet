# Engineering family synthesis

## Family verdict

Engineering tools win when one case preserves physical meaning from authoring through numerical execution, hardware or native boundaries, visualization, and a reproducible result. The 14-domain census shows that Jet already has strong shared substrate—typed dimensions, ranked tensors and linear algebra, bounded streams and codecs, deterministic plots, explicit placement and authority, safe parallel foundations, and cross-tier evidence—but almost every engineering domain layer is a real gap. The best first moves are shared mechanisms, not fourteen disconnected mini-languages: typed units, spectral/DSP kernels, sparse and ODE/DAE solvers, mesh/stencil and BRep boundaries, hardware I/O, standard formats, and evidence-bearing streams. Every performance `gauntlet_cell` is currently `none`; published incumbent numbers are measurement leads, not Jet wins.

The source file `~/.cache/jet-luna/dx2/_mechanisms.json` was absent, and the prior science-numerics mechanism file was also absent. Therefore every mechanism below has `existing_mechanism_id: null`; the canonical M-* identifiers are new family-level names. `M-DISTRIBUTIONS` and `M-SYMBOLIC` are not emitted because no engineering P0 census rows justify them; they remain available for a later family only if that family's census proves the mechanism.

## Mechanisms

| ID | Mechanism | Domains | Gate kind | Jet state |
|---|---|---:|---|---|
| `M-LINALG` | Typed dense tensors and linear algebra | 13 | none | mixed |
| `M-FFT` | FFT and inverse spectral transforms | 7 | public-api | mixed |
| `M-ODE-DAE` | ODE and DAE integration with events | 5 | public-api | real-gap |
| `M-SOLVER-SUBSTRATE` | Checked nonlinear and domain solver substrate | 11 | public-api | mixed |
| `M-SPARSE-SOLVERS` | Sparse assembly, direct factorization, and Krylov solves | 10 | public-api | mixed |
| `M-OPTIMIZATION` | Constrained optimization and design studies | 10 | public-api | real-gap |
| `M-UNITS` | Typed dimensional and affine quantities | 13 | invariant | mixed |
| `M-MPI-LAUNCH` | MPI launch and distributed domain execution | 13 | stdlib-dependency | real-gap |
| `M-GPU-KERNELS` | Explicit GPU kernels and placement receipts | 11 | public-api | mixed |
| `M-NOTEBOOK` | Notebook and interactive REPL loop | 9 | none | mixed |
| `M-PLOTTING` | Deterministic and interactive engineering visualization | 14 | ui | mixed |
| `M-SCI-FORMATS` | Scientific and engineering file formats | 14 | public-api | mixed |
| `M-DSP-FILTERS` | Typed filter design and multirate DSP | 8 | public-api | real-gap |
| `M-MESH-STENCILS` | Meshes, stencils, and discretized field operators | 5 | public-api | real-gap |
| `M-CONTROL-MODELS` | State-space, transfer, and typed model graphs | 3 | public-api | real-gap |
| `M-BREP-GEOMETRY` | Exact BRep topology and geometry kernels | 4 | stdlib-dependency | real-gap |
| `M-HARDWARE-IO` | Authority-safe hardware and instrument I/O | 10 | public-api | real-gap |
| `M-DATA-TABLES` | Typed tables, records, and result schemas | 8 | none | mixed |
| `M-WORKFLOW-DAG` | Inspectable workflow graphs and execution plans | 14 | public-api | mixed |
| `M-SIMULATION` | Reproducible domain simulation lifecycle | 12 | public-api | mixed |
| `M-EVIDENCE` | Inspection, diagnostics, and reproducible evidence | 4 | none | mixed |
| `M-NETWORKING` | Typed network and service protocol boundaries | 6 | none | mixed |
| `M-PACKAGING` | Package, dependency, and native-provider boundary | 11 | stdlib-dependency | mixed |
| `M-STORAGE` | Durable scientific stores and caches | 5 | stdlib-dependency | mixed |

Each mechanism's `p0_rows` in `mechanisms.json` quotes the exact census feature and need, normalizing the one `RF/wireless` census spelling to registry id `rf-wireless-comms`. Domain-only chemical property/flowsheet mechanisms remain in the domain inventory and are not promoted to shared mechanisms.

## Domain matrix

| Domain | Benchmark peer | P0 rows | Gauntlet coverage | Biggest beat vector |
|---|---|---:|---|---|
| `signal-processing` | FFTW 3 (published single-core AMD Ryzen 7 1800X benchmark; MATLAB/SciPy/GNU Radio remain ecosystem comparators) | 22 | none | One typed numeric/evidence path can carry signal values, limits, precision, and rendered evidence. |
| `acoustics-audio-engineering` | ODEON for room-acoustic ray tracing and auralization; MATLAB Audio Toolbox and Max/MSP for measurement and live DSP; pyroomacoustics and Faust for scriptable/compiled signal processing; COMSOL Acoustics for FEM/FDTD-class field simulation | 21 | none | Typed contracts can name the device, frame, geometry, and metric field in every failure. |
| `control-systems` | ControlSystems.jl 1.x numerical path, with MATLAB/Simulink Coder as the deployment incumbent | 25 | none | Typed physical and numerical contracts can carry time base, labels, delays, and dimensions instead of parallel metadata. |
| `rf-wireless-comms` | srsRAN Project C++ PHY/L1 stack with FFTW3f-backed GNU Radio-style signal kernels; MATLAB 5G Toolbox is the reference-quality API incumbent for waveform/LDPC/channel-estimation behavior | 25 | none | One authority receipt can cover CPU, GPU, and radio boundaries without silent device fallback. |
| `electromagnetics-antenna` | openEMS C++ EC-FDTD engine, with Meep C++ FDTD as the independent open-source comparator | 24 | none | Typed physical correctness can reject dimension, shape, and complex-value mistakes before solving. |
| `mechanical-cad-cae` | Open Cascade Technology (OCCT) 8.0.1 BRep/Boolean kernel, as used by FreeCAD, CadQuery, and build123d; pair it with the topology-first BRep mesher described in arXiv:2604.02141 for mesh generation. | 29 | none | One typed model ledger can join source, units, geometry facts, diagnostics, and exports. |
| `fea-structural` | Abaqus/Standard and MSC/Autodesk Nastran structural solvers, with Abaqus sparse direct workflows and Nastran SOL 101/SOL 103/SOL 106 as the named commercial reference family. | 29 | none | One typed physical-and-numerical ledger can join dimensions, shapes, mesh, solver residuals, and field provenance. |
| `cfd` | OpenFOAM and SU2 | 32 | none | Typed setup and evidence can explain every unit, default, transfer, output, and convergence result before a long run. |
| `electrical-eda` | Xyce | 23 | none (no circuit, SPICE, Xyce, PCB, or EDA cell in tools/perf/corpus.tsv or the gauntlet manifest) | A typed physical ledger can reject voltage, current, and dimension mistakes before circuit or board work. |
| `chemical-process` | Cantera 3.2.0 native C++ kinetics/thermo stack (`Solution`/`Kinetics`), with CoolProp as the property-call cross-check | 23 | none | Typed units before code generation can reject balance mistakes, but unit interpolation must first stop emitting ICE warnings. |
| `power-energy-systems` | PSS/E transmission AC power flow and OpenDSS distribution power flow | 23 | none | Compiler-known dimensions can reject incompatible electrical quantities with no runtime dimension representation. |
| `civil-structural-bim` | IfcOpenShell C++ geometry iterator (primary interchange/geometry incumbent) and ETABS native CSI solver/API (structural-analysis incumbent) | 28 | none | A source-backed graph projection can keep checked source and visual nodes from drifting. |
| `automotive-aerospace-systems` | JSBSim | 27 | none | One typed semantic ledger can keep notebook, model graph, runtime, generated code, and tests aligned. |
| `instrumentation-lab-automation` | NI M Series/X Series hardware-timed acquisition with the native NI-DAQmx C driver path and NI real-time buffering | 46 | none | One typed data and error kernel can give acquisition, control, plot, and report paths one inspectable foundation. |

The performance objects in `domains.json` preserve each miner's incumbent, workload, dataset, published numbers, causal speed explanation, and required Jet win proof. None of the 14 domains has a registered gauntlet cell. In particular, FFTW's published 18,018/16,748/16,664 Mflop/s rows, ControlSystems.jl's 4.351 microsecond transfer response, Xyce's 27x and 19x large-circuit speedups, Power Grid Model's 24x–723x graph values, and JSBSim's 250x real-time report are not same-run Jet comparisons.

## Cross-domain owner gates

| Suggested decision | Gate | Affected mechanisms/domains |
|---|---|---|
| `D-ENG-UNITS1` | Ratify dimensional, affine Point/Delta, formatting, conversion provenance, and warning-free quantity behavior. | `M-UNITS`; all domains, with ICE evidence in FEA and chemical probes |
| `D-ENG-NUMERIC1` | Choose public FFT/DSP, sparse, ODE/DAE, optimization, precision, convergence, and failure-result boundaries. | `M-FFT`, `M-DSP-FILTERS`, `M-SPARSE-SOLVERS`, `M-ODE-DAE`, `M-OPTIMIZATION`; signal, control, RF, EM, FEA, CFD, electrical, chemical, power, automotive |
| `D-ENG-GEOMETRY1` | Choose BRep, mesh/stencil, physical-group, tolerance, refinement, healing, and legal native dependency boundaries. | `M-BREP-GEOMETRY`, `M-MESH-STENCILS`; EM, CAD, FEA, CFD, civil, acoustics |
| `D-ENG-IO1` | Choose authority-safe hardware sessions, device streams, clocks, buffers, callbacks, triggers, and missed-sample semantics. | `M-HARDWARE-IO`, `M-REALTIME-STREAMS`, `M-NETWORKING`; acoustics, RF, control, electrical, chemical, automotive, instrumentation |
| `D-ENG-FORMATS1` | Choose first-party versus audited bridges and scope for STEP, IFC, WAV, SigMF, CGNS/VTK, Gerber, MATPOWER, FMU, and related formats. | `M-SCI-FORMATS`; all domains |
| `D-ENG-EXEC1` | Choose explicit MPI/GPU/provider and package boundaries, with no silent fallback and auditable receipts. | `M-MPI-LAUNCH`, `M-GPU-KERNELS`, `M-PACKAGING`, `M-EVIDENCE`; all high-performance domains |
| `D-ENG-DOMAIN1` | Choose which domain models are stock Core, packages, or external authorities, and preserve one typed case-to-result ledger. | `M-CONTROL-MODELS`, `M-CIRCUIT-SOLVE`, `M-SIMULATION`, `M-WORKFLOW-DAG`; all 14 domains |

## Contradictions and defects

| Mechanism | Source 1 | Source 2 | Synthesis |
|---|---|---|---|
| FFT | Signal census marks `FFT primitive with complex spectrum` already implemented and its probe returns `8` (`signal-processing/census.json`, rows 1–15). | The same report and RF report identify a rank-1 real, CPU-only naive DFT and explicitly reject an FFTW performance claim (`signal-processing/report.md` §Performance; `rf-wireless-comms/report.md` §Avoid list). | Count correctness at the primitive level only; `M-FFT` remains mixed and needs inverse, streaming, production kernels, and a gauntlet. |
| Units | Jet reports compiler-known dimensions and affine quantities as a shipped safety foundation (`chemical-process/report.md` §Beat vectors; `power-energy-systems/report.md` §Beat vectors). | FEA and chemical positive probes emit `internal compiler error: sema accepted unit formatting only for unit values` while exiting zero (`fea-structural/probes/units_positive.txt`; `chemical-process/probes/quantity_run_exact.txt`). | `M-UNITS` is mixed, not cleanly shipped; defects are recorded separately and block warning-free claims. |
| Performance | Incumbent files contain published FFTW, Xyce, PGM, and JSBSim numbers with causal mechanisms (`*/performance.json`). | Every engineering performance record reports `gauntlet_cell: none`; several values are explicitly product claims, graph values, illustrative output, or unmeasured (`cfd`, `rf-wireless-comms`, `power-energy-systems`, `instrumentation-lab-automation` performance files). | Preserve numbers as leads; no engineering Jet win is established. |
| Generic substrate versus domain support | Jet code and probes prove Tensor, units, plots, codecs, authority, and placement (`DESIGN.md` current state; domain claims ledgers). | CAD, FEA, CFD, EDA, chemistry, power, BIM, and instrumentation reports each warn that generic vectors, units, solve, FFI, or plots do not provide their domain model. | Reuse substrate, but keep domain mechanisms and owner gates explicit. |

No additional wrong diagnostic was found in the finished engineering probes: E1001, E0102, E0359, and E0405 captures are expected negative-path diagnostics, while the two unit-formatting ICE captures above are the actionable compiler defects.
