# Life sciences and health family synthesis

## Family verdict
Life-sciences and health work is a typed-record, physical-geometry, model-and-evidence problem. Incumbents win because domain values, standard formats, long-running workflows, and inspection receipts stay connected from first input to published result. Jet already supplies tensors, units, codecs, tables, notebooks, plots, authority, and proof foundations. The family still has six new domain mechanisms: genomic records, molecular graphs, volume images, FHIR/HL7 records, reaction networks, and agent simulation. Resumable sample-keyed workflow DAGs are reused from the engineering family.

## Mechanisms
| ID | Mechanism | Domain count | P0 rows | Gate kind | State | Reuse |
|---|---|---:|---:|---|---|---|
| M-SCI-FORMATS | Standard scientific formats and interchange | 5 | 21 | stdlib-dependency | mixed | reused |
| M-OPTIMIZATION | Optimization model surface and LP/MILP/NLP/CP-SAT backends | 4 | 14 | syntax | mixed | reused |
| M-GPU-KERNELS | GPU kernels, fusion, placement, and backend receipts | 4 | 7 | public-api | mixed | reused |
| M-LINALG | Generic typed tensor and linear-algebra substrate | 2 | 4 | public-api | mixed | reused |
| M-SPARSE-SOLVERS | Sparse assembly, Krylov methods, and AMG/preconditioners | 1 | 0 | public-api | mixed | reused |
| M-ODE-DAE | Adaptive ODE/DAE integration and event results | 1 | 5 | public-api | real-gap | reused |
| M-UNITS | Typed physical units and dimensional analysis | 3 | 3 | syntax | mixed | reused |
| M-DISTRIBUTIONS | Probability distributions, samplers, and Bayesian inference | 1 | 1 | public-api | mixed | reused |
| M-MPI-LAUNCH | Distributed execution, MPI collectives, and allocation-aware job launch | 2 | 2 | command | mixed | reused |
| M-NOTEBOOK | Notebook/REPL evaluation and reactive inspection | 3 | 2 | ui | mixed | reused |
| M-PLOTTING | Deterministic plotting, visualization, and publication export | 3 | 2 | ui | mixed | reused |
| M-GENOMIC-RECORDS | Genomic sequence, alignment, variant, and interval records | 1 | 16 | public-api | real-gap | new |
| M-MOLECULAR-GRAPH | Molecular graphs, stereochemistry, queries, and fingerprints | 1 | 16 | public-api | real-gap | new |
| M-VOLUME-IMAGE | Volumetric images with physical geometry and labels | 1 | 11 | public-api | real-gap | new |
| M-FHIR-HL7 | FHIR and HL7 clinical record models and transactions | 2 | 26 | public-api | real-gap | new |
| M-REACTION-NETWORK | Named biochemical reaction networks and rule models | 1 | 2 | public-api | real-gap | new |
| M-AGENT-SIM | Agent-based epidemic simulation and intervention studies | 1 | 19 | public-api | real-gap | new |
| M-WORKFLOW-DAG | Inspectable workflow graphs and execution plans | 3 | 8 | public-api | mixed | reused |
| M-FFT | Planner-backed FFT family | 1 | 2 | public-api | real-gap | reused |
| M-SYMBOLIC | Typed symbolic AST, assumptions, equations, and rewrite calculus | 1 | 1 | syntax | real-gap | reused |
| M-TYPED-MODEL-RESULT | Typed model, fit, and result records | 1 | 1 | public-api | real-gap | reused |
| M-DATA-TABLES | Typed tabular, relational, and columnar data boundary | 2 | 4 | public-api | mixed | reused |
| M-VALIDATION-RECEIPTS | Cross-tier validation, reproducibility, and evidence receipts | 3 | 2 | invariant | mixed | reused |
| M-SIM-EVENTS | Deterministic simulation events, lifecycle, and resource queues | 1 | 4 | public-api | real-gap | reused |

## Domains
| Domain | Persona | P0 count | Performance incumbent | Gauntlet | Biggest beat vector |
|---|---|---:|---|---|---|
| bioinformatics-genomics | Computational biologist or bioinformatics engineer | 24 | BWA-MEM2, minimap2, htslib, and bcftools | none | Typed workflow evidence |
| cheminformatics-compchem | Medicinal or computational scientist | 45 | GROMACS, ORCA/PySCF, and RDKit | none | Typed quantities and provenance |
| medical-imaging | Radiologist, imaging scientist, clinical engineer, or medical-ML engineer | 22 | MONAI Auto3DSeg, ITK/SimpleITK, and 3D Slicer | none | Physical coordinates as invariants |
| health-records-interop | FHIR server, SMART, hospital interface, or OpenMRS engineer | 25 | HAPI FHIR JPA on Java/PostgreSQL | none | Typed transport and record substrate |
| neuroscience | Neuroscientist, electrophysiologist, BCI engineer, or computational modeler | 29 | MNE-Python, NEURON, and Arbor | none | Recording metadata and units |
| epidemiology-public-health | Epidemiologist, public-health analyst, model engineer, or Bayesian statistician | 38 | Covasim with Numba and Stan NUTS | none | Typed outbreak state and evidence |
| systems-biology | Systems biologist or quantitative modeler | 20 | COPASI CopasiSE and Catalyst.jl | none | One network with deterministic and stochastic views |

