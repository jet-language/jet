# Domain registry — every domain Jet must win, out of the box

Owner-editable. This is the source list for the domain matrix (Tower epoch e15). Each row names the incumbent languages, the best stack a practitioner actually uses today, the developer-experience exemplar Jet must beat stock, the performance exemplar Jet must beat on the gauntlet, and the workload that becomes the gauntlet cell. Add a row to request a domain; strike a row to remove one; the orchestrator runs a follow-up census wave for every change. The nine domains mined on 2026-09-01 (web tooling, web frameworks, live debugging, games, backend, systems, mobile, data and notebooks, CLI and TUI) live in epoch e14 and are not repeated here.

Rows: 108 domains in 11 families. Census evidence per domain lands under `~/.cache/jet-luna/dx2/<id>/` and is registered in `docs/reference/prior-art.md` by card.

## Science and numerics (e15-m01-science-numerics)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `numerical-computing` | Numerical computing | MATLAB, Julia, Python (NumPy/SciPy), Fortran | MATLAB core + toolboxes; Julia + SciML; NumPy/SciPy/Numba | MATLAB Live Editor and Julia REPL/Pluto | Julia and Fortran on BLAS/LAPACK | dense linear algebra, ODE solve, FFT on 1e6 points |
| `symbolic-math` | Symbolic mathematics | Wolfram Language, Python (SymPy), Maple, Maxima | Mathematica notebooks; SymPy; Symbolics.jl | Mathematica notebook | Mathematica kernel, FLINT, Symbolics.jl | symbolic simplification, integration, series on 1e4 expressions |
| `statistics` | Statistics | R, SAS, Stata, Python (statsmodels) | R tidyverse + CRAN task views; RStudio; brms | RStudio and tidyverse | R data.table, Julia | GLM and mixed models on 1e7 rows |
| `optimization-or` | Optimization and operations research | AMPL, Julia (JuMP), Python (Pyomo/CVXPY/OR-Tools), C++ | JuMP + HiGHS/Gurobi; OR-Tools; MiniZinc | JuMP | Gurobi, HiGHS, OR-Tools CP-SAT | MIPLIB subset LP/MILP, vehicle routing |
| `simulation-modeling` | Simulation and modeling | Modelica, Simulink, Python (SimPy/Mesa), AnyLogic (Java) | OpenModelica/Dymola; ModelingToolkit.jl; SimPy; Mesa | Modelica and ModelingToolkit.jl | Dymola, ModelingToolkit.jl | DAE system with 1e4 equations, agent-based sim with 1e6 agents |
| `hpc-parallel` | High-performance and parallel computing | C/C++/Fortran + MPI/OpenMP, Chapel, Julia | MPI + OpenMP + Kokkos/RAJA; SLURM; Chapel; Julia Distributed | Chapel and Julia Distributed | MPI C/Fortran, Kokkos | stencil and N-body across nodes, strong and weak scaling |
| `gpu-compute` | GPU compute | CUDA C++, Triton, OpenCL/SYCL, CUDA.jl, wgpu | CUDA + cuBLAS/cuDNN; Triton; CUDA.jl; JAX | Triton and CUDA.jl | CUDA hand-tuned kernels | GEMM, reductions, scans, custom fused kernels |
| `quantum-computing` | Quantum computing | Python (Qiskit, Cirq, PennyLane), Q#, OpenQASM | Qiskit + Aer; PennyLane; cuQuantum | Qiskit and PennyLane | qsim, cuQuantum | state-vector simulation of 30 qubits, circuit transpilation |
| `proof-formal` | Proof assistants and formal verification | Lean 4, Rocq/Coq, Isabelle, Agda, TLA+, Dafny, F* | Lean 4 + Mathlib + VS Code infoview; TLA+ Toolbox; Dafny | Lean 4 infoview | Lean 4 kernel, Isabelle | type-check Mathlib slice, model-check TLA+ spec |
| `logic-constraint` | Logic and constraint programming | Prolog (SWI), Datalog (Souffle), MiniZinc, clingo, miniKanren | SWI-Prolog; Souffle; MiniZinc IDE; clingo | MiniZinc IDE and SWI-Prolog | Souffle, clingo | Datalog program analysis on 1e7 facts, CP scheduling |
| `probabilistic-programming` | Probabilistic programming and Bayesian inference | Stan, Python (PyMC/NumPyro/Pyro), Turing.jl | Stan + brms/cmdstanr; PyMC; Turing.jl | brms and PyMC | Stan NUTS, NumPyro | hierarchical model NUTS sampling |
| `pde-fem` | PDE solvers and finite elements | C++ (deal.II, MFEM), Python (FEniCS), COMSOL, FreeFEM | FEniCSx; deal.II; MFEM; COMSOL | COMSOL and FEniCSx | deal.II, MFEM | Poisson and elasticity on unstructured meshes, 1e6 DOF |

