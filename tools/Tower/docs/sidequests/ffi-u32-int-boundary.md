# Plan: FFI unsigned integer boundary tests

**Status:** planned. No owner decision required for test coverage.

## Goal

Make the current C/Rust FFI integer mapping explicit and tested, especially `u32`/unsigned
C integer values crossing into Jet `Int`.

## Implementation Steps

1. Inventory current FFI type mapping in `Source/CBind.rs`, `Source/CFFI.rs`, and
   `Source/Codegen`.
2. Add tests for unsigned C declarations mapping to Jet `Int`.
3. Include boundary values around `u32::MAX` when the backend can exercise them.
4. Verify diagnostics teach the limitation if a value cannot be represented safely.
5. Document the current policy in the C FFI sidequest or reference docs.

## Verification

- `nix develop -c cargo test --test cffi -- --nocapture`
- `nix develop -c cargo test --test ui -- --nocapture`
