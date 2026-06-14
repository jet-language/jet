# Capstone — **Forge**: a Nix-backed dev-environment & task runner, written in Jet

> A capstone stress-test project for the Jet language. One look should make a
> developer want to use Jet: it's a real tool that solves a real problem
> (reproducible toolchains without `nix develop` ceremony), and it exercises
> the **majority of the language** end to end.

## The pitch (why someone wants Jet after seeing this)

`nix develop -c cargo build` is powerful but ceremonious. **Forge** lets you
declare your project's tools and tasks in one small file and run them with a
friendly CLI:

```
forge list                 # show tasks + the tools each needs
forge plan build           # show the toolchain + command, resolve nothing
forge run build            # provision exact tool versions via Nix, then run
forge env                  # print the assembled PATH/vars (eval in your shell)
forge doctor               # is nix present? is the manifest valid?
forge init                 # drop a starter forge.json (baked in at compile time)
```

Under the hood Forge is a **translation layer to Nix**: it shells out to
`nix build --json` to materialize tool binaries into the Nix store, parses the
JSON Nix emits, assembles a `PATH`, and runs your tasks in that environment.
Nix stays the package *provider*; Forge is the friendly *front end* — and it's
written entirely in Jet.

### Honest scope (evaluated against the toolchain)

- Jet **cannot** replace Nix's sandboxed builder. It *can* orchestrate Nix via
  `std.process` + `std.json`. That's the realistic, shippable layer.
- v1 std has **no TOML parser** (`toml` is reserved, not implemented), so the
  manifest is **`forge.json`** (JSON is in std). Honest and idiomatic.
- Everything except actual provisioning runs **offline and deterministically**
  (parse, plan, DAG, cycle detection). The Nix path **degrades gracefully**
  when `nix` is absent, and there's an **offline fixture mode** so the
  JSON-parsing + parallel-resolve code paths are demonstrable and test-green
  without a network or Nix.

## What it showcases (language coverage map)

| Feature | Where |
|---|---|
| Package manager: main pkg + path deps | `forge/jet.toml` → 4 subpackages |
| Sub-packages / a dependency graph | `taskrunner` depends on `ansi` |
| Multi-file package (local `import "..."`) | `forge/` (forge.jet + cli.jet + app.jet) |
| Structs + methods (`self`, static) | manifest models, `Style` |
| Enums + exhaustive `switch` | `Color`, `ManifestError`, `Json` walks |
| Recursive enum (invisible boxing) | task-graph error path / `Plan` tree |
| Generics `<T>` + traits + `derive` | `taskrunner` topo sort, `Comparable` |
| Trait-as-type (dynamic dispatch) | a `Reporter`/`Styler` interface |
| Closures: `map`/`filter`/`reduce`/`each`/`sort_by` | manifest + task formatting |
| Option (`T?`, `value()`/`null`) | lookups everywhere |
| Errors as values (`T ? E`, `ok`/`err`, `?`, `or`) | manifest load, nix calls |
| `Map<K,V>` / `List<T>` literals + API | env vars, task table, counts |
| Strings: interpolation, split/trim/join/contains | everywhere |
| `comptime` + `embed_file` | baked-in default manifest + version banner |
| `std`: fs, io, env, process, json, tasks | nixbridge + cli |
| **Tasks/channels (concurrency)** | parallel tool resolution in `nixbridge` |
| `test "..." { }` blocks (`jet test`) | every subpackage |
| Friendly diagnostics ethos | cycle + manifest errors written Jet-style |

## Architecture (bottom-up build order)

```
examples/capstone/
  PLAN.md            (this file)
  PROGRESS.md        (living status — update as you go)
  forge/
    jet.toml         main package "forge", path-deps on the 4 below
    forge.jet        entry: main(), wires CLI -> app
    cli.jet          local file import: arg parsing -> Command enum
    app.jet          local file import: command dispatch / orchestration
    packages/
      ansi/          (leaf)  terminal styling: Color enum, Style struct, traits
      manifest/      (std.json, std.fs) load+validate forge.json -> Project model
      taskrunner/    (dep: ansi) task DAG: topo sort, cycle detection, run plan
      nixbridge/     (std.process, std.json, std.tasks) Nix translation layer
    demo/            a sample project Forge operates on
      forge.json     tools + tasks manifest
      fixtures/      captured `nix build --json` output for offline mode
```

### Subpackage responsibilities

- **ansi** — `Color` enum; `Style { color, bold }` with builder methods;
  `paint(text)`; a `Styler` trait so callers depend on the interface.
  Pure Jet, zero std. Showcases enums/structs/methods/traits/strings.
- **manifest** — `Project`, `Tool`, `Task` structs; `ManifestError` enum;
  `load(path) -> Project ? ManifestError` walking the `Json` enum by hand.
  Showcases std.json/std.fs, Option, errors-as-values, Map, closures.
- **taskrunner** — build a dependency graph from tasks, **topological sort**
  with **cycle detection** (the on-brand great error: "task cycle: build ->
  test -> build"). Generic helpers + `Comparable` derive for stable ordering.
  Showcases generics/traits, recursion, Map/List, closures.
- **nixbridge** — `resolve(tools) -> Env ? NixError`: shells `nix build --json`
  per tool **in parallel via tasks/channels**, parses JSON store paths,
  assembles `PATH`. Offline fixture mode reads `demo/fixtures/*.json` so the
  parse + parallel paths run without Nix. Graceful "nix not found" message.

### CLI surface (app.jet)

`Command` enum: `List`, `Plan(task)`, `Run(task)`, `Env`, `Doctor`, `Init`,
`Help`. Dispatch returns proper exit codes (`std.process.exit`).

## Verification strategy (must stay green)

1. `jet test` in each subpackage (topo sort, cycle detect, manifest parse,
   ansi codes, json fixture parse).
2. `forge` against `demo/` offline: `list`, `plan build`, `env --dry-run`,
   `run build --dry-run`, `doctor`, `init` — all deterministic, captured as
   expected output.
3. Real Nix path is opt-in (`forge run build` without `--dry-run`); documented,
   not part of the green battery (no network in CI).

## Build order (each step ends green before the next)

1. Scaffold dirs + `PROGRESS.md`. ✅ gate: dirs exist.
2. `ansi` subpackage + its tests. gate: `jet test packages/ansi`.
3. `manifest` subpackage + `demo/forge.json` + tests. gate: parse green.
4. `taskrunner` (dep ansi) + tests. gate: topo + cycle tests green.
5. `nixbridge` + `demo/fixtures` + offline tests. gate: fixture parse green.
6. main `forge` (jet.toml wiring all 4) + cli.jet + app.jet + forge.jet.
   gate: `forge list`/`plan`/`run --dry-run` run against demo.
7. Capture expected outputs; final full run-through; update PROGRESS + README.

## Open risks (watch while building)

- Transitive path deps (taskrunner→ansi while forge→taskrunner): verify early;
  fall back to forge depending on all four flat if the graph misbehaves.
- Local-file `import "cli";` + package `import manifest;` in the same package:
  proven separately in examples; verify they coexist in step 6.
- `tasks.spawn` capturing values across the boundary needs `take`/`clone`
  (see example 33/34) — follow that pattern exactly in nixbridge.