## Engineering (e15-m02-engineering)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `signal-processing` | Signal processing | MATLAB (Signal Processing, DSP System toolboxes), Python (SciPy.signal), GNU Radio (C++/Python), DSP.jl | MATLAB Signal Analyzer, DSP System Toolbox; SciPy.signal; GNU Radio | MATLAB Signal Analyzer and DSP System Toolbox apps | FFTW, Intel IPP, hand-written C | FFT, filter banks, resampling, spectrogram at 1e8 samples/s |
| `acoustics-audio-engineering` | Acoustics and audio engineering | MATLAB (Audio Toolbox), COMSOL Acoustics, Python (pyroomacoustics), Odeon, EASE, Max/MSP | MATLAB Audio Toolbox apps; COMSOL Acoustics; pyroomacoustics; Odeon/EASE for room acoustics; Faust | MATLAB Audio Toolbox and COMSOL Acoustics | Odeon ray tracing, FDTD acoustic solvers, Faust | room impulse response, beamforming, FDTD acoustic field, real-time audio processing at 48 kHz |
| `control-systems` | Control systems | MATLAB/Simulink/Stateflow, Python (python-control), ControlSystems.jl, Modelica | Simulink + Control System Toolbox + Embedded Coder; python-control; ControlSystems.jl | Simulink | Simulink code generation, ControlSystems.jl | LQR/MPC design and closed-loop simulation with code generation |
| `rf-wireless-comms` | RF, wireless, and communications | MATLAB (Communications, 5G, WLAN, RF toolboxes), GNU Radio, C++ (srsRAN), Keysight ADS | MATLAB 5G Toolbox; GNU Radio; srsRAN; ADS | MATLAB 5G and Communications toolboxes | srsRAN C++, GNU Radio | OFDM link simulation, LDPC decoding, channel estimation |
| `electromagnetics-antenna` | Electromagnetics and antennas | HFSS, CST, FEKO, MATLAB Antenna Toolbox, openEMS, Meep | HFSS/CST; MATLAB Antenna Toolbox; openEMS; Meep | MATLAB Antenna Toolbox and CST | FDTD engines (Meep, openEMS) | FDTD patch antenna simulation, method of moments |
| `mechanical-cad-cae` | Mechanical CAD and computational design | SolidWorks, Fusion 360, Onshape, FreeCAD, CadQuery/Build123d, OpenSCAD | Onshape/Fusion; CadQuery + OCCT; OpenSCAD | Onshape and CadQuery | OCCT kernel, Parasolid | parametric model with booleans, fillets, STEP export, mesh generation |
| `fea-structural` | Finite element analysis | Abaqus, ANSYS Mechanical, Nastran, CalculiX, Code_Aster | ANSYS Workbench; Abaqus; CalculiX | ANSYS Workbench | Abaqus and Nastran solvers | linear static, modal, and nonlinear contact on 1e6 DOF |
| `cfd` | Computational fluid dynamics | OpenFOAM (C++), ANSYS Fluent, SU2, Basilisk, lattice Boltzmann codes | OpenFOAM; Fluent; SU2 | Fluent and SimScale | OpenFOAM, SU2 | lid-driven cavity, channel flow, transient turbulent case |
| `electrical-eda` | Electrical design and circuit simulation | SPICE (ngspice, LTspice, Xyce), KiCad, Altium, Verilog-A, Simscape Electrical | KiCad + ngspice; LTspice; Altium | KiCad and LTspice | Xyce, ngspice | transient simulation of switching regulator, PCB DRC |
| `chemical-process` | Chemical and process engineering | Aspen Plus/HYSYS, DWSIM, Cantera (C++/Python), CoolProp | Aspen; DWSIM; Cantera; CoolProp | Aspen Plus | Cantera | flowsheet convergence, thermodynamic property calls, reactor kinetics |
| `power-energy-systems` | Power and energy systems | PSS/E, PowerWorld, Python (pandapower, PyPSA), OpenDSS, Simscape Electrical | pandapower; PyPSA; OpenDSS; PSS/E | pandapower and PyPSA | PSS/E, OpenDSS | AC power flow, N-1 contingency, unit commitment |
| `civil-structural-bim` | Civil, structural, and BIM | Revit/Dynamo, Grasshopper, ETABS/SAP2000, IfcOpenShell (Python), Speckle | Revit + Dynamo; Grasshopper; IfcOpenShell; Speckle | Grasshopper and Dynamo | ETABS, IfcOpenShell C++ | IFC model parsing and analysis, frame analysis |
| `automotive-aerospace-systems` | Automotive and aerospace systems engineering | MATLAB Aerospace/Vehicle toolboxes, OpenMDAO (Python), JSBSim (C++), AUTOSAR tooling, SysML | MATLAB/Simulink; OpenMDAO; JSBSim; Cameo SysML | MATLAB toolboxes and OpenMDAO | JSBSim, in-house C++ | 6-DOF flight simulation, multidisciplinary optimization |
| `instrumentation-lab-automation` | Instrumentation and lab automation | LabVIEW, Python (PyVISA, NI-DAQmx), EPICS, Tango, SCPI | LabVIEW; PyVISA + nidaqmx; EPICS; Bluesky | LabVIEW | NI real-time, C DAQ drivers | 1 MS/s acquisition with live plotting and instrument control |

