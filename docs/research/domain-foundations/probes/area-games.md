# Probe area-games — Games

Read `docs/research/domain-foundations/probes/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: games.

## Build this

A small 2D game as one Jet package: a window with a fixed-timestep loop and interpolation, keyboard and gamepad input, sprite atlas loading and drawing, a simple physics step (AABB), sound effects and one music track with a sample-accurate audio callback, a scene graph with parenting, a save file, a pause menu, and a headless test that replays 600 frames deterministically. Start from examples/features/game/**, examples/features/ui/**, examples/features/lowlevel/ffi.jet and cbind, examples/features/io/files.jet. If a raylib bridge exists, use it; otherwise record the bridge cost.

## Answer these

1. Is the frame loop, the audio callback deadline, and the asset pipeline expressible without the compiler?
2. Where does the FFI bridge to a C game library cost the author (boilerplate counts)?
3. What does a first-party games battery need?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/rendering-graphics/`, `~/.cache/jet-luna/dx2/creative-coding/`, `~/.cache/jet-luna/dx2/audio-music-production/`, `~/.cache/jet-luna/dx2/ar-vr-xr/`, `~/.cache/jet-luna/dx2/simulation-modeling/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/area-games/probe.md`, `gaps.json`, `batteries.json`, and the code under `~/.cache/jet-luna/dx3/area-games/pkg/`. Gap ids start with `area-games-G`.
