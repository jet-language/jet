# Games foundations probe

## What I built

I built a package-shaped 2D game probe in `pkg/main.jet`. It has a scene, component, keyboard action, fixed-step accumulator, interpolation, AABB collision, manually parented nodes, raylib drawing, one-shot sound, save/load, and a deterministic 600-iteration replay checksum. It also contains negative fixtures for a configurable replay frame count, gamepads, sprite atlases, audio callbacks/music, and a generated C binding.

Files: `pkg/package.jet`, `pkg/main.jet`, `pkg/assets/player.atlas`, `pkg/assets/jump.wav`, `pkg/gap_*.jet`, `pkg/include/game_probe.h`, `pkg/.jet/bindings/c/game_probe.jet`, `gaps.json`, and `batteries.json`.

## What worked

- Package effects and filesystem: `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/area-games/pkg/main.jet` wrote and read `/home/nate/.cache/jet-luna/dx3/area-games/pkg/save.txt`, printing `save=x=26 paused=false`.
- Fixed timestep and interpolation: the same run printed `frame=0 fixed_x=12 render_x=10` through `frame=7 fixed_x=26 render_x=25`.
- AABB and parent transform library code: it printed `aabb_hits=0` and `scene_child_world_x=120` from ordinary `Body`/`Node` structs and pure helpers.
- Scene/input contract: it printed `scene_input=1`, `input:jump=Space`, `components:Body`, and the stable headless transcript with `frame:0`, `frame:1`, `frame:2`.
- Headless raylib drawing: `window_ready=false key_space=false`; rectangle/text calls completed without a display. `JET_RAYLIB_DISPLAY=1 /home/nate/.cache/jet-luna/dx3/area-games/build/main` also completed with `window_ready=false`, so the documented fallback was exercised.
- One-shot sound surface and pure block DSP: it printed `sound_effect_play=true` and `audio_block_checksum=4032`. This proves handles and ordinary block code, not decoded audio or callback timing.
- Save/load: `core.files` round-tripped the state string without a project manifest beyond `package.jet`.
- Deterministic pure replay check: `scripts/agent/jet-env jet test /home/nate/.cache/jet-luna/dx3/area-games/pkg/main.jet` printed `600 frame replay is deterministic: pass` and `1 passed, 0 failed, 0 skipped`.
- AOT build: `/home/nate/Projects/Github/jet/scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/area-games/pkg/main.jet` completed with `jet Built build/main in 26.9s`; `/home/nate/.cache/jet-luna/dx3/area-games/build/main` reproduced the same output.
- C binding generation: `/home/nate/Projects/Github/jet/scripts/agent/jet-env jet inspect bind /home/nate/.cache/jet-luna/dx3/area-games/pkg/include/game_probe.h --pkg game_probe` printed `bound 3 functions ... -> .jet/bindings/c/game_probe.jet`; the typed caller passed `jet check`.

## Gaps

1. **G1 — `impossible`: configurable headless replay frame budget.** A runner must accept a declared frame count and consume that many replay frames. `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-games/pkg/gap_replay_frames.jet` returned `Error [E0764]: \`run\` has no parameter labelled \`frames\`` and `Fix: \`run\` accepts \`replay\`, \`backend\``. `game.run` in the working run emits only `frame:0`, `frame:1`, and `frame:2`; the 600-loop hash is separate pure code. Shared by deterministic games, embedded loops, and data/simulation replay harnesses.
2. **G2 — `impossible`: typed gamepad buttons and axes.** `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-games/pkg/gap_gamepad.jet` returned `Error [E1004]: \`core.game.raylib\` has no item \`gamepad_down\``; its fixed-item correction lists `key_down` but no controller operation. Keyboard bindings work. Shared by games, embedded control panels, and GUI applications.
3. **G3 — `impossible`: typed texture-atlas decode, named-region lookup, and sprite drawing.** `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-games/pkg/gap_atlas.jet` returned `Error [E1004]: \`core.game.raylib\` has no item \`load_texture_atlas\`` and `Error [E1004]: \`core.game.raylib\` has no item \`draw_sprite\``; the correction list has no texture, atlas, or sprite operation. `scene.assets.image` retains an opaque path and rectangles draw, but the atlas fixture cannot become a sprite. Shared by games, GUI, and web rendering packages.
4. **G4 — `impossible`: sample-accurate audio callback deadline and music-stream lifecycle.** `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-games/pkg/gap_audio.jet` returned `Error [E1001]: There is no core module \`core.audio\``. `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-games/pkg/gap_music.jet` returned `Error [E1004]` for \`load_music\`, \`play_music_stream\`, and \`update_music_stream\`; the public list ends at \`load_sound\`, \`play_sound\`. Shared by games and embedded audio.
5. **G5 — `boilerplate`: one-step native game-library bridge preparation.** The 5-line C header and 6-line generated cache type-check, but `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/area-games/pkg/bridge_check.jet` returned `Error [E0956]: Extern call \`jet_ffi_game_probe_add\` has no prepared bridge`. A custom bridge needs header, generated cache, package link dependency, native archive, and prepared runtime bridge. Shared by any area binding C. This scratch run had no native archive/compiler, so I do not count E0956 as a Jet defect.

## Friction

- The probe's working simulation and hierarchy use 2 state structs (`Body`, `Node`) and 7 small helpers in a 128-line file. They are safe ordinary library code, not missing language primitives.
- The package had to grant `FS`, `GPU`, `IO`, `Mem.Alloc`, and `Panic` in `package.jet`; the compiler reported the required/granted effects and then built successfully. This is explicit authority, not hidden setup.
- The C bridge setup repeats 3 declarations in the header, 3 generated wrappers, and a native link/preparation step. The generated binding is pleasant after setup, but a missing archive leaves the caller type-correct and not runnable.
- `core.game` stores image/sound paths and raylib sound calls return headless handles. The author must supply actual decoding, format validation, and renderer/audio backends rather than receiving those from the asset API.
- No `slow` claim was made: this probe did not compare a Jet workload against an incumbent on the same input.

## Defects

None observed. The negative diagnostics were consistent with the fixed public API and deliberately incomplete bridge setup; no wrong answer, panic, or compiler crash occurred.

## Battery notes

- `package.jet` authority and asset fixtures — verifies a first package can declare effects and carry assets; not library-only.
- `core.game` scene, component/query, input binding, frame hook, and headless transcript — verifies the stable first-party scene contract; not library-only.
- Fixed-step accumulator, interpolation, AABB, and parent transforms — ordinary deterministic library code; library-only.
- Raylib headless window, drawing, keyboard, and one-shot sound smoke — catches renderer fallback and device calls; not library-only.
- Sprite asset with atlas decode, region lookup, and draw — must exercise a real texture path; not library-only.
- 600-frame replay with injected input and deterministic hash — tests frame budget and repeatability; not library-only.
- Audio callback deadline and music stream lifecycle — measures callback-safe processing and updates; not library-only.
- Save-file and pause-menu round trip — tests persistence and pause state; library-only.
- Keyboard/gamepad input matrix — keeps the working keyboard fallback distinct from controller support; not library-only.

## Verdict

Buildable with the listed gaps fixed.
Core Jet can package, type-check, run, test, and AOT-build a deterministic headless 2D skeleton today.
Fixed-step/interpolation/AABB/parent transforms/save/pause/keyboard/one-shot sound are ordinary library code.
The complete target is blocked by scene replay length, gamepads, atlas drawing, and callback/music APIs.
Custom C libraries are possible only with extra native-link and prepared-bridge setup; no speed claim was made.