## Life sciences and health (e15-m03-life-sciences-health)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `bioinformatics-genomics` | Bioinformatics and genomics | C/C++ (BWA, samtools, GATK Java), R Bioconductor, Python (Biopython), Nextflow/Snakemake, Rust (noodles) | Nextflow + nf-core; Bioconductor; samtools/htslib; Biopython | Nextflow with nf-core and Bioconductor | BWA-MEM2, htslib, minimap2 | FASTQ to BAM to VCF pipeline, alignment throughput |
| `cheminformatics-compchem` | Cheminformatics and computational chemistry | Python (RDKit, PySCF, OpenMM), C++ (GROMACS, Open Babel), Fortran (Gaussian, ORCA) | RDKit + Jupyter; GROMACS/OpenMM; PySCF/Psi4 | RDKit and OpenMM | GROMACS, ORCA | molecular dynamics steps/day, fingerprint similarity over 1e7 molecules, SCF |
| `medical-imaging` | Medical imaging | C++ (ITK/VTK), Python (SimpleITK, pydicom, MONAI), 3D Slicer, MATLAB Medical Imaging Toolbox | 3D Slicer; MONAI; SimpleITK; pydicom | 3D Slicer and MONAI | ITK, CUDA pipelines | volume registration, segmentation inference, DICOM series load |
| `health-records-interop` | Health records and interoperability | Java (HAPI FHIR), C# (Firely), HL7 v2, SMART on FHIR (JS) | HAPI FHIR; Firely; SMART on FHIR; OpenMRS | HAPI FHIR and SMART app launch | HAPI FHIR JPA server | FHIR bundle validation, search, and transaction throughput |
| `neuroscience` | Neuroscience | Python (MNE, Brian2, SpikeInterface), NEURON, MATLAB (FieldTrip, EEGLAB), Arbor | MNE-Python; NEURON/Arbor; SpikeInterface | MNE-Python | NEURON, Arbor | EEG/MEG pipeline, spiking network of 1e5 neurons |
| `epidemiology-public-health` | Epidemiology and public health | R (epi packages, EpiModel), Python (covasim), Stan | R epidemiology stack; covasim; Stan | R with EpiModel | covasim with Numba | SEIR agent-based simulation with 1e6 agents, outbreak inference |
| `systems-biology` | Systems biology | SBML tooling (COPASI C++, Tellurium Python), MATLAB SimBiology, Julia (Catalyst) | COPASI; Tellurium; SimBiology; Catalyst.jl | SimBiology and Tellurium | COPASI, Catalyst.jl | ODE model fitting and stochastic simulation of reaction networks |

## Earth and space (e15-m04-earth-space)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `gis-geospatial` | GIS and geospatial | QGIS/ArcGIS (Python ArcPy), C/C++ (GDAL/OGR, PostGIS), Python (GeoPandas, Shapely), JS (MapLibre, deck.gl), H3 | QGIS; GeoPandas + Shapely; PostGIS; MapLibre/deck.gl | QGIS and GeoPandas | GDAL, PostGIS, H3 | spatial join of 1e7 points to polygons, raster reprojection, vector tiles |
| `remote-sensing` | Remote sensing | Google Earth Engine (JS/Python), Python (rasterio, xarray), SNAP (Java), Orfeo (C++) | Earth Engine; rasterio + xarray + Dask; SNAP | Google Earth Engine | GDAL, Dask, Orfeo | NDVI time series over 1e4 tiles, cloud masking |
| `climate-weather` | Climate and weather | Fortran (WRF, CESM, IFS), Python (xarray, MetPy, Pangeo), CDO/NCO | xarray + Pangeo; WRF; CDO | xarray and Pangeo | Fortran + MPI models | netCDF regridding, stencil dynamics on a global grid |
| `geoscience-seismology` | Geoscience and seismology | Python (ObsPy), C (SeisComP, Madagascar), Petrel, OpendTect | ObsPy; SeisComP; Madagascar | ObsPy | Madagascar, SeisComP | waveform processing, seismic migration |
| `astronomy-astrophysics` | Astronomy and astrophysics | Python (astropy ecosystem), C/C++ (Gadget, Enzo, CASA), Fortran, IDL legacy | astropy + Jupyter; CASA; Gadget-4 | astropy | Gadget-4, Enzo | FITS pipeline, N-body simulation, HEALPix maps |
| `oceanography-hydrology` | Oceanography and hydrology | Fortran (MOM6, ROMS, MODFLOW), Python (FloPy, xarray), HEC-RAS, SWMM | MOM6; MODFLOW + FloPy; HEC-RAS | FloPy | MODFLOW, ROMS | groundwater flow simulation, ocean model step |
| `space-satellite` | Space and satellite systems | Java (Orekit), GMAT, STK, Python (poliastro/hapsira), C (NASA cFS), CCSDS | Orekit; GMAT; STK; cFS | STK and poliastro | Orekit, GMAT | orbit propagation with perturbations, ground station scheduling, telemetry decoding |

