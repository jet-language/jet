# JetPlay / jetfighter

Product capstone for a small 2D game plus source-backed editor workflow.

Run the playable visual build:

```sh
jet run examples/apps/jetfighter/main.jet
```

Run the deterministic transcript used by tests:

```sh
JETPLAY_HEADLESS=1 jet run examples/apps/jetfighter/main.jet
```

Run the editor on a copied checkout or temp app root:

```sh
jet run examples/apps/jetfighter/workbench.jet -- /tmp/jetfighter 2 0
```

Build the web editor UI:

```sh
jet build --target=web examples/apps/jetfighter/workbench_ui.jet
```

The editor rewrites `level.jet`; the game imports that source file, so the next
run compiles and plays the edited level. `workbench_ui.jet` is the web-buildable
editor surface; open `build/index.html` after the web build. CI sets
`JETPLAY_HEADLESS=1` for the deterministic transcript.

Proof:

- deterministic replay, assets, sound, input, ECS query, frame hook, budget,
  playable raylib visual mode, and render/export transcript in `main.jet`
- source edit loop in `workbench.jet`
- web editor UI build in `workbench_ui.jet`
- perf and LOC evidence under `proof/`
