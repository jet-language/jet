# Embedded foundations probe

## What I built
I built one host-replayable Jet package for a `sensor_v1` Cortex-M board description and a `thumbv7em-none-eabihf` target. The program models C-layout GPIO, timer, UART, and DMA registers, a bounded interrupt queue and UART ring, a pinned DMA buffer, a control tick, and trial/confirmed/rejected image slots. Separate fixtures probe fixed-address MMIO, cryptographic signing, local-module execution, and the absent interrupt, DMA, WCET, flash, and OTA surfaces.

Files under `pkg/`: `package.jet`, `run.jet`, `mmio_fixed.jet`, `signature_crypto.jet`, `module_replay.jet`, and the five `missing_*.jet` fixtures.

## What worked

- **Package checking:** works. `JET_STORE_DIR=... scripts/agent/jet-env jet check .../run.jet` exited zero with `check: passed` and `diagnostics=52` (all warnings).
- **Native artifact:** works. `JET_STORE_DIR=... scripts/agent/jet-env jet build .../run.jet` ended `jet Built build/run in 25.0s ✓`; the build also reported the granted `IO, Mem.Alloc` effects.
- **Board description:** works as ordinary library data. Host output was `board sensor_v1 thumbv7em-none-eabihf`.
- **Typed register model:** works with `#Layout(c)` records. Host output was `gpio mode 1 output 1 mmio 1`; fixed MMIO code also checked and built with the explicit unsafe gate.
- **Timer and bounded handoff:** works as library code. Host output was `timer status 1 interrupts 4 dropped 0`; the loop generated four timer events and drained them after the ISR-shaped queue write.
- **UART ring:** works as a fixed-capacity library type. Host output was `uart first 65 queued 3 dropped 0`.
- **Pinning and replay bookkeeping:** works. Host output was `dma submitted true completed true owner 0 checksum 100`; `mem.pin(&buffer)` is accepted, but the owner transitions are user fields, not DMA runtime facts.
- **Control loop:** works as deterministic library code. Host output was `control wcet_estimate_us 12 deadline_us 100 misses 0`; this is an integer estimate and comparison, not a compiler WCET proof.
- **Signing API:** works in the standalone host and native build. `jet run .../signature_crypto.jet` printed `verified true`; `jet build .../signature_crypto.jet` ended `jet Built build/signature_crypto in 10.4s ✓`.
- **Slot policy:** works as an in-memory state machine. The main replay printed `flash signature true active 100 rollback 1`, exercising acceptance and a rejected trial image.
- **Unsafe audit:** works. `jet inspect unsafe .../run.jet` reported one gate and discharged `pointer_from_address`, `volatile_write`, and `volatile_read` with the required validity/alignment/no-alias obligations.

Representative successful output:

```text
board sensor_v1 thumbv7em-none-eabihf
gpio mode 1 output 1 mmio 1
timer status 1 interrupts 4 dropped 0
uart first 65 queued 3 dropped 0
dma submitted true completed true owner 0 checksum 100
control wcet_estimate_us 12 deadline_us 100 misses 0
flash signature true active 100 rollback 1
```

## Gaps

1. **area-embedded-G1 — `impossible`:** A usable freestanding Cortex-M target backend and toolchain path is missing in this environment. `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet-env jet build --target=board.sensor_v1 /home/nate/.cache/jet-luna/dx3/area-embedded/pkg/run.jet` returned `Error [E3302]: Target \`thumbv7em-none-eabihf\` is not available`; `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet-env jet self doctor --target=thumbv7em-none-eabihf` reported the target-specific standard-library component missing. This blocks embedded and is relevant to any target-bound area. A library author can install the rustc target component outside Jet, but this probe did not do so.

2. **area-embedded-G2 — `impossible`:** There is no compiler/runtime binding from a Cortex-M vector table to a bounded ISR and task handoff. `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-embedded/pkg/missing_interrupt.jet` returned `Error [E1004]: \`core\` has no item \`interrupt\``. `core.sys.on_interrupt` is a process-lifetime Ctrl-C hook, not a hardware vector API. The workaround is target-specific startup/vector FFI inside an audited unsafe boundary. Shared with hardware drivers, real-time safety, automotive, robotics, and PLC work.

3. **area-embedded-G4 — `impossible`, `call-site`:** There is no compiler-enforced target-aware WCET and hard-deadline contract. `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-embedded/pkg/missing_wcet.jet` returned `Error [E1004]: \`core\` has no item \`wcet\``; `jet inspect facts` lists `Target.Memory`, `Target.MMIO`, `Target.Startup`, `Time.Monotonic`, `Time.Sleep`, and `Target.Scheduler`, but no WCET fact. The probe must carry `wcet_estimate_us`, compare it manually, and accept `L2510 (hidden_cost_in_loop)` warnings about exact-`Int` cost. Shared with real-time safety, automotive, robotics, and PLC work.