## Finance and business (e15-m05-finance-business)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `quant-trading` | Quantitative finance and trading | C++ (QuantLib, low-latency), q/kdb+, Python (pandas, Zipline, Backtrader), Lean (C#) | kdb+; QuantLib; QuantConnect Lean; pandas | QuantConnect and kdb+ | kdb+, hand-tuned C++ | order book replay, backtest over 1e9 ticks, option pricing |
| `risk-actuarial` | Risk and actuarial | R (actuar, ChainLadder), Excel, Prophet (actuarial), Python | R actuarial packages; Prophet; Excel | R actuar | vectorized C++ | Monte Carlo VaR, reserving triangles, cash-flow projection |
| `econometrics` | Econometrics | Stata, EViews, R, gretl, Python (statsmodels, linearmodels) | Stata; R fixest; statsmodels | Stata | R fixest, Julia | panel regressions with high-dimensional fixed effects on 1e8 rows |
| `spreadsheets-business-logic` | Spreadsheets and business logic | Excel (formulas, LAMBDA, VBA, Office Scripts), Google Sheets (Apps Script), Airtable | Excel + Power Query + LAMBDA; Google Sheets | Excel | Excel calculation engine | recalculation of a 1e6-cell dependent model, Power Query transform |
| `accounting-erp` | Accounting and ERP | SAP ABAP, NetSuite SuiteScript (JS), Odoo (Python), Dynamics (C#/X++) | Odoo; SAP; NetSuite | Odoo | SAP HANA | ledger posting, period close, report generation |
| `fintech-payments` | Fintech and payments | Java/Go/TypeScript services, Zig (TigerBeetle), ISO 20022, Stripe SDKs | Stripe; TigerBeetle; Open Banking APIs; ISO 20022 libraries | Stripe SDK and docs | TigerBeetle | double-entry ledger transfers/s, ISO 20022 message parsing |
| `business-rules-workflows` | Business rules and workflow engines | Drools/DMN (Java), Camunda/BPMN, Temporal (Go/TS), Step Functions | Temporal; Camunda; Drools | Temporal | Drools, Temporal | durable workflow with retries, rules evaluation throughput |
| `low-code-automation` | Low-code and automation platforms | Zapier, n8n (TS), Make, Power Automate, Retool, Airtable | n8n; Retool; Power Automate | n8n and Retool | n/a (integration latency) | 1,000-step workflow with branching and retries |

## AI, ML, and data platforms (e15-m06-ai-ml)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `ml-training` | Machine learning training | Python (PyTorch, JAX, TensorFlow, Lightning, Hugging Face) | PyTorch + Lightning + Hugging Face + W&B; JAX/Flax | PyTorch Lightning and Hugging Face | JAX/XLA, torch.compile | transformer and ResNet training steps/s, mixed precision |
| `ml-inference-serving` | ML inference and serving | C++ (TensorRT, ONNX Runtime, llama.cpp), Python (vLLM, TorchServe), Triton Inference Server | vLLM; TensorRT; ONNX Runtime; llama.cpp; Ollama | Ollama and vLLM | TensorRT, llama.cpp | LLM tokens/s at batch, image model latency |
| `llm-apps-agents` | LLM applications and agents | Python (LangChain/LangGraph, DSPy, Instructor), TypeScript (Vercel AI SDK), MCP | Vercel AI SDK; LangGraph + Studio; MCP; DSPy | Vercel AI SDK and LangGraph Studio | n/a (latency and cost) | agent loop with tool calls, structured output, streaming |
| `nlp-text` | Natural language processing | Python (spaCy, NLTK, Hugging Face tokenizers), Rust tokenizers | spaCy; Hugging Face tokenizers; Stanza | spaCy | Rust tokenizers, spaCy Cython | tokenization and NER throughput on 1 GB text |
| `computer-vision` | Computer vision | C++/Python (OpenCV), PyTorch (torchvision, Ultralytics YOLO, Detectron2), Kornia | OpenCV; Ultralytics; Kornia | Ultralytics | OpenCV C++, TensorRT | image pipeline and detection FPS |
| `recommender-search` | Search, vector search, and recommendation | Java (Lucene, Elasticsearch, OpenSearch, Vespa), C++ (FAISS), Rust (Qdrant, Meilisearch), Milvus | Vespa; Elasticsearch; Qdrant; FAISS | Vespa and Qdrant | FAISS, Vespa | ANN search QPS at 1e8 vectors, BM25 index and query |
| `reinforcement-learning` | Reinforcement learning and simulation | Python (Gymnasium, Stable-Baselines3, RLlib), MuJoCo, Isaac Gym/Lab | Gymnasium + SB3; Isaac Lab; MuJoCo | Gymnasium and SB3 | Isaac Gym, MuJoCo MJX | PPO environment steps/s, vectorized environments |
| `mlops` | MLOps, experiment tracking, and labeling | Python (MLflow, W&B, DVC, Kubeflow), Label Studio, Feast | W&B; MLflow; DVC; Label Studio | Weights and Biases | n/a | experiment tracking, model registry, feature store serving |
| `data-engineering-etl` | Data engineering and ETL | Python (Airflow, Dagster, Prefect, dlt), SQL (dbt), Scala/Java (Spark) | Dagster + dbt; Airflow; Spark | Dagster and dbt | Spark, Polars | DAG of 100 tasks, 10 GB transform, incremental models |
| `stream-processing` | Stream processing | Java (Kafka Streams, Flink), Rust (Materialize, Arroyo), Python (Bytewax) | Flink; Materialize; Kafka Streams | Materialize SQL and Flink SQL | Flink, Arroyo | windowed aggregation at 1e6 events/s with exactly-once |
| `big-data-analytics` | Analytical databases and big data | Spark (Scala), C++ (ClickHouse, DuckDB), Java (Trino), Python (Dask) | DuckDB; ClickHouse; Spark; Trino | DuckDB | ClickHouse, DuckDB | TPC-H subset at SF100 |
| `bi-dashboards` | Business intelligence and dashboards | Tableau, Power BI (DAX), Looker (LookML), Metabase, Superset, Evidence, Streamlit | Evidence; Metabase; Streamlit | Evidence and Metabase | n/a (query engines) | dashboard over 1e8 rows with drill-down |

## Media and creative (e15-m07-media-creative)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `audio-music-production` | Audio and music production | C++ (JUCE, VST3/AU/CLAP), Max/MSP, Pure Data, SuperCollider, Faust, Csound | JUCE + CLAP; Faust; Max/MSP; SuperCollider | Max/MSP and Faust | Faust, JUCE C++ | real-time synth and effect at 48 kHz with 64-sample buffers, plugin load |
| `video-vfx-compositing` | Video, VFX, and compositing | C/C++ (FFmpeg, GStreamer, OpenFX), Nuke (Python), DaVinci Fusion | FFmpeg; Nuke; DaVinci Resolve; GStreamer | Nuke and DaVinci Resolve | FFmpeg | 4K transcode, composite graph render, frame-accurate seeking |
| `3d-animation-dcc` | 3D, animation, and digital content creation | Blender (Python bpy), Houdini (VEX/PDG), Maya (MEL/Python), OpenUSD (C++/Python), Cinema 4D | Houdini; Blender; OpenUSD | Houdini and Blender | OpenUSD, Embree | USD scene composition, procedural geometry, rigging evaluation |
| `rendering-graphics` | Rendering and graphics programming | C++ (Vulkan, DirectX 12, Metal, Embree, OptiX), Rust (wgpu), shader languages (GLSL, HLSL, WGSL, Slang) | Vulkan + Slang; wgpu; Embree; Mitsuba | Slang and wgpu | Vulkan, Embree, OptiX | path tracer, deferred renderer at 4K, BVH build |
| `image-processing` | Image processing | C++ (OpenCV, libvips, ImageMagick), Python (Pillow, scikit-image), Halide | scikit-image; libvips; Halide | scikit-image and Halide | Halide, libvips | image pipeline throughput, resize and color transforms on 1e5 images |
| `ar-vr-xr` | AR, VR, and spatial computing | Unity XR (C#), Unreal (C++), OpenXR, ARKit/RealityKit (Swift), ARCore (Kotlin), WebXR | Unity XR; RealityKit; OpenXR; WebXR | Unity XR and RealityKit | Unreal, native OpenXR | stereo render with tracking at 90 Hz, anchor persistence |
| `creative-coding` | Creative coding and generative art | Processing/p5.js, openFrameworks (C++), TouchDesigner, Nannou (Rust), Cables | p5.js; TouchDesigner; openFrameworks; Nannou | p5.js and TouchDesigner | openFrameworks | particle system and shader sketch at 60 fps, live coding |
| `typography-publishing` | Typesetting and publishing | LaTeX, Typst (Rust), InDesign, Pandoc (Haskell), Quarto, HarfBuzz | Typst; Quarto; Pandoc; LaTeX | Typst | Typst, HarfBuzz | 500-page document compile with math, PDF export |

## Embedded and hardware (e15-m08-embedded-hardware)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `embedded-iot` | Embedded and IoT | C (Zephyr, FreeRTOS, ESP-IDF, Arduino), Rust (Embassy, embedded-hal), TinyGo, MicroPython | PlatformIO; Zephyr; Embassy + probe-rs; ESP-IDF | PlatformIO and Embassy with probe-rs | C on Zephyr, Rust Embassy | interrupt latency, power sleep cycle, OTA update, sensor loop |
| `plc-industrial` | PLC and industrial automation | IEC 61131-3 (Structured Text, Ladder), CODESYS, TwinCAT, OPC UA, Modbus, PLCnext | CODESYS; TwinCAT; OPC UA SDKs | CODESYS and TwinCAT | TwinCAT real-time | 1 ms cycle control loop with OPC UA publishing |
| `fpga-hdl` | FPGA and hardware description | Verilog/SystemVerilog, VHDL, Chisel (Scala), Amaranth (Python), SpinalHDL, Vivado/Quartus, Verilator, cocotb | Chisel or Amaranth; Verilator; cocotb; Vivado | Chisel, Amaranth, and cocotb | Verilator | RTL simulation of a RISC-V core, synthesis and timing closure |
| `robotics` | Robotics | C++/Python (ROS 2, MoveIt, Drake), Gazebo/Isaac Sim, MATLAB Robotics and Navigation toolboxes | ROS 2 + Foxglove; Drake; Isaac Sim | ROS 2 with Foxglove | Drake, C++ ROS nodes | motion planning, sensor fusion (EKF), point cloud processing at 30 Hz |
| `drones-uav` | Drones and UAV | C++ (PX4, ArduPilot), MAVLink, MATLAB UAV Toolbox, DJI SDK | PX4 + QGroundControl; ArduPilot; MAVSDK | PX4 and QGroundControl | PX4 | flight controller loop at 1 kHz, SITL simulation, mission upload |
| `automotive-embedded` | Automotive embedded and functional safety | C (AUTOSAR Classic, MISRA), C++ (AUTOSAR Adaptive), CAN/SOME-IP, Simulink Embedded Coder, ISO 26262 tooling | Simulink + Embedded Coder; Vector tools; AUTOSAR | Simulink Embedded Coder and Vector CANoe | MISRA C | CAN gateway throughput, ASIL-D control task with WCET |
| `real-time-safety-critical` | Real-time and safety-critical systems | Ada/SPARK, C (RTEMS, VxWorks), Rust (Ferrocene), seL4, DO-178C toolchains, WCET analysis | SPARK + GNAT Pro; seL4; Ferrocene | SPARK and GNAT Studio | Ada and C | WCET-bounded control loop, certification evidence generation |
| `hardware-drivers` | Device drivers and hardware interfacing | C (Linux kernel, libusb), Rust for Linux, embedded-hal, device trees, DPDK | Linux kernel modules; Rust for Linux; libusb; embedded-hal | Rust for Linux and embedded-hal | C kernel drivers | DMA throughput, USB bulk transfer, GPIO toggle rate |

## Platforms and infrastructure (e15-m09-platforms-infrastructure)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `databases-storage-engines` | Databases and storage engines | C (SQLite, PostgreSQL), C++ (RocksDB, DuckDB), Zig (TigerBeetle), Rust (sled, Neon) | SQLite; PostgreSQL; RocksDB; TigerBeetle | SQLite and DuckDB embedding | RocksDB, TigerBeetle | YCSB and TPC-C throughput, WAL durability, B-tree scan |
| `networking-protocols` | Networking and protocols | C/C++ (DPDK, Envoy, msquic), Rust (quiche, tokio), Go (net), eBPF/XDP, gRPC, WebRTC | gRPC; Envoy; quiche; io_uring; eBPF | gRPC and Envoy | DPDK, io_uring, XDP | HTTP/2 and QUIC proxy throughput, packet processing Mpps |
| `telecom-5g` | Telecom and 5G core | C (Open5GS), C++ (srsRAN, OAI), Erlang/OTP, Kamailio (SIP) | Open5GS; srsRAN; Erlang/OTP; Kamailio | Erlang/OTP | srsRAN, Open5GS | signaling throughput, SIP registrations/s, PHY layer processing |
| `devops-iac-cloud` | DevOps, infrastructure as code, and cloud | HCL (Terraform/OpenTofu), Pulumi (TS/Python/Go), AWS CDK, Nix, Ansible (YAML), Kubernetes operators (Go), Crossplane | Pulumi or CDK; Nix; Kubernetes operators; Helm | Pulumi and Nix | n/a (plan and apply latency) | 1,000-resource plan and apply, drift detection, policy checks |
| `containers-orchestration` | Containers, orchestration, and sandboxes | Go (Docker, Kubernetes, Nomad), Rust (Firecracker, youki), WASM (Spin, wasmCloud) | Docker Compose + Tilt; Kubernetes; Firecracker; Spin | Docker Compose and Tilt | Firecracker, crun | cold start latency, density per host, image build cache |
| `os-kernels` | Operating systems and kernels | C (Linux, BSD), Rust (Redox, Rust for Linux), Zircon (C++), seL4, Theseus | Linux; Redox; seL4 | Rust for Linux and Redox | Linux | syscall latency, scheduler benchmark, filesystem throughput |
| `compilers-language-tooling` | Compilers and language tooling | C++ (LLVM, MLIR, Clang), Rust (rustc, Cranelift, tree-sitter), C# (Roslyn), Haskell (GHC), OCaml | LLVM/MLIR; tree-sitter; Cranelift; Roslyn; LSP | tree-sitter, MLIR, and Roslyn | LLVM, Cranelift | parse and type-check 1e6 lines, codegen throughput, incremental rebuild |
| `build-systems-monorepo` | Build systems and monorepos | Bazel/Buck2 (Starlark), Nx/Turborepo (TS), Gradle (Kotlin), CMake, Meson, Nix | Buck2 or Bazel; Nx; Nix | Buck2 and Nx | Buck2, Bazel remote execution | incremental build of a 1e4-target graph, remote cache hit rate |
| `wasm-plugins-sandboxing` | WebAssembly, plugins, and sandboxing | Rust (Wasmtime, Wasmer), WASI, Component Model, Extism, Lua embedding | Wasmtime + Component Model; Extism; WASI | Extism and the Component Model | Wasmtime, Wasmer | plugin call overhead, instantiation time, capability isolation |
| `serverless-edge` | Serverless and edge | JS/TS (Cloudflare Workers, Deno Deploy, Vercel), Go/Rust (Lambda), Fastly Compute (Wasm) | Cloudflare Workers; Deno Deploy; AWS Lambda | Cloudflare Workers and Deno Deploy | Workers isolates, Fastly Compute | cold start, p99 latency, KV and durable object access |
| `desktop-apps` | Desktop applications | Electron (TS), Tauri (Rust + web), Qt/QML (C++), Flutter, WPF/WinUI (C#), SwiftUI/AppKit, GTK, Slint | Tauri; Qt; SwiftUI; Slint; Flutter desktop | Tauri, SwiftUI, and Slint | Qt and native toolkits | startup time, memory, 1e6-row list scroll, file dialogs and tray |
| `browser-extensions` | Browser extensions and userscripts | JS/TS (WebExtensions, Plasmo, wxt) | wxt or Plasmo; WebExtensions API | wxt and Plasmo | n/a | content script injection overhead, storage sync, cross-browser packaging |
| `cms-ecommerce` | CMS and e-commerce | PHP (WordPress, Laravel), Shopify (Liquid, Hydrogen), TS (Medusa, Payload, Strapi), Python (Saleor) | Payload; Shopify Hydrogen; Medusa | Payload and Shopify | n/a (catalog render) | catalog page render, checkout flow, content model migrations |
| `messaging-eventing` | Messaging and eventing | Java (Kafka, Pulsar), Go (NATS), Erlang (RabbitMQ), C++ (Redpanda), Redis Streams | NATS; Kafka or Redpanda; RabbitMQ | NATS | Redpanda, Kafka | publish and subscribe throughput and latency, exactly-once delivery |
| `distributed-systems` | Distributed systems and consensus | Go (etcd, Raft), C++ (FoundationDB), Rust (Automerge), JS (Yjs), TLA+, Jepsen (Clojure), deterministic simulation (TigerBeetle VOPR) | Raft libraries; CRDTs (Automerge, Yjs); FoundationDB; Jepsen; deterministic simulation testing | Automerge and Yjs | FoundationDB, TigerBeetle | consensus throughput, CRDT merge, Jepsen-style fault injection |
| `observability-platforms` | Observability platforms | Go (Prometheus, Grafana, Loki, Jaeger), OpenTelemetry (multi), eBPF (Pixie) | OpenTelemetry; Prometheus; Grafana; Loki; Tempo | Grafana and OpenTelemetry | Prometheus, VictoriaMetrics | metrics ingest rate, trace query latency, log indexing |
| `api-design-contracts` | API design and contracts | OpenAPI, GraphQL (Apollo, Hasura), gRPC/protobuf, tRPC (TS), AsyncAPI, JSON Schema | tRPC; GraphQL; gRPC; OpenAPI codegen | tRPC and GraphQL | protobuf | schema-driven codegen, validation throughput, contract tests |

## Security (e15-m10-security)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `security-tooling` | Security tooling and penetration testing | Ruby (Metasploit), Python (Scapy, Impacket), Go (Nuclei), C (nmap), Semgrep, CodeQL | Burp Suite; Metasploit; Nuclei; Semgrep; CodeQL | Burp Suite and Semgrep | nmap, masscan | port scan of /16, static analysis rules over 1e6 lines |
| `reverse-engineering` | Reverse engineering and malware analysis | Ghidra (Java/Python), IDA (IDAPython), Binary Ninja, radare2 (C), Frida (JS), angr (Python), YARA | Ghidra; Binary Ninja; Frida; angr; YARA | Binary Ninja and Frida | Capstone, angr, Unicorn | disassembly and decompilation of a 10 MB binary, emulation, YARA scanning |
| `cryptography-engineering` | Cryptography engineering | C (libsodium, BoringSSL, OpenSSL), Rust (RustCrypto, ring), F* (HACL*), PQC (liboqs) | libsodium; RustCrypto; BoringSSL; liboqs | libsodium and RustCrypto | BoringSSL, hand-tuned assembly | AES-GCM, ChaCha20-Poly1305, X25519, ML-KEM throughput with constant-time proof |
| `digital-forensics` | Digital forensics and incident response | Python (Volatility, Plaso), C (Sleuth Kit), Go (Velociraptor, osquery C++), Sigma rules | Velociraptor; osquery; Volatility; Autopsy | Velociraptor and osquery | Sleuth Kit, osquery | memory image parse, disk timeline, endpoint query fan-out |
| `identity-auth` | Identity, authentication, and authorization | Java (Keycloak), Go (Ory, SpiceDB), OpenFGA, Auth0/Clerk, WebAuthn libraries, SAML | Ory; Keycloak; SpiceDB or OpenFGA; passkeys | Clerk and Ory | SpiceDB | OIDC flows, authorization check throughput, passkey ceremonies |
| `blockchain-web3` | Blockchain and zero-knowledge | Solidity (Foundry, Hardhat), Rust (Solana Anchor, Substrate), Move, TS (viem), zk (Circom, Noir, Halo2) | Foundry; Anchor; viem; Noir | Foundry and Anchor | Solana runtime, zk provers | contract test and fuzz, transaction throughput, proof generation |

## Tools and productivity (e15-m11-tools-productivity)

| id | domain | incumbents | best stack today | DX exemplar to beat | performance exemplar to beat | gauntlet workload |
|---|---|---|---|---|---|---|
| `editor-ide-extensions` | Editor and IDE extensions | TS (VS Code API), Lua (Neovim), Rust/WASM (Zed), Kotlin (JetBrains), Emacs Lisp, LSP | VS Code extension API; Neovim Lua; Zed extensions | VS Code API and Neovim Lua | Zed | extension activation time, LSP round trip, large-file editing |
| `shell-sysadmin` | Shell and system administration scripting | bash/zsh/fish, PowerShell, Nushell (Rust), Oils, Ansible | Nushell; PowerShell; bash | Nushell and PowerShell | bash with coreutils | log processing pipeline, fleet configuration, process supervision |
| `text-processing-parsing` | Text processing and parsing | Perl/awk/sed, regex engines (RE2, PCRE2), Rust (nom, ripgrep), tree-sitter, XSLT/XPath, jq | ripgrep; jq; nom; tree-sitter | jq and nom | RE2, ripgrep | 10 GB log grep and transform, structured document query |
| `documentation-static-sites` | Documentation and static sites | TS (Docusaurus, Astro Starlight), Python (MkDocs Material, Sphinx), Rust (mdBook), Go (Hugo), Typst | Astro Starlight; MkDocs Material; mdBook | Starlight and MkDocs Material | Hugo | 1e4-page build, search index, versioned docs |
| `testing-qa-automation` | Testing and QA automation | Playwright/Cypress/Selenium (TS/Java/Python), Appium, k6/Locust (load), Hypothesis/QuickCheck, Pact, mutation testing | Playwright; k6; Hypothesis; Pact; Stryker | Playwright and k6 | k6 | 1e4 virtual-user load test, property test throughput, contract verification |
| `chatbots-conversational` | Chatbots and conversational interfaces | Slack Bolt (TS/Python), Discord.js, Telegram bots, Rasa (Python), Botpress, Twilio | Slack Bolt; Discord.js; Rasa | Slack Bolt and Discord.js | n/a | event handling latency, conversation state, rate-limit handling |
| `education-teaching` | Education and teaching programming | Scratch, Python turtle, Processing, Swift Playgrounds, Racket (HtDP), Pyret, Sonic Pi | Scratch; Swift Playgrounds; Racket + DrRacket; Pyret | Scratch and Swift Playgrounds | n/a | time to first running program, error message readability, classroom sharing |
| `office-document-automation` | Office and document automation | Python (openpyxl, python-docx, reportlab), Apps Script, Office.js, Java (PDFBox, POI), pdf.js | Office.js; Apps Script; openpyxl; PDFBox | Office.js and Apps Script | PDFium, Apache POI | 1e4-page PDF generation, spreadsheet read and write, mail merge |
| `package-registries-supply-chain` | Package managers, registries, and supply chain | npm/pnpm (JS), cargo, uv/pip (Python), Nix, Go modules, SBOM (CycloneDX), Sigstore, SLSA | cargo; uv; pnpm; Nix; Sigstore | cargo, uv, and pnpm | uv, pnpm | dependency resolution and install time, lockfile diff, provenance verification |

## How a row is used

1. A miner produces the census for the row: report, feature census with Jet cross-check and binary probes, claim ledger, sources, and a performance section naming the incumbent to beat and the gauntlet workload.
2. The family synthesis groups shared P0 findings into mechanism cards (one mechanism, many domains) under e15-m12; every domain gets one design card carrying its census, the ballots its plan lane must author, its scaffold, and its benchmark peer.
3. Ballots for user-facing surfaces are raised per family as its census lands, with the full six-pass process.
4. The gauntlet gains one cell per workload column; the competitive performance gate applies per cell (strict win, Rust parity band 1.05).
