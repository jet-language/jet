# Probe area-embedded — Embedded and hardware

Read `~/.cache/jet-luna/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: embedded.

## Build this

A firmware program as one Jet package for a Cortex-M target (cross build; run what you can under an emulator or host replay): a board description, GPIO and timer MMIO with typed registers, an interrupt handler with bounded handoff to a task, a DMA transfer with ownership of the buffer, a UART driver with a bounded ring buffer, a deadline-checked control loop with a WCET estimate, a flash image with a signed slot and a rollback rule, and a debug/flash receipt. Start from examples/features/lowlevel/** (freestanding, cross, mmio_board_write, target_machine_board, inline_asm, layout_c, unsafe_obligations), examples/features/memory/**, examples/features/safety/**.

## Answer these

1. Which of interrupts, DMA ownership, deadlines/WCET, and flash/OTA need the compiler or runtime, and which are library code over MMIO?
2. What does the memory model give a driver author for ownership of DMA buffers?
3. What does an embedded battery need?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/embedded-iot/`, `~/.cache/jet-luna/dx2/hardware-drivers/`, `~/.cache/jet-luna/dx2/real-time-safety-critical/`, `~/.cache/jet-luna/dx2/automotive-embedded/`, `~/.cache/jet-luna/dx2/robotics/`, `~/.cache/jet-luna/dx2/plc-industrial/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/area-embedded/probe.md`, `gaps.json`, `batteries.json`, and the code under `~/.cache/jet-luna/dx3/area-embedded/pkg/`. Gap ids start with `area-embedded-G`.
