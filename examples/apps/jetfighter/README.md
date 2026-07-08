# JetPlay / jetfighter

Product capstone for a small 2D game plus source-backed editor workflow.

Run the playable deterministic build:

```sh
jet run examples/apps/jetfighter/main.jet
```

Run the editor on a copied checkout or temp app root:

```sh
jet run examples/apps/jetfighter/workbench.jet -- /tmp/jetfighter 2 0
```

The editor rewrites `level.jet`; the game imports that source file, so the next
run compiles and plays the edited level. `workbench_ui.jet` is the web-buildable
editor surface. Native display remains opt-in through the raylib bridge with
`JET_RAYLIB_DISPLAY=1`; CI proves the same scene through headless replay.

Proof:

- deterministic replay, assets, sound, input, ECS query, frame hook, budget, and
  render/export transcript in `main.jet`
- source edit loop in `workbench.jet`
- web editor UI build in `workbench_ui.jet`
- perf and LOC evidence under `proof/`
