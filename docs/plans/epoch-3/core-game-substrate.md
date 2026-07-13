# Stable Core Game Substrate

## Goal

Card #238 turns D-WD10 into an Epoch 3 plan: `core.game` becomes the stable substrate for game development. Core owns assets, ECS, input, fixed-step timing, deterministic replay, editor hooks, and budgets. Renderer, audio, and editor backends remain replaceable packages.

This builds on D-GAME1 through D-GAME3: the beginner surface is scene-first with a frame hook. A `Scene` owns durable editable game data; `scene.on_frame((frame) => { ... })` attaches game logic to that scene.

## Current law

- D-GAME1 ratifies a first-party game stack.
- D-GAME2 names the engine `core.game`.
- D-GAME3 ratifies scene-first plus frame hook as the beginner API.
- D-WD10 ratifies the wider substrate: assets, ECS, input, fixed-step timing, deterministic replay, editor hooks, and budgets, with replaceable renderer/audio/editor backends.
- D-RAYLIB1 keeps `core.raylib` as the interim bridge below the native-shaped stack.
- R12 requires AOT and `jet dev` parity for executable TIR semantics; game examples must not become native-only blind spots.

Owner decisions D-GAME-ASSET1, D-GAME-ECS1, D-GAME-INPUT1, D-GAME-REPLAY1, D-GAME-BACKEND1, and D-GAME-BUDGET1 are ratified. The first implemented slice is the headless deterministic substrate: scene-owned assets, struct-marker components, input snapshots, replay transcript, explicit headless backend, and scene budgets. Editor hooks and native renderer/audio/editor packages remain later layers over the same scene identity.

## Vertical slices

1. Scene skeleton: `core.game` registers `Scene`, durable nodes, frame hook lowering, and a no-display type-check example that proves parser, sema, TIR, and codegen coverage.
2. Fixed-step timing and input: deterministic frame model, input snapshot type, and one Breakout/Pong-scale example with replayable input.
3. Assets: typed asset handles, load diagnostics, hot-reload metadata, and deterministic missing-asset behavior in CI.
4. ECS layer over scene data: components and systems operate on the same scene identity, not a second engine model.
5. Replay: record and replay frame inputs, random seeds, timing, and budget events; compiled and dev paths match.
6. Editor hooks: semantic IDs for scene nodes and components so a future visual editor can round-trip code-first scenes without owning a second file format.
7. Replaceable backends: renderer/audio/editor backends satisfy stable Core traits or interfaces without changing game code.
8. Budgets: frame time, memory, asset size, and draw-call budgets attach to the game package or scene and produce Jet diagnostics.

## Acceptance tests

- Golden example: one-file scene with frame hook runs deterministically in a headless/no-op backend.
- UI snapshots: unknown asset, duplicate scene node identity, invalid input binding, nondeterministic replay source, budget exceeded, backend missing, and ECS query on a non-component.
- Dev parity: `jet dev` and compiled execution produce the same replay transcript for a fixed input file.
- No-unsafe guard: safe game examples emit no Rust `unsafe`; any backend unsafe stays inside vetted internals or user `#Unsafe` gates.
- Editor identity test: formatting or reordering scene declarations preserves semantic IDs used by editor hooks.
- Backend boundary test: swapping a renderer backend does not change scene/update semantics.

## Dependency order

1. Landed: ratify exact substrate APIs that are user-facing.
2. Landed: keep a no-op/headless backend as the test floor while the game API type-checks.
3. Landed: scene skeleton and frame hook.
4. Landed: fixed-step timing and input snapshots.
5. Landed: assets and typed handles.
6. Landed: replay transcript floor.
7. Landed: ECS marker/query floor over the same scene identity.
8. Landed: explicit headless backend and scene budgets in the transcript.
9. Next: editor hooks, native replay artifact read/write, replaceable renderer/audio/editor packages, and richer budget diagnostics.

## Ratified owner decisions

- D-GAME-ASSET1=A: scene asset registry with typed handles.
- D-GAME-ECS1=B: struct marker components.
- D-GAME-INPUT1=A with typed-action direction: scene bindings plus frame snapshot, compatible with future typed action enums.
- D-GAME-REPLAY1=A as amended by D-ARTIFACT-EXT1=A: `game.Replay` API plus `.jetreplay` artifact contract.
- D-GAME-BACKEND1=A: typed `game.Backend` value, default headless path.
- D-GAME-BUDGET1=A with runtime watcher direction: scene/package budgets as one fact model plus runtime visibility.

## Adversarial tradeoffs

- Safety first: replay and asset systems must be deterministic and typed; native graphics/audio bindings cannot leak unchecked handles into beginner code.
- Beginner experience: first tutorial starts with a scene and moving object, not component queries or backend setup.
- Runtime performance: ECS and fixed-step scheduling must have a zero-cost path under the beginner scene model; no hidden boxing or dynamic dispatch in hot loops unless explicitly selected.
- One mechanical path: frame hooks, ECS, and editor hooks all operate on one scene identity model. They are views over one substrate, not three engines.
- Ecosystem breadth: replaceable backends let experts target raylib, wgpu, native audio, or future editors without fragmenting the Core API.
