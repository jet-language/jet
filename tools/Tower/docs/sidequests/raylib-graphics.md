# Raylib graphics package

**Card:** c60 / coivmgi. **Decision:** D-RAYLIB1=A. **Status:** ready to build.

## Goal

Ship an official opt-in `core.raylib` package so Jet can open a window, draw 2D/3D,
handle input/audio, and support flagship game/visual demos without user-written C FFI.
This is a package/bridge, not compiler surface.

## Constraints

- I6 holds: no new dependency in compiler `Source/`; use the existing stdlib FFI-bridge
  pattern.
- Beginner API wraps unsafe C handles in RAII values (`Window`, `Texture`, `Sound`) with
  checked errors.
- Nix path uses `pkgs.raylib`; non-Nix path vendors/builds raylib source and records the
  hash in `.jet/lock`.
- Future own renderer remains possible; this package is a practical graphics bridge now.

## Build Plan

1. Add package metadata for `core.raylib`, dependency sourcing, and lock hashing.
2. Generate C bridge bindings for window lifecycle, drawing, texture load/free, input,
   audio, timing, and error conversion.
3. Expose Jet wrappers: `Window.open`, `should_close`, `draw.begin/end`, `draw.text`,
   `draw.texture`, `Color`, keyboard/mouse input, basic sound.
4. Add examples: hello window, moving sprite, input demo, audio demo.
5. Add a flagship vertical slice under `examples/showcase/` once the small examples pass.

## Verification

- Golden tests for examples that can run headless; use a headless/no-window mode where
  raylib supports it.
- Build tests on Linux in Nix; document platform smoke requirements for macOS/Windows.
- Ensure `cargo tree -p jet` remains free of raylib dependencies.

