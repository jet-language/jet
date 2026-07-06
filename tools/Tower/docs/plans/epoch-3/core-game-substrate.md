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

Exact APIs for assets, ECS queries, input bindings, replay files, budget attachment, editor hooks, and backend selection still need owner decisions before implementation.

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

1. Ratify exact substrate APIs that are user-facing.
2. Finish native `core.raylib` bridge or keep a no-op backend as the test floor while the game API type-checks.
3. Land scene skeleton and frame hook.
4. Add fixed-step timing and input snapshots.
5. Add assets and typed handles.
6. Add replay and dev/AOT transcript parity.
7. Add ECS over the same scene identity.
8. Add editor hooks, replaceable backends, and budgets.

## Owner ballots needed

- D-GAME-ASSET1: asset declaration/loading surface and typed handle policy.
- D-GAME-ECS1: ECS/component/query public API and how it layers over `Scene`.
- D-GAME-INPUT1: input binding names, devices, and snapshot API.
- D-GAME-REPLAY1: replay file/API surface and determinism contract.
- D-GAME-BACKEND1: renderer/audio/editor backend selection surface.
- D-GAME-BUDGET1: budget attachment surface and diagnostic policy.

## Adversarial tradeoffs

- Safety first: replay and asset systems must be deterministic and typed; native graphics/audio bindings cannot leak unchecked handles into beginner code.
- Beginner experience: first tutorial starts with a scene and moving object, not component queries or backend setup.
- Runtime performance: ECS and fixed-step scheduling must have a zero-cost path under the beginner scene model; no hidden boxing or dynamic dispatch in hot loops unless explicitly selected.
- One mechanical path: frame hooks, ECS, and editor hooks all operate on one scene identity model. They are views over one substrate, not three engines.
- Ecosystem breadth: replaceable backends let experts target raylib, wgpu, native audio, or future editors without fragmenting the Core API.
