# Embedded-hardware family synthesis

## Family verdict
Embedded hardware is a target-bound evidence problem: firmware, controllers, HDL, robots, vehicles, ECUs, safety systems, drivers, and instruments all need one accountable chain from declared machine facts through bounded I/O, timing, artifacts, and physical or simulated witness. Jet already supplies typed values, effects, target dossiers, unsafe obligations, deterministic replay, and structured evidence, but the board/HAL, bus, interrupt/DMA, flash/debug, OTA, fieldbus, robot-bus, HDL, WCET, and qualification layers remain mostly real gaps. Reuse existing numeric, format, network, workflow, visualization, and hardware-I/O mechanisms; do not mistake those substrates for domain completion.

## Mechanisms

| ID | Mechanism | Domains | Gate kind | Jet state | Reuse |
|---|---|---:|---|---|---|
| M-BOARD-BSP | Typed board, target, BSP, and HAL facts | 8 | public-api | mixed | new |
| M-WCET | Target-bound timing, jitter, WCET, and deadline evidence | 8 | invariant | real-gap | new |
| M-SAFETY-EVIDENCE | Safety, proof, qualification, and traceability evidence | 6 | invariant | mixed | new |
| M-SCI-FORMATS | Scientific and engineering file formats | 6 | stdlib-dependency | mixed | reused |
| M-BUS-TXN | Bounded typed device and protocol transactions | 5 | public-api | real-gap | new |
| M-HARDWARE-IO | Authority-safe hardware and instrument I/O | 5 | public-api | real-gap | reused |
| M-WORKFLOW-DAG | Inspectable workflow graphs and execution plans | 5 | public-api | mixed | reused |
| M-EVIDENCE | Inspection, diagnostics, and reproducible evidence | 4 | none | mixed | reused |
| M-FLASH-DEBUG | Firmware artifacts, boot, flash, and debug receipts | 4 | command | mixed | new |
| M-INTERRUPTS | Interrupt vectors, priority, and bounded handoff | 4 | public-api | real-gap | new |
| M-PACKAGING | Package, dependency, and native-provider boundary | 4 | stdlib-dependency | mixed | reused |
| M-SIMULATION | Reproducible domain simulation lifecycle | 4 | public-api | mixed | reused |
| M-UNITS | Typed dimensional and affine quantities | 4 | invariant | mixed | reused |
| M-NETWORKING | Typed network and service protocol boundaries | 3 | none | mixed | reused |
| M-DATA-TABLES | Typed tables, records, and result schemas | 2 | none | mixed | reused |
| M-DMA | DMA ownership, mapping, queues, and completion | 2 | public-api | real-gap | new |
| M-FIELDBUS | Industrial and vehicle fieldbus models | 2 | public-api | real-gap | new |
| M-NOTEBOOK | Notebook and interactive REPL loop | 2 | none | mixed | reused |
| M-OTA | Signed update images, slots, rollback, and recovery | 2 | public-api | real-gap | new |
| M-ROBOT-BUS | Typed robot message buses, QoS, and command lifecycles | 2 | public-api | real-gap | new |
| M-STORAGE | Durable scientific stores and caches | 2 | stdlib-dependency | mixed | reused |
| M-DSP-FILTERS | Typed filter design and multirate DSP | 1 | public-api | real-gap | reused |
| M-GPU-KERNELS | Explicit GPU kernels and placement receipts | 1 | public-api | mixed | reused |
| M-HDL-SEMANTICS | Typed HDL signals, clocks, elaboration, and RTL | 1 | syntax | real-gap | new |
| M-LINALG | Typed dense tensors and linear algebra | 1 | none | mixed | reused |
| M-MESH-STENCILS | Meshes, stencils, and discretized field operators | 1 | public-api | real-gap | reused |
| M-OPTIMIZATION | Constrained optimization and design studies | 1 | public-api | real-gap | reused |
| M-PLOTTING | Deterministic and interactive engineering visualization | 1 | ui | mixed | reused |
| M-REALTIME-STREAMS | Bounded real-time audio and device streams | 1 | invariant | mixed | reused |
| M-SOLVER-SUBSTRATE | Checked nonlinear and domain solver substrate | 1 | public-api | mixed | reused |

New mechanisms: 11; reused mechanisms: 19; P0 rows: 306; mechanism-assigned P0 rows: 294. The three widest mechanisms are M-BOARD-BSP (8 domains), M-WCET (8 domains), M-SAFETY-EVIDENCE (6 domains). Existing IDs are reused only where prior mechanism files define them; no M-HDL-* or M-CIRCUIT-* definition was found.

## Domain matrix

