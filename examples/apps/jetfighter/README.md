# jetfighter

Headless-replay implementation slice. Current deterministic mode proves entity
logic, asset and sound registration, input mapping, component queries, frame
hooks, budgets, and replay transcript through `core.game`.

The raylib display bridge is opt-in with `JET_RAYLIB_DISPLAY=1`; the checked
slice keeps CI headless while preserving the native-window path for local runs.
