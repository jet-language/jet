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

## Freestanding profile

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
| `core.path` | `core.http` |
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

## Typed target profile facts

Card #239 / D-TARGET-* adds the internal profile model used by the next
embedded slices. Hosted builds keep hidden defaults. A no-OS profile carries
these typed facts before codegen:

- target triple plus `no-os`
- named memory regions: origin, size in bytes/KiB/MiB, kind (`flash`, `ram`,
  `mmio`, `reserved`), access (`r`, `rw`, `rx`, `rwx`)
- linker provenance: generated from profile facts, or a file path with a
  `sha256:` hash
- allocator policy: none, fixed region/size, or hosted default
- panic policy: abort, report sink, or hosted default
- audit requirements for build artifact plus dossier lens

Validation is data-first in this slice. It reports missing flash/RAM,
overlapping or overflowing memory, RAM budget overflow, heap use with no
allocator, hosted Core APIs on a no-OS profile, MMIO outside declared regions,
MMIO without an unsafe audit gate, missing panic policy, and missing linker
provenance. Turning those data errors into new user diagnostics remains gated
on exact diagnostic decisions.

The profile audit JSON is stable and contains memory layout, linker source,
allocator, panic behavior, unavailable Core APIs, and MMIO unsafe reasons. The
ratified user-facing shape is a `jet dossier target` lens plus a build artifact;
CLI/package wiring lands in the later surface slice.

## Running under QEMU (D-CROSS3 local harness)

After a cross-build, run the binary under QEMU user-mode emulation without
a full system image. Install `qemu-user-static` (or `qemu-arch-extra` on
Arch), then:

```sh
# aarch64 example
qemu-aarch64-static ./build/61_freestanding
```

For a bare-metal target that doesn't link std (future work), use QEMU
system-mode:

```sh
qemu-system-aarch64 \
  -machine virt \
  -cpu cortex-a57 \
  -kernel ./build/61_freestanding \
  -nographic
```

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
- Only one target is proven in CI (aarch64-linux per D-CROSS1).
- WASM is deferred to post-epoch (no browser runtime in v1).
- E3303 (missing global allocator in freestanding) is registered. The typed
  profile model now catches the same fact as data; CLI/profile wiring will
  promote it through the user diagnostic path once that surface slice lands.
