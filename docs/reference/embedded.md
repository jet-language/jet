# Embedded and Freestanding Builds (E2-M15)

Jet supports cross-compilation and freestanding (no-OS) builds using rustc's
target matrix. This document covers the local QEMU harness (D-CROSS3 option A).

## Cross-compilation

Build for a non-host target by passing `--target=<triple>` to `jet build`:

```
jet build --target=aarch64-unknown-linux-gnu examples/features/lowlevel/cross.jet
```

Jet passes `--target <triple>` straight through to rustc. Any triple that
rustc knows (run `rustc --print target-list`) is accepted. The standard
library for the target must be installed first:

```
rustup target add aarch64-unknown-linux-gnu
```

Run `jet self doctor --target=aarch64-unknown-linux-gnu` to check whether the
toolchain component is present before building.

## Freestanding machine

The `--freestanding` flag rejects OS-dependent APIs at compile time (E3301)
and compiles with `panic=abort` (D-CROSS2). Only `core`-level modules are
permitted:

| Allowed | Rejected |
|---------|---------|
| `core.math` | `core.files` |
| `core.encoding.json` | `core.io` |
| `core.mem` | `core.net` |
| `core.random` | `core.tasks` |
| `core.env` | `core.time` |
| `Path` values | `core.http` |
| `core.crypto` | `core.log` |
|             | `core.time` |

Example:

```
jet build --freestanding examples/features/lowlevel/freestanding.jet
```

Combine with a cross target:

```
jet build --freestanding --target=aarch64-unknown-linux-gnu 61_freestanding.jet
```

## Typed target machine facts

Card #239 / D-TARGET-* makes freestanding and embedded builds use typed board
machines. Hosted Jet keeps hidden defaults. A selected no-OS machine carries
these facts before codegen and into build artifacts:

- target triple plus `no-os`
- named memory regions: origin, size in bytes/KiB/MiB, kind (`flash`, `ram`,
  `mmio`, `reserved`), access (`r`, `rw`, `rx`, `rwx`)
- linker provenance: generated from machine facts, or a file path with a
  `sha256:` hash
- allocator policy: none, fixed region/size, or hosted default
- panic policy: abort, report sink, or hosted default
- execution honesty: no-OS machines are AOT-only (`dev` / `jit` rejected)
- audit requirements for build artifact plus dossier lens

Validation is data-first. It reports missing flash/RAM, overlapping or
overflowing memory, RAM budget overflow, heap use with no allocator, hosted
Core APIs on a no-OS machine, MMIO outside declared regions, MMIO without an
unsafe audit gate, missing panic policy, and missing linker provenance.

### Real firmware artifacts

Selecting a typed machine builds deterministic firmware under
`.jet/target/<name>/` (tests use a temp dir):

- `memory.ld` — generated linker script from memory regions
- `startup.c` / `startup.S` — reset or `_start` for the triple
- `firmware.elf` — linked image (`clang` + `ld.lld`)
- `firmware.map` — linker map
- `<name>.target.json` — stable audit JSON plus size/budget fields

Representative boards:

| Machine | Triple | Proof |
|---------|--------|-------|
| `board.sensor_v1` | `thumbv7em-none-eabihf` | MCU ELF + map + audit + flash budget |
| `board.virt_aarch64` | `aarch64-unknown-none` | QEMU `virt` boots and prints `OK` |

```sh
jet inspect dossier target board.sensor_v1
jet inspect dossier target board.virt_aarch64 --json
```

Hostile machines fail closed (overlap, missing panic, heap without allocator,
MMIO outside regions). Unsupported Dev/JIT paths return
`ExecutionTierUnsupported` for no-OS machines.

## Running under QEMU (D-CROSS3 local harness)

After a cross-build, run the binary under QEMU user-mode emulation without
a full system image. Install `qemu-user-static` (or `qemu-arch-extra` on
Arch), then:

```sh
# aarch64 example
qemu-aarch64-static ./build/61_freestanding
```

For a typed no-OS machine, QEMU system-mode is the live proof path. The
`board.virt_aarch64` machine builds `firmware.elf` and boots under:

```sh
qemu-system-aarch64 \
  -machine virt \
  -cpu cortex-a57 \
  -kernel .jet/target/board.virt_aarch64/firmware.elf \
  -nographic
```

The smoke harness expects the UART to print `OK` (see `tests/target_machines.rs`).

## Checking target availability

```
jet self doctor --target=aarch64-unknown-linux-gnu
```

Adds a `cross` section to the doctor report:
- Confirms the target is in `rustc --print target-list`
- Checks whether `rustup target add` has installed the std library
- Reports `ok` or `warn` with the fix command

## Caveats (v1)

- No board-support package or HAL library (use the low-level tier directly).
- Jet module spelling for board machines (`module board.sensor_v1 { … }` per
  D-TARGET-SURFACE1) selects through typed machine builders today; package
  `targets: { machine: … }` wiring stays on the existing `targets:` surface.
- Machine validation errors remain data (`TargetMachineError`) until a follow-up
  diagnostic ballot promotes them to registered codes.
- WASM is deferred to post-epoch (no browser runtime in v1).
- E3303 (missing global allocator in freestanding) is registered. The typed
  machine model catches the same fact as data.
