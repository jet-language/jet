# Typed Embedded And Freestanding Target Profiles

## Goal

Card #239 turns D-WD11 into an Epoch 3 plan: embedded, freestanding, and no-OS work uses typed target profiles. Normal Jet keeps hidden defaults and no target jargon. When an embedded or kernel target is selected, the profile exposes memory, linker, allocator, panic, volatile/MMIO, and audit controls.

The product promise is one Jet language: a web app never sees board details, while firmware can prove every hidden assumption.

## Current law

- D-WD11 ratifies typed target profiles as the direction.
- E2-M15 already established cross-compilation and freestanding as shipped roadmap territory.
- D-LL1 and S58 require low-level unsafety through `use core.mem` plus `#Unsafe("reason")` or `#Unsafe fn`; volatile/MMIO belongs behind that expert gate.
- R8 says the default v1 path is not `no_std`; freestanding is opt-in, not beginner default.
- R9 says single-file `jet run` stays ceremony-free.

The profile declaration surface, board module shape, memory-unit spelling, linker policy, and build command flags need owner decisions before implementation.

## Vertical slices

1. Profile model: internal typed target profile with target triple, OS absence, allocator policy, panic policy, memory regions, linker inputs, and audit requirements.
2. CLI/profile selection: one ratified way to select an embedded or freestanding target, with normal Jet unchanged.
3. Memory and linker checks: profile validates flash/RAM regions, stack/heap ceilings, entry symbol policy, and linker script provenance before codegen.
4. Allocator and panic policy: no allocator, fixed allocator, and panic abort/report policies are sema-visible facts, not codegen guesses.
5. Volatile/MMIO audit: register access requires `core.mem` and `#Unsafe("reason")`; diagnostics point to the missing gate or missing profile capability.
6. Freestanding Core subset: only target-available Core modules are usable; unavailable modules produce Jet diagnostics with target-specific reasons.
7. QEMU or emulator smoke: a tiny firmware exits or writes a deterministic signal under a pinned emulator when the target supports it.
8. Audit output: build emits a profile audit report showing memory layout, linker source, allocator, panic behavior, unsafe regions, and unavailable Core APIs.

## Acceptance tests

- UI snapshots: target profile missing required memory region, RAM overflow, allocator used when policy is none, panic behavior unspecified for freestanding, `core.files` used on no-OS target, MMIO outside `#Unsafe`, and linker script missing.
- Golden or smoke example: minimal freestanding program builds through the typed profile and produces a deterministic emulator signal where available.
- No-regression test: ordinary `jet run hello.jet` does not require or mention target profiles.
- Audit test: profile build emits stable machine-readable audit output.
- I1 test: generated unsafe for volatile/MMIO appears only inside approved gated regions or vetted internals.
- I6 test: compiler crates remain dependency-free; emulator/toolchain helpers stay outside compiler crates.

## Dependency order

1. Ratify exact profile declaration and selection surface.
2. Add internal profile model and validation.
3. Wire profile facts into sema availability checks.
4. Add memory/linker/allocator/panic diagnostics.
5. Add volatile/MMIO audit gate integration.
6. Define the freestanding Core subset.
7. Add emulator smoke and audit output.

## Owner ballots needed

- D-TARGET-SURFACE1: profile declaration and selection surface, including whether target data lives in source, package metadata, CLI flags, or generated board files.
- D-TARGET-MEMORY1: memory region/unit spelling and validation policy.
- D-TARGET-LINKER1: linker script provenance and override policy.
- D-TARGET-ALLOC1: allocator and panic policy surface.
- D-TARGET-AUDIT1: audit report shape and whether it is command output, build artifact, or dossier lens.

## Adversarial tradeoffs

- Safety first: target profiles must make unsafe hardware assumptions explicit; volatile/MMIO cannot become a shortcut around I1.
- Beginner experience: no target concepts appear until the user selects an embedded/freestanding target.
- Runtime performance: profile facts should remove unused runtime assumptions and Core helpers, not add runtime checks.
- One mechanical path: freestanding is a profile of Jet, not a dialect. Parser and sema stay the same; availability and codegen inputs differ by typed target facts.
- Ecosystem breadth: the profile model must cover microcontrollers, kernels, and no-OS experiments without baking in one board vendor's worldview.
