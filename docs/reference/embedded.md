# Embedded and Freestanding Builds (E2-M15)

Jet supports cross-compilation and freestanding (no-OS) builds using rustc's
target matrix. This document covers the local QEMU harness (D-CROSS3 option A).

## Cross-compilation

Build for a non-host target by passing `--target=<triple>` to `jet build`:

```
jet build --target=aarch64-unknown-linux-gnu examples/features/60_cross.jet
```

Jet passes `--target <triple>` straight through to rustc. Any triple that
rustc knows (run `rustc --print target-list`) is accepted. The standard
library for the target must be installed first:

```
rustup target add aarch64-unknown-linux-gnu
```

Run `jet doctor --target=aarch64-unknown-linux-gnu` to check whether the
toolchain component is present before building.

## Freestanding profile

The `--freestanding` flag rejects OS-dependent APIs at compile time (E3301)
and compiles with `panic=abort` (D-CROSS2). Only `core`-level modules are
permitted:

| Allowed | Rejected |
|---------|---------|
| `core.math` | `core.fs` |
| `core.json` | `core.io` |
| `core.mem` | `core.net` |
| `core.random` | `core.tasks` |
| `core.env` | `core.time` |
| `core.path` | `jet.http` |
| `jet.crypto` | `jet.log` |
|             | `jet.time` |

Example:

```
jet build --freestanding examples/features/61_freestanding.jet
```

Combine with a cross target:

```
jet build --freestanding --target=aarch64-unknown-linux-gnu 61_freestanding.jet
```

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
jet doctor --target=aarch64-unknown-linux-gnu
```

Adds a `cross` section to the doctor report:
- Confirms the target is in `rustc --print target-list`
- Checks whether `rustup target add` has installed the std library
- Reports `ok` or `warn` with the fix command

## Caveats (v1)

- No board-support package or HAL library (use the low-level tier directly).
- Only one target is proven in CI (aarch64-linux per D-CROSS1).
- WASM is deferred to post-epoch (no browser runtime in v1).
- E3303 (missing global allocator in freestanding) is registered but not yet
  emitted automatically; configure an allocator manually with `core.mem`.
