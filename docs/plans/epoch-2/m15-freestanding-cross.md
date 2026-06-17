# E2-M15 — Cross-compilation and freestanding profile

**Status:** draft — **blocked on D-CROSS1…D-CROSS3** (Group M15).
**Depends on:** E2-M13 (allocator/low-level tier), E2-M14 (link configuration).
rustc provides the target matrix nearly free (owner-todo §6).
**Error codes:** E33xx block (claim in docs/spec/diagnostics.md).

## Goal

Avoid painting Jet out of embedded, kernel, and constrained targets. A
cross-compiled CLI artifact works for at least one non-host target, and a
freestanding profile avoids std-dependent APIs with clear diagnostics — while
the low-level tier stays gated and never leaks into normal Jet.

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-CROSS1 | First non-host target | **A** — one CLI target (e.g. `aarch64-linux`) | A | ✅ ratified 2026-06-16 — A: first cross target = one CLI target (e.g. aarch64-linux) |
| D-CROSS2 | Freestanding panic strategy | **A** — abort default | A | ✅ ratified 2026-06-17 — A: abort by default (no unwind tables; smaller binary; right for embedded) |
| D-CROSS3 | Embedded smoke | **A** — documented local harness minimum | A | ✅ ratified 2026-06-17 — A: documented local QEMU harness; no physical hardware required in CI |

## Scope

- **`jet build --target <triple>`** — inherit rustc's target matrix; one
  non-host target proven end-to-end (D-CROSS1).
- **Target detection + `jet doctor` support** — doctor reports whether a target's
  toolchain components are installed and how to add them.
- **`--freestanding` / `--no-std`-class profile** using Rust `core` where
  possible; std-dependent APIs are rejected with a clear diagnostic naming the
  alternative.
- **Allocator story** tied to E2-M13 (explicit/arena/fixed allocators in
  freestanding mode).
- **Panic strategy + size profiles (D-CROSS2)** — abort by default; binary-size
  profiles (e.g. `--small` from S15) documented.
- **Embedded/freestanding smoke (D-CROSS3)** — a minimal target in CI or a
  documented local harness with the minimum hardware/emulator requirement.

## Freestanding diagnostic (example)

```
error[E3301]: `fs.read` is not available in a freestanding build
  --> sensor.jet:12
   |
12 |     val cfg = fs.read("cfg.toml")?;
   |               ^^^^^^^ this needs the operating system's file API
why: `--freestanding` targets have no OS; only `core`-level APIs are available.
fix: embed the config at compile time, or build without `--freestanding`.
```

## Diagnostics to register

- **E3301** std-only API used in a freestanding build (names the API + fix).
- **E3302** target triple unknown / toolchain component missing (points at
  `jet doctor`).
- **E3303** no global allocator configured in a freestanding build that needs one.

## Examples & tests

- `examples/features/50_cross.jet` — a small CLI cross-built for the chosen
  non-host target (artifact runs under emulation in CI or the documented harness).
- `examples/features/51_freestanding.jet` — a `--freestanding` program that
  avoids std APIs.
- ui fixtures for E3301–E3303.

## Out of scope

- A board support package or HAL library (point at the low-level tier instead).
- A full embedded target matrix (one target proves the path).
- Bootloaders / linker-script authoring tooling.
- WASM target (revisit post-epoch; relevant to a future playground).

## Exit criteria

- A cross-compiled CLI artifact works for at least one non-host target.
- A freestanding smoke example avoids std-dependent APIs with clear diagnostics.
- The low-level tier enables the demo without leaking into normal Jet.
- `jet doctor` helps diagnose a missing target toolchain.
- `nix develop -c cargo test` green.