| Domain | Benchmark peer | P0 rows | Performance incumbent | Gauntlet | Biggest beat vector |
|---|---|---:|---|---|---|
| embedded-iot | C on Zephyr; Rust Embassy | 35 | C on Zephyr (interrupt/sensor loop) and Rust Embassy (async HAL/executor) | none | One semantic Prelude can carry target providers, effects, artifacts, and diagnostics across host and no-OS tiers; #2300 criteria 2–5 and real-board witnesses remain open. |
| plc-industrial | Beckhoff TwinCAT 3 RT PLC with EtherCAT and OPC UA | 26 | Beckhoff TwinCAT 3 Real-Time PLC runtime with TwinCAT EtherCAT master and distributed clocks, paired with an OPC UA publication path | none | One target, authority, and evidence ledger can connect controller capabilities, MMIO, providers, artifacts, and faults; the real controller witness is unbuilt. |
| fpga-hdl | Verilator 5.050 native RTL simulator | 35 | Verilator 5.050 native RTL simulator | none — gauntlet/measurement-manifest.json currently covers 22 Jet/Python-oriented entries and has no FPGA, HDL, Verilator, or Ibex cell. | One checked target/evidence ledger could span HDL lowering, simulation, constraints, bitstream, and board receipt; Jet currently has no circuit that feeds it. |
| robotics | Drake C++ ROS nodes | 38 | Drake, C++ ROS nodes | embedded.kernel exists in gauntlet/measurement-manifest.json, but the 2026-09-01 result has no entries for that cell; no ROS/Drake/PCL 30 Hz robotics cell exists. | Typed physical sensor contracts can make frame, covariance, unit, shape, and missingness errors visible in one ledger; the robotics message layer is unbuilt. |
| drones-uav | PX4 Autopilot C++ on NuttX/POSIX | 26 | PX4 | none | Typed target truth can bind memory, linker, allocator, panic, MMIO, provider, and AOT facts before firmware publication; PX4 board setup is not one compiler dossier. |
| automotive-embedded | MISRA C AUTOSAR Classic CAN/CAN FD with Vector CANoe SIL/HIL | 36 | MISRA C production firmware on an AUTOSAR Classic CAN/CAN FD stack, with Vector CANoe or equivalent SIL/HIL test harness | embedded.kernel (partial host replay only; no CAN, ASIL, physical MMIO, RTOS, or WCET) | A typed evidence ledger can carry ECU capability, target, ownership, diagnostic, and performance identity through one artifact path; the automotive binding is unbuilt. |
| real-time-safety-critical | seL4/sel4bench C microkernel IPC; Ada/SPARK and RTOS peers | 31 | seL4/sel4bench C microkernel IPC path, with Ada/SPARK and RTOS control-loop implementations as the safety peer | embedded.kernel | One ledger can expose code, authority, proof, target, replay, and evidence facts to compiler, inspection, and archive instead of splitting them across tools. |
| hardware-drivers | C Linux kernel drivers; libusb; DPDK | 33 | C Linux kernel drivers, with libusb for userspace USB and DPDK for userspace DMA | embedded.kernel (currently unmeasured; gauntlet/results/2026-09-01.json records entries=[] and verdict=unmeasured) | Audited low-level access lets ordinary code stay safe while a reasoned unsafe block carries device obligations; C does not make that audit receipt a language invariant. |
| instrumentation-lab-automation | NI M/X hardware-timed acquisition with native NI-DAQmx C | 46 | NI M Series/X Series hardware-timed acquisition with the native NI-DAQmx C driver path and NI real-time buffering | none | One typed data and error kernel can carry acquisition, control, plot, report, limits, and provenance instead of splitting Python, driver, and publication conventions. |

## Owner gates

