# Sidequest: E2-M15 — Cross-compilation and freestanding implementation

**Plan:** `docs/plans/epoch-2/m15-freestanding-cross.md`  
**Status:** all decisions ratified; ready to implement after M13 ✅ + M14 ✅  
**Depends on:** E2-M13 ✅ (allocators/low-level tier), E2-M14 ✅ (link config)

## Ratified decisions

| Decision | What to implement |
|---|---|
| D-CROSS1 | `jet build --target <triple>`; first proven target = one CLI target (e.g. `aarch64-linux`) |
| D-CROSS2 | **Abort by default** (`panic=abort`); no unwind tables; smaller binary; right for embedded |
| D-CROSS3 | **Documented local QEMU harness** minimum; no physical hardware required in CI |

## D-CROSS2 implementation note

Freestanding panic strategy is **abort by default**. This means:
- `--freestanding` sets `panic = "abort"` in the generated Rust
- `--small` (S15) also implies `panic = "abort"` per M6 timing decision (D-LIB1)
- Normal hosted Jet builds may still use unwind; document the difference

## D-CROSS3 implementation note

CI embedded smoke test = a documented local QEMU harness. The repo ships:
1. A `docs/embedded.md` guide with exact QEMU commands to run the freestanding example
2. `examples/features/51_freestanding.jet` runs clean under the harness
3. No physical board required for CI to pass — QEMU is sufficient

## Diagnostics to register (E33xx)

E3301 (std-only API in freestanding build), E3302 (unknown target triple / missing toolchain), E3303 (no global allocator in freestanding build that needs one).

## Exit criteria

See `m15-freestanding-cross.md`. Key: one non-host cross target proven end-to-end; freestanding example avoids std APIs with clear diagnostics; `jet doctor` helps with missing target toolchain; QEMU harness documented. `nix develop -c cargo test` green.
