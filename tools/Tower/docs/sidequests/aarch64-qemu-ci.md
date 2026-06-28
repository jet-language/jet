# Real aarch64/QEMU Freestanding CI

**Card:** c4 / c57. **Status:** ready to build. **Decision:** none needed.

## Goal

Turn the existing local freestanding/QEMU harness into a real CI job for
aarch64, so freestanding support is continuously verified on an emulator rather
than documented as a local-only check.

## Build Plan

1. Inventory the existing freestanding tests and target flags.
2. Add a CI workflow job that enters the Nix shell and installs/uses the pinned
   QEMU target package from the flake.
3. Build the freestanding example for aarch64.
4. Boot/run it under QEMU with a short timeout and assert the expected serial
   output or exit marker.
5. Keep the allocator-config E3303 variant as a follow-up only if the current
   target image lacks the config seam.
6. Document local reproduction command in the plan and release docs.

## Verification

- `nix develop -c cargo test --test cross`
- `nix flake check`
- CI job dry-run locally where possible through the exact Nix command.

