# Native raylib: a Jet game library that beats raylib (#5086)

Status: design note, 2026-10-30. Owner goal: our own better raylib, written in
Jet, with raylib's immediate-mode feel and Jet idioms on top. The owner's answer
on D-RAYLIB-FONT-HOME1 also asks for a C raylib binding that the C binder
generates, as a showcase of that binder. Section 6 lists the open ballots.
Tower owns the card plan.

Package name (owner answer 2026-11-01): Jet's own library is the package
`raylib`, imported as `ray`: `use ray :: raylib` (#6542). Below,
`ray` names that library as code refers to it. The C binding in the first row
of section 1 therefore needs another package name; the owner has not given one.

## 1. Three pieces, three homes

| Piece | What it is | Home |
|---|---|---|
| C binding (name not yet given) | The real C raylib, bound by the C binder from `raylib.h`. It needs raylib installed. | First-party package |
| `raylib`, imported as `ray` | Jet's own library: windows, shapes, textures, text, input and audio mixing, all in Jet. | First-party package |
| `font` | Jet's own font engine: reads TrueType and OpenType, rasterizes glyphs, shapes text. `ray` and `gui` use it. | First-party package |

All three ship with Jet (D-JPK-RINGSHIP1), and `jet add` writes the dependency
line. Today's `core.game.raylib` and `core.font` retire once `ray` and `font`
reach stage 3. Until then, `core.game.raylib` keeps its names and draws through
the stage 1 rasterizer below.

## 2. Module layout of `ray`

| Module | Job |
|---|---|
| `ray` | `Window`, `Frame`, `Color`, `Key`, `MouseButton`, `GamepadButton`, `Vector2`, `Rectangle` |
| `ray.raster` | the software rasterizer: clipping, spans, alpha blending, shapes, blits |
| `ray.image` | `Image`, PNG/QOI/BMP decoding (inflate from `core.archive`), `checksum()` |
| `ray.text` | the built-in bitmap font and the glyph cache over `font` |
| `ray.input` | per-frame key, mouse and gamepad state |
| `ray.audio` | the mixer, `Sound`, `Music`, WAV and QOA decoding |
| `ray.platform` | the display, input and sound protocol clients (section 3) |
| `ray.gpu` | stage 5: the Vulkan renderer, opt-in |

Drawing never touches the platform. A `Frame` is an RGBA framebuffer plus a
clip rectangle. The platform only presents finished frames and delivers input
and sound.

## 3. Below the platform line

A static musl Jet program cannot load C libraries (#5086's open question), so
`ray` speaks the protocols itself. D-RAYLIB-DRIVER-LOAD1 (revised) asks the
owner to ratify this.

- **Windows.** On Wayland, `ray` connects to `$WAYLAND_DISPLAY` and shares a
  memfd RGBA buffer through `wl_shm`. It opens the window with `xdg-shell`. On
  X11 it connects to the `$DISPLAY` socket and presents with MIT-SHM, falling
  back to `PutImage`. When the compositor offers no server-side title bar (as
  on GNOME), `ray` uses the X11 path through XWayland.
- **Input.** Keys and the mouse arrive as display-server events. `Key` values
  are physical key positions, taken from the kernel key code that both servers
  send. Typed characters read the XKB keymap the server hands over. Gamepads
  are read from `/dev/input/event*` with evdev.
- **Sound.** `ray.audio` mixes in Jet and streams 16-bit PCM to the PulseAudio
  native socket, which PipeWire serves on current distros. With no sound
  server it writes to the kernel ALSA PCM device through ioctl calls, as
  tinyalsa does.
- **Core doors.** These clients need Unix sockets with fd passing, `memfd`,
  `mmap`, `ioctl` and `poll`. They come from the Core platform layer of #5035
  as audited calls, not as private runtime powers (D-EXTEND-ARCH1). Protocol
  encoding and everything above it is ordinary Jet.
- **GPU (stage 5).** Mesa and NVIDIA drivers are glibc shared libraries and
  cannot load into a static musl process. A program that opts into GPU
  drawing builds as a dynamic executable and loads `libvulkan.so.1` through
  the system loader. Software drawing stays the default and stays static.

## 4. Above the platform line: software first

The rasterizer comes first because it makes headless pixels exact. Every tier
(o0, o1, o2) and every host runs the same integer code, so a golden can assert
a frame's checksum.

- Pixels are 8-bit RGBA, straight alpha. Blending is source-over in integers:
  `(src * a + dst * (255 - a) + 127) / 255` per channel.
- Rectangles clip to the frame, then fill row spans.
- Lines draw both endpoints. The longer axis steps one pixel at a time and the
  other coordinate is the exact line value rounded half up, which is the
  pixel set Bresenham's algorithm walks. Circles fill every pixel within the
  radius, one span per row.
- Triangles use edge functions with the top-left fill rule, so shared edges
  are drawn exactly once.
- Textured blits copy or blend source rows, with clipping on both sides. PNG
  decoding is Jet code: zlib through `core.archive`, the five scanline
  filters, Adam7, palettes, `tRNS` and every bit depth.
- Text uses a built-in 5x7 bitmap font for ASCII, scaled by whole pixels, until
  `font` lands. Then `draw_text_ex` takes a `font.Face`.

The GPU path (stage 5) renders the same draw list with Vulkan. Goldens keep
using the software path, so they stay exact everywhere.

## 5. The API against raylib's

`ray` keeps every raylib function name in snake_case and its argument order
(card 5086 asks for 1:1). D-RAY-API1 asks the owner to ratify this shape.

```jet
use ray :: raylib

win :: ray.init_window(800, 450, "demo")?
loop frame in win.frames() {
    frame.clear_background(.RayWhite)
    if win.is_key_down(.Space) -> frame.draw_rectangle(10, 10, 50, 50, .Red)
}
```

Where it beats raylib:

| Raylib | `ray` |
|---|---|
| One hidden global window | `Window` and `Frame` values; two windows can coexist |
| `int` keys and colors; a wrong key compiles | `Key`, `MouseButton`, `GamepadButton` enums; `Color` with named constants |
| Any call can touch the GPU | Drawing on a `Frame` has no effect; opening, presenting, input and sound carry `GPU`, file loads add `FS` |
| Screenshots need a real window and a GPU | `ray.headless_window(w, h)` draws the same pixels into memory; `win.load_image_from_screen().checksum()` is exact on every tier |
| Needs a C toolchain and system libraries | One static file; nothing to install |
| stb_truetype text with no shaping | The `font` engine: kerning, ligatures, then complex scripts |

The rule for porting is mechanical. Draw calls take `frame.`, window and input
calls take `win.`, and loaders stay on `ray.`.

## 6. Stages and their proof

| Stage | Delivers | Proof |
|---|---|---|
| 1 | The software rasterizer as internal Core code behind `core.game.raylib`'s names, on the headless path | Core `#Test` rows assert exact framebuffer checksums; game goldens on o0, o1, o2 |
| 2 | The `raylib` package (imported as `ray`), headless: `Window`, `Frame`, the shapes, images, text, `load_image_from_screen` | Goldens with frame checksums on o0, o1, o2 |
| 3 | The platform clients: Wayland, X11, evdev, PulseAudio, ALSA; `core.game.raylib` retires | A window opens on Wayland and on X11 from one static binary; a recorded input replay drives a golden |
| 4 | The `font` package: outlines, anti-aliased glyphs, kerning, ligatures; then complex-script shaping | Glyph checksums; HarfBuzz's own shaping test data |
| 5 | The opt-in Vulkan renderer | Its frames match the software frames within a stated tolerance; the software goldens stay exact |
| 6 | The generated C raylib binding package (name not yet given) | The C binder builds it from `raylib.h`; a raylib example runs where raylib is installed |

Open owner ballots:

- D-RAYLIB-FONT-HOME1 (revised): the three homes above, and our own font
  engine instead of a FreeType or HarfBuzz binding.
- D-RAY-API1: raylib's names on `Window` and `Frame` values, typed colors and
  keys, and headless capture.
- D-RAYLIB-DRIVER-LOAD1 (revised): speak the display and sound protocols in
  Jet; GPU builds are a later opt-in.

## 7. Where the code stands

`core.game.raylib` already draws through the stage 1 rasterizer and has taken
the headless parts of stage 2 that need no new names: `draw_line`,
`draw_circle`, `draw_triangle`, PNG textures for `load_texture_atlas`, and a
real `draw_sprite` blit. Two goldens pin frame checksums on o0, o1 and o2:
`game/raylib_sprites` (shapes and sprites from a palette Adam7 PNG and an RGBA
PNG) and `game/raylib_indexed` (#5035's 320x200 indexed screen with a cycling
256-entry palette at 35 Hz, steered by a replayed key log).

The rasterizer path makes no foreign calls. Every host call it reaches is a
raw Linux syscall issued by the runtime:

| Call | Syscalls | Reached from |
|---|---|---|
| Canvas and texture memory | the runtime allocator | `begin_drawing`, `load_texture_atlas` |
| Atlas metadata | `open`, `read`, `close` | `load_texture_atlas` |
| PNG bytes | `open`, `read`, `close` through `core.files.read_bytes` | `load_texture_atlas` |
| `JET_RAYLIB_CAPTURE` | the startup environment, then `open`, `write`, `close` | `end_drawing` |

Two of these can still move up into Jet. The atlas metadata parse can become
Core code over `core.files.read`, since `load_texture_atlas` already carries
`FS`. The capture writer stays in the runtime until `end_drawing` may carry
`FS` or `ray` replaces it with `load_image_from_screen`; both are API changes
that D-RAY-API1 decides.