| Suggested decision | Gate | Affected mechanisms/domains |
|---|---|---|
| D-EH-PLATFORM1 | Choose the common board/BSP/target/HAL, artifact, flash/debug, and no-OS provider boundary. | M-BOARD-BSP, M-FLASH-DEBUG, M-SAFETY-EVIDENCE; all domains |
| D-EH-IO1 | Choose typed bus transactions, interrupts, DMA ownership, timing budgets, and hardware error/recovery facts. | M-BUS-TXN, M-INTERRUPTS, M-DMA, M-WCET; all domains |
| D-EH-UPDATE1 | Choose signed image, OTA slot, anti-rollback, download, boot, and recovery evidence. | M-OTA, M-FLASH-DEBUG; embedded-iot, automotive-embedded, drones-uav |
| D-EH-FIELDBUS1 | Choose industrial and vehicle fieldbus scope, wire contracts, security, timing, and provider boundaries. | M-FIELDBUS; plc-industrial, automotive-embedded |
| D-EH-ROBOTBUS1 | Choose ROS/uORB/MAVLink message, QoS, lifecycle, command, mission, and replay boundaries. | M-ROBOT-BUS; robotics, drones-uav |
| D-EH-HDL1 | Choose the typed HDL semantic layer, RTL/simulation/formal artifacts, synthesis/timing, and programmer bridge. | M-HDL-SEMANTICS, M-FLASH-DEBUG; fpga-hdl |
| D-EH-SAFETY1 | Choose proof, qualification, traceability, safe-state, coverage, WCET, schedulability, and certified-provider evidence. | M-SAFETY-EVIDENCE, M-WCET; automotive-embedded, real-time-safety-critical, plc-industrial |
| D-EH-GAUNTLET1 | Choose real target/emulator benchmark cells and strict peer comparison fields for each workload. | M-WCET and all mechanisms; all domains |

The domain objects list 8 cross-family gate bundles. The per-domain census contains 257 P0 rows marked owner_gate; this is a row count, not a Tower card count.

## Contradictions and defects

| Mechanism or issue | Source 1 | Source 2 | Synthesis |
|---|---|---|---|
| Target dossier versus physical proof | Embedded, FPGA, automotive, safety, and driver reports show typed target/provider/MMIO facts and successful dossier probes (embedded-iot/report.md §Jet today; fpga-hdl/probes/jet_target_dossier.json; hardware-drivers/report.md §Beat vectors). | Every performance object says gauntlet_cell none or no physical witness; embedded #2300 criteria 2–5 and board/flash/debug/power rows remain open (embedded-iot/report.md §Performance; hardware-drivers/performance.json). | Count target truth as shipped or mixed foundation only; do not claim board parity or timing until emulator/physical receipts exist. |
| Generic modules versus domain protocols | Jet ships generic units, sockets, tasks, tables, effects, and target facts (engineering/mechanisms.json; plc-industrial/report.md §Jet today). | The live gap probes reject core.opcua, core.modbus, and core.can with E1001, and core.ros with E1004 (plc-industrial/probes; robotics/probes/ros_gap.out; automotive-embedded/probes/missing_automotive.jet). | Reuse generic substrates but keep fieldbus, robot-bus, and ECU/ROS module gaps explicit. |
| Safety substrate versus qualification | Contracts, unsafe obligations, authority, deterministic checks, and structured evidence are present (real-time-safety-critical/report.md §Beat vectors; automotive-embedded/report.md §Beat vectors). | Automotive and safety rows still require ASIL/ISO 26262 or SPARK/RTOS qualification, WCET/schedulability, target drivers, and traceability bundles; no qualified target result is recorded (automotive-embedded/report.md §Performance; real-time-safety-critical/performance.json). | Call the foundation mixed; gate safety claims on target-bound evidence and certification artifacts. |
| HDL identity discrepancy | The engineering synthesis names existing hardware/DSP mechanisms such as M-HARDWARE-IO and M-DSP-FILTERS (engineering/mechanisms.json). | No prior mechanism file contains an M-HDL-* or M-CIRCUIT-* definition; engineering/synthesis.md line 71 references M-CIRCUIT-SOLVE but the ID is absent from the registry and file. | Mint only M-HDL-SEMANTICS for this family and record the absent/phantom prior IDs instead of claiming reuse. |
| Published incumbent numbers versus Jet results | Zephyr, TwinCAT, Verilator, PX4, NI, seL4, and CAN sources publish samples or device-family numbers (each domain performance.json). | All nine target performance files report no gauntlet cell or no matched dataset; several numbers are explicitly documentation, device-family, or host-replay values (embedded-iot/performance.json; fpga-hdl/performance.json; instrumentation-lab-automation/performance.json). | Retain numbers as fixture leads only; no Jet win is established. |
| Policy diagnostics versus implementation defects | The invalid-target, retired-flag, ROS, OPC UA, Modbus, and CAN probes produce actionable diagnostics (embedded-iot/probes; plc-industrial/probes; robotics/probes; automotive-embedded/probes). | The GPU gate probe correctly returns E2104/E1803 for an unaudited or undecided effect (drones-uav/probes/compute_tensor_gate.txt; compute_tensor_allow_gpu.txt). | Keep expected-negative policy paths as regressions and do not classify them as successful domain implementation. |

See defects.json for E2102, E2105, the OPC UA/Modbus/CAN E1001 rows, the ROS E1004 row, and the expected GPU policy diagnostics. No internal compiler error appeared in the nine domain probe directories.
