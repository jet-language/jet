# raylib_hello — Jet + raylib through the native C binder

Opens an 800×450 raylib window and animates a frame loop at 60 FPS until the
window is closed. Binds raylib with Jet's native C binder (D-CBIND3,
`Source/CBind.rs`) — no `unsafe` in `main.jet`.

## What it draws

Every frame, against a dark-navy background:

- A 60×60 rectangle bouncing horizontally (triangle-wave of the frame counter),
  its color cycling each frame.
- A circle bouncing vertically on the right edge, also color-cycling.
- `hello, world` in large RAYWHITE text, plus a smaller caption line.

Motion happens every frame and the text is fixed and legible — a real raylib
scene, not an empty window.

## Run it

```
jet run examples/graphics/raylib_hello
```

That's it — no `PKG_CONFIG_PATH`, no manual `nix build`, no `nix shell`. The
directory argument is treated as a project root (`main.jet` is the entry).

`pkg.jet` declares `raylib: c@system`. On the first build Jet resolves raylib:
host `pkg-config raylib` if present, otherwise it auto-fetches `nixpkgs#raylib`,
caches the store path under `.jet/clinks/raylib`, and bakes that lib dir into the
binary's rpath so `libraylib.so.600` is found at run time with no
`LD_LIBRARY_PATH`. The fetch happens once; later builds reuse the cache.

Use `jet build examples/graphics/raylib_hello` to compile + link without opening
the window. The binder writes its cache to `.jet/bindings/c/raylib.jet` on first
build (regenerated when `include/raylib.h` changes).

To pin a specific nixpkgs attribute (when the link name differs from the attr),
write `raylib: c@nixpkgs:raylib`. The link key is the dep name (`raylib`),
matching the bound header's basename (`raylib.h`).

## Color-as-int ABI note

raylib's draw calls take a by-value `Color` struct = `{ unsigned char r, g, b,
a; }` (exactly 4 bytes). The binder maps scalars, `bool`, `char*`, and `void` —
not by-value structs — so those prototypes would be dropped.

`include/raylib.h` instead declares the color parameter as `int`. This is
**ABI-correct, not a hack**: a 4-byte struct of `unsigned char` is classified
INTEGER and passed in one general-purpose register on both x86-64 SysV and
AArch64 AAPCS — bit-identical to passing a `uint32_t`. `main.jet` packs the color
little-endian as `r | (g<<8) | (b<<16) | (a<<24)`, matching the in-memory byte
order of the struct's fields (e.g. RAYWHITE `{245,245,245,255}` →
`245 | 245<<8 | 245<<16 | 255<<24`). The generated call is the real raylib draw
call with the correct register contents.

All bound functions — `InitWindow`, `SetTargetFPS`, `BeginDrawing`, `EndDrawing`,
`CloseWindow`, `WindowShouldClose`, `GetTime`, `ClearBackground`, `DrawText`,
`DrawRectangle`, `DrawCircle` — bind and link cleanly against real raylib 6.0
(`ldd build/main` shows `libraylib.so.600`).

## Not a feature example (no golden test)

This is a GUI program — it opens a window and never writes a fixed stdout, so it
can't be stdout-golden-tested. The golden runner (`tests/golden.rs`) executes
every entry under `examples/features/`; a window there would hang it. That's why
this lives under `examples/graphics/` instead.