## Owner gates
| Suggested decision | Scope | Domains | Owner choice |
|---|---|---|---|
| D-LSH-RECORDS1 | M-GENOMIC-RECORDS, M-MOLECULAR-GRAPH, M-VOLUME-IMAGE | bioinformatics, cheminformatics, medical imaging | Choose native value models, format loss, coordinate and geometry invariants, and package or bridge boundaries. |
| D-LSH-CLINICAL1 | M-FHIR-HL7 | health records, epidemiology | Choose supported releases, profiles, terminology, security, privacy, ACK/OperationOutcome, and audit semantics. |
| D-LSH-MODELS1 | M-REACTION-NETWORK, M-AGENT-SIM, M-ODE-DAE, M-OPTIMIZATION | systems biology, epidemiology, neuroscience, cheminformatics | Choose model ownership, solver/provider boundaries, seeds, convergence, extensions, and result contracts. |
| D-LSH-WORKFLOW1 | M-WORKFLOW-DAG | bioinformatics, medical imaging, epidemiology, systems biology | Choose resumability, keyed channels, cache identity, resource admission, external actions, and restart evidence. |
| D-LSH-COMPUTE1 | M-LINALG, M-SPARSE-SOLVERS, M-MPI-LAUNCH, M-GPU-KERNELS | compute-heavy domains | Set placement, authority, precision, sparse/parallel providers, determinism, and benchmark rules. GPU packages require allow: [GPU]. |
| D-LSH-FORMATS1 | M-SCI-FORMATS, M-DATA-TABLES | all domains | Choose standards coverage, unknown-field/loss policy, package dependencies, legal corpora, missingness, and cross-tier validation. |

The census contains 317 owner-gated rows, 227 P0 rows, and 203 P0 deficits after excluding already-implemented rows. Thirty P0 deficits remain domain-only and therefore have mechanism_id null in domains.json. The synthesis contains 24 candidates: six new family mechanisms and 18 canonical mechanisms reused from sibling family outputs.

## Contradictions and tensions
| Mechanism | Source A | Source B | Reconciliation |
|---|---|---|---|
| M-UNITS | Cheminformatics reports unit literals and provenance as implemented and says not to duplicate the unit plane (cheminformatics-compchem/report.md:50,93). | Neuroscience calls EEG/MEG channel units a real gap because channel calibration and sensor semantics are absent (neuroscience/report.md:42-43). | Reuse Jet quantities; add sensor and biological unit metadata without a second dimensional mechanism. |
| M-SCI-FORMATS | Health reports JSON/XML/HTTP/SQL as implemented substrate while FHIR remains a real gap (health-records-interop/report.md:47-61). | Bioinformatics reports generic JSON/CSV/streaming as implemented while FASTQ/SAM/BAM/VCF/GFF3 contracts remain gaps (bioinformatics-genomics/report.md:54-82). | Keep generic codecs; add standards-specific schemas, indexes, and loss diagnostics. |
| M-GPU-KERNELS | Medical imaging reports accelerator placement and strict profiles as implemented, but tiled inference and volume rendering are absent (medical-imaging/report.md:87-96). | Cheminformatics marks precision/device controls owner-gated, while epidemiology marks GPU authority owner-gated (cheminformatics-compchem/report.md:54-63; epidemiology-public-health/report.md:98). | Reuse the compute seam and E1803 fail-closed authority; owner-gate kernels, providers, precision, and determinism. |
| M-WORKFLOW-DAG | Bioinformatics requires task hashes, resume, keyed channels, and resource scheduling as P0 gaps (bioinformatics-genomics/report.md:43-53). | Medical imaging and systems biology describe headless batch/report paths as gaps without the same sample-channel law (medical-imaging/report.md:84-86; systems-biology/report.md:76,87). | Use resumable keyed DAGs as the shared mechanism; treat simpler batch/report profiles as constrained uses. |
| M-ODE-DAE | Systems biology reports no biological ODE/SSA/SDE solver despite generic tensor/linalg foundations (systems-biology/report.md:46-58,104-122). | Neuroscience and cheminformatics cite generic FFT/linalg/units or SCF/MD foundations but no domain simulator (neuroscience/report.md:85-90; cheminformatics-compchem/report.md:92-99). | Mint one solver family for domain equations and jumps, while keeping matrix solve, FFT, and chemistry kernels as separate providers. |
| Performance scope | The registry requests medical volume registration, segmentation inference, and DICOM loading (registry.json:46). | The medical performance record measures MONAI Auto3DSeg training time, not those requested load/registration cells (medical-imaging/performance.json:2-17). | Do not call the published training numbers a matched gauntlet; add the registry workload before a win claim. |
| Performance scope | The registry requests an EEG/MEG pipeline and a 1e5-neuron network (registry.json:48). | The neuroscience record reports an illustrative 50-cell Arbor ring timing and no 1e5-neuron gauntlet (neuroscience/performance.json:2-13). | Treat the tutorial timing as a capability smoke point only, not parity evidence. |

## Evidence and defects
All seven census arrays, performance records, claim ledgers, and manifests were read from their domain directories. Probes found expected E1001 for absent core.image, expected E1803 for undecided GPU authority, and path-specific generic FieldErrors for malformed FHIR-shaped JSON. No probe exposed an ICE or a wrong diagnostic; defects.json is therefore empty.