4. **area-embedded-G5 — `impossible`, `unsafe`:** There is no target-backed flash storage or authenticated A/B update state with persistent rollback. `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-embedded/pkg/missing_flash.jet` returned `Error [E1004]: \`core\` has no item \`flash\``; `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-embedded/pkg/missing_ota.jet` returned `Error [E1004]: \`core\` has no item \`ota\``; `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet-env jet flash --help` returned `Error [E2101]: \`flash\` isn't a jet command`. The slot state and rollback count in `run.jet` are only heap values. The workaround is a board bootloader or external updater through target FFI.

5. **area-embedded-G6 — `impossible`, `boilerplate`:** There is no hardware debug/programming transport that flashes a target and emits a device receipt. `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet debug --help` exposes source frames, DAP, and replay only; `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet flash --help` returns E2101. Native `jet build` emits `Built build/run`, but performs no device operation or hardware receipt. A library author must invoke probe-rs, OpenOCD, or a vendor tool and carry its result manually. Shared with hardware drivers, automotive, robotics, and PLC work.

6. **area-embedded-G3 — `unsafe`, `boilerplate`:** There is no DMA mapping and ownership transfer primitive that validates buffer lifetime, addressability, and completion. `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-embedded/pkg/missing_dma.jet` returned `Error [E1004]: \`core\` has no item \`dma\``; `run.jet` hand-codes four DMA registers plus `owner`, `submitted`, and `completed` flags. `mem.pin(&buffer)` supplies address stability only. The workaround is raw target registers and a hand-written unsafe ownership protocol. Shared with hardware drivers, real-time safety, robotics, and automotive.

7. **area-embedded-G7 — `unsafe`, `boilerplate`:** There is no target-profile-backed typed MMIO binding for named register blocks, widths, and access modes. `run.jet` manually declares four `#Layout(c)` records and four base addresses; `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-embedded/store scripts/agent/jet-env jet inspect unsafe /home/nate/.cache/jet-luna/dx3/area-embedded/pkg/mmio_fixed.jet` reports `pointer_from_address` and volatile operations discharged only within one `#Unsafe` gate. The workaround is to repeat those records, constants, and obligations per board. Shared with all target-bound hardware areas.

## Friction

- The author wrote four register records with 12 fields, four peripheral base constants, and all access wrappers by hand. This is library boilerplate, not a missing ability to represent data.
- A package using host I/O and allocation needs `authority: { holds: { allow: [IO, Mem.Alloc] } }`; without it the first host run stopped at E1803. Adding the manifest entry made the run and build proceed.
- `jet check` and default `jet run` produce 52 warnings for this small package, including repeated `L0505 (heap_growth_in_loop)` at plain type declarations and `L2510` hidden-cost warnings. They do not block the successful host result, but they obscure the timing signal.
- Default interpreted `jet run .../module_replay.jet` reached `E0956` for the exported crypto helper, while `jet run --release .../module_replay.jet` printed `module verified true`. The diagnostic is accurate about that evaluator tier; the release/build workaround is available.
- No speed claim is made. No physical board, flash probe, hardware debugger, or emulator run was possible after E3302 target admission failed.

## Defects

No wrong result, ICE, or runtime crash was reproduced in the successful paths. The only negative runtime observation was the documented default-evaluator E0956 for a cross-module crypto call; the same helper succeeds in the release tier and standalone host run.

## Battery notes

- `board description and target dossier`: name MCU, memory/linker facts, peripheral bases, and host-replay substitutions. **Pure library code.**
- `typed MMIO register block`: exercise C layout, widths, volatile access, and one explicit unsafe audit. **Pure library code.**
- `bounded interrupt-to-task handoff`: show a short ISR queue, overflow policy, task drain, and the vector-table boundary. **Needs runtime/target support.**
- `DMA owned-buffer transfer`: show pinning, submission/completion, reuse rules, and the mapping boundary. **Needs runtime/target support.**
- `UART bounded ring`: fixed-capacity receive/transmit behavior with full/empty cases. **Pure library code.**
- `deadline-checked control loop`: deterministic tick, deadline accounting, and an explicit non-WCET caveat. **Needs timing/compiler support for hard proof.**
- `signed slot policy and rollback`: image acceptance, trial confirmation, rejection, and rollback, separate from persistent flash. **Policy is pure library code.**
- `flash/debug receipt path`: target programming, device observation, and a reproducible release receipt. **Needs toolchain/runtime/device support.**

## Verdict

Host-native embedded-shaped code is buildable today: check, run, crypto, unsafe inspection, and native build all succeeded.

The language can express board records, C layout, bounded queues/rings, pinning, and policy state machines as library code.

A Cortex-M firmware deliverable is not buildable in this environment: board target admission fails E3302 and hardware vector, DMA, WCET, flash/OTA, and programming primitives are absent.

The highest-value shared primitives are usable freestanding target support, hardware IRQ/DMA ownership, hard timing analysis, and target flash/debug receipts.

The output is a host replay, not physical-board or emulator evidence.
