# Canvas Workspace Architecture

Canvas v1 proves source-backed graph editing for one `.jet` file. The next
architecture makes Canvas a package/workspace manager without changing the
source-of-truth rule: Jet source files, `pkg.jet`, `workspace.jet`, env source,
and `.jet/lock` remain the only semantic state. Canvas projects them, edits
them through checked source transactions, and reprojects after `jet fmt` plus
front-end validation.

R9 stays intact. `jet run foo.jet` and `jet dev foo.jet --target=web` remain
ceremony-free single-file flows. Workspace mode is discovered when the entry
file belongs to a package/workspace, or selected explicitly from Canvas.

## Current Limit

- `crates/jet-devserver/src/WebHost.rs` serves `/canvas` for one watched file.
- `crates/jet-devserver/src/Canvas.rs` exposes one `source_id`, one `revision`, one graph
  document, one-file transactions, one-file source control, and placeholder
  package action audit fields.
- `docs/reference/canvas-protocol.md` names the right law: Canvas has no parser,
  checker, runtime, graph asset, or semantic sidecar.
- Existing Jetpack substrate already owns package/workspace truth:
  `pkg.jet`, `workspace.jet`, `.jet/lock`, strict package graph, env/dev,
  services, trust grants, catalogs, overlays, provenance, and locks.

## Target Shape

### Project Graph

Add a workspace-level document above the existing file graph:

```json
{
  "protocol": "jet.canvas.project",
  "schema_version": 1,
  "project_root": "/repo",
  "project_revision": "sha256-...",
  "entry": "apps/web/main.jet",
  "files": [
    {"path": "apps/web/main.jet", "revision": "sha256-...", "kind": "source"}
  ],
  "workspace": {"path": "workspace.jet", "members": []},
  "packages": [],
  "targets": [],
  "envs": [],
  "services": [],
  "locks": [],
  "diagnostics": [],
  "source_control": {}
}
```

The existing graph document remains the function/source detail view. Project
records link to file graphs by `source_id` and file-qualified spans. Unknown
fields stay forward-compatible only for non-semantic facts.

### Revision Model

Use `project_revision` for the projected package/workspace snapshot and
per-file revisions for edits. A transaction conflicts only when one of its
touched files or manifests changed. Whole-repo conflict is reserved for edits
whose read set spans the whole workspace graph.

### Transactions

Add project transactions that carry touched files explicitly:

```json
{
  "schema_version": 1,
  "op": "add_workspace_member",
  "project_revision": "sha256-...",
  "files": [
    {"path": "workspace.jet", "revision": "sha256-..."}
  ],
  "member_path": "packages/logger"
}
```

Every write path follows one rule:

1. Build overlay text for every touched source file.
2. Run formatter on each changed source.
3. Re-run the front end and Jetpack manifest/workspace evaluators.
4. Reject with Jet diagnostics if validation fails.
5. Write all touched files, then reproject.

No hidden Canvas DB. Local-only state may store viewport, tabs, selection,
recent commands, breakpoints, watches, and unsaved UI preferences. Shared visual
intent uses existing source-anchored comments only when the user asks to share it.

### Command Bridge

Canvas should call existing engines instead of owning replacements:

- `jet check`, `jet test`, `jet build`, `jet dev`
- Jetpack package graph, lock, catalog, overlay, provider, and provenance APIs
- `jetpack dev`, service health/logs, trust grant checks, env realization
- future build graph commands: `jet inspect graph`, `jet inspect query build`,
  `jet inspect explain-build`

Actions need honest authority metadata: source edits, package fetches, env
entry, service start/stop, secrets, network/cache, build outputs, and touched
files. Beginner UI summarizes intent; expert UI shows exact grants, hashes,
lock reasons, and diff.

## Product Surface

- Workspace Map: packages, members, files, imports, direct deps, catalog deps,
  targets, envs, services, lock/provenance, diagnostics, dirty state.
- Package Pane: `payload`, package kind, version, edition, runtime, exports,
  targets, effects, grants, public API, package visibility.
- Dependency Pane: add/remove/update deps through `pkg.jet` edits, with lock
  preview, strict-visibility errors, source channel, hash, and overlay facts.
- Targets/Tasks Pane: build/test/run/dev/doc/package/publish actions from the
  package/build graph. Runs through existing CLI/driver surfaces.
- Dev Pane: env packages, services, ports, logs, secrets, trust prompts, app
  preview, Canvas preview.
- Source Graph Pane: existing function graph, scoped by package/file, with
  cross-file references, rename impact, source jumps, and package boundaries.
- Diagnostics Pane: grouped by workspace, package, file, target, and manifest.
  Only Jet diagnostics appear.
- Trust/Provenance Pane: grants, lock reasons, envelopes, SBOM/audit facts,
  service authority, cache/network writes.

## Implementation Plan

1. Project graph read model:
   - add `project_json_for_entry(path)` beside `graph_json_for_file`;
   - discover project root, workspace, package, lock, files, deps, targets, envs;
   - add `/canvas/project` endpoint and protocol docs;
   - tests: package fixture, monorepo fixture, single-file fallback.

2. File-qualified graph/query:
   - include file-qualified IDs in graph/query results;
   - allow `graph?source_id=...` or `POST query` with `source_id`;
   - tests: cross-file source-to-graph, references, rename preview.

3. Project transactions:
   - add multi-file transaction envelope with touched file revisions;
   - implement `add_workspace_member`, `create_package`, `edit_pkg_field`,
     `add_dependency`, `remove_dependency`, `add_target`, `add_env_service`;
   - validate via formatter, front end, workspace/package evaluators;
   - tests: stale touched-file rejection, no hidden state, rollback on failure.

4. Source control:
   - replace current-file status with workspace status/diff/history groups;
   - surface transaction diff before write;
   - keep Git text truth; no graph locks unless later ratified.

5. Command/action bridge:
   - expose package/build/dev/service actions with authority and audit payloads;
   - replace `local-source` action audit placeholders with real package/lock data;
   - tests: preview action, run/check action metadata, denied authority result.

6. UI restructuring:
   - keep first screen usable as editor, not landing page;
   - add project rail + package/deps/dev/targets panes around existing graph;
   - keep single-file mode compact and ceremony-free.

## Acceptance Gates

- `docs/reference/canvas-protocol.md` documents project graph, project
  transactions, workspace source control, command/action authority, and R9
  fallback.
- `tests/canvas.rs` covers project graph JSON, multi-file revisions,
  manifest/workspace edit ops, cross-file source spans, and no sidecar state.
- `tests/web_dev.rs` covers `/canvas/project`, `/canvas/graph?source_id=...`,
  project transactions, and source-control workspace payload.
- Jetpack tests cover projection helpers for `pkg.jet`, `workspace.jet`,
  `.jet/lock`, env/services, and package graph diagnostics.
- Any new diagnostic has a registry entry and UI snapshot.
- No external compiler dependency, no graph asset store, no Canvas-only
  semantics, no mandatory manifest for single-file users.

## Ratified Decisions

Ratified 2026-07-08:

- `D-CANVAS-WORKSPACE1=B`: package/workspace graph over source truth. Canvas
  opens a project graph built from `workspace.jet`, `pkg.jet`, source files, env
  source, and `.jet/lock`; file graphs remain child views.
- `D-CANVAS-WORKSPACE-STATE1=A`: semantic facts persist in source; private
  viewport/tabs/selection/debug watches stay local; shared visual intent uses
  explicit source-anchored comments.
- `D-CANVAS-WORKSPACE-AUTH1=A`: cross-file edits use previewed source
  transactions with touched-file revisions, formatter, front-end proof, package
  validation, and audit payloads.
- `D-CANVAS-WORKSPACE-NAV1=A`: one semantic project tree facets packages,
  targets, files/modules, symbols, graphs, diagnostics, deps, and Git state.

## Shipped Slice

2026-07-08:

- Added read-only `jet.canvas.project` projection and `/canvas/project`.
- Project mode reports `single_file`, `package`, or `workspace`.
- Workspace projection reads `workspace.jet` via Jetpack's evaluator and parses
  member `pkg.jet` manifests through the existing manifest parser.
- Project documents include per-file revisions, package facts, dependency facts,
  target facts, lock facts, and the ratified state policy.
- Tests cover single-file fallback, workspace member/package projection, protocol
  docs, and the web dev route.
- Added `jet.canvas.project.edit` and `/canvas/project/transaction` for
  previewed project source transactions.
- First project transaction op: `add_dependency`, editing `pkg.jet` through the
  existing manifest helper, validating the Jetpack manifest parser before write,
  checking `project_revision` plus touched-file revisions, and returning
  authority/audit/diff payloads. Preview mode writes nothing.
- Canvas UI now fetches `/canvas/project` and renders a source-backed Project
  rail with entry, packages, deps, targets, source-truth file count, and state
  policy. No Canvas project asset or semantic sidecar.
- File graphs and queries now accept project-relative `source_id`, resolving
  through the projected source-truth file set with a bounded project-root
  fallback for new live files.
- `/canvas/source-control` now reports package/workspace Git text truth:
  `project_revision`, dirty file count, per-file status/diff, and entry history.
- Project transactions now include `create_package`, which creates real
  `pkg.jet` + entry `.jet` files from a touched-file envelope using `missing`
  revisions, validates manifest syntax, and reprojects from source after write.
- Project transactions now include `add_workspace_member`, editing or creating
  `workspace.jet` through a touched-file envelope and validating Jetpack's
  workspace evaluator before write.
- Project transactions now include `remove_dependency`, `edit_pkg_field`, and
  `add_target`, all validating through the existing Jetpack manifest parser.
- Project transactions now include `add_env_service`, creating/editing
  `env.jet` and validating Jetpack module evaluation before write.
- Project graph now projects `env.jet` package refs, prompt, secrets, dev
  services, and Jet diagnostics from ModuleEval; `env.jet` participates in
  project file revisions as kind `env`.
- Project rail source cards navigate to file-qualified graph views; Git dirty
  state reports workspace file counts.
- Canvas action palette/preview authority now reports package-backed grants,
  package id/version, and touched source file instead of local placeholder
  authority; single-file mode remains explicit as `single-file`.
- Focused proof: `nix develop -c cargo test --test canvas` and
  `nix develop -c cargo test --test web_dev` pass.

Remaining card work:

- deeper lock/package diagnostics beyond current manifest/workspace/env basics;
- action authority metadata for dev/service/lock operations beyond package
  source actions;
- broader full-suite verification before closing the card.

## Editor Architecture — current vs Blueprint-grade target (re-baselined 2026-07-10)

The sections above cover the workspace/project layer. This section covers how
the editor itself is built, mapped against UE5 Blueprint proven decomposition,
and the target seams that keep full parity work from becoming shotgun surgery.

### What Canvas is made of today

Rust (source truth + projection + transactions), ~10.5k lines across 13 files:

- `crates/jet-devserver/src/Canvas.rs` (20) — thin module root.
- `crates/jet-devserver/src/Canvas/graph_projection.rs` (1583) — AST/HIR → graph JSON. Decides node
  `archetype` (`entry` / `control` / `function_exec` / `function_pure` / value)
  and `kind`. This is the real "semantic node layer."
- `crates/jet-devserver/src/Canvas/graph_json.rs` (815),
  `crates/jet-devserver/src/Canvas/graph_helpers.rs` (588) — JSON shapes.
- `crates/jet-devserver/src/Canvas/edit_actions.rs` (2301) — transaction bus. One `match op { ... }`
  (`insert_call`, `insert_branch`, `edit_inline_expr`, `rename_binding`,
  `edit_function_signature`, pattern-arm ops, multi-input ops, `replace_source`…).
- `crates/jet-devserver/src/Canvas/query_actions.rs` (1335) — palette/action database (`actions`
  op): project functions + core catalog + exclusion reasons.
- `crates/jet-devserver/src/Canvas/project_transactions.rs` (929),
  `crates/jet-devserver/src/Canvas/project_scan.rs` (553) — workspace layer.
- `crates/jet-devserver/src/Canvas/schema_api.rs` (1022),
  `crates/jet-devserver/src/Canvas/validation_json.rs` (608), and
  `crates/jet-devserver/src/Canvas/debug_source_git.rs` (319).
- `crates/jet-canvas/src/html.rs` (388) and `crates/jet-canvas/src/js.rs` (18)
  own the browser shell and script assembly.

Browser runtime, ~5.3k lines of JS concatenated by
`crates/jet-canvas/src/js.rs::canvas_js()` into one
IIFE (good: independently lintable files; bad: no module boundaries or exports —
everything shares one closure scope):

- `crates/jet-canvas/src/js/runtime-state.js` (172) — state + `window.__jetCanvasTest` hooks.
- `crates/jet-canvas/src/js/editing-history.js` (671) — undo/redo/transaction posting.
- `crates/jet-canvas/src/js/diagnostics-query.js` (470) — problems, check, jump.
- `crates/jet-canvas/src/js/drawing-palette.js` (582) — context menu / palette.
- `crates/jet-canvas/src/js/project-navigation.js` (650) — tabs, graph switch, project rail.
- `crates/jet-canvas/src/js/graph-rendering.js` (1196) — canvas 2D immediate-mode draw + node style +
  hit map + node-size measurement.
- `crates/jet-canvas/src/js/inspector-connections.js` (674) — Details panel (`innerHTML` templates).
- `crates/jet-canvas/src/js/input-events.js` (457) — pointer/keyboard.
- `crates/jet-canvas/src/js/transactions-catalog.js` (402),
  `crates/jet-canvas/src/js/bootstrap.js` (41).

Test harness (the M0 win, real): `scripts/canvas-test/driver.mjs` (CDP pipe),
`scenario.mjs` (1250, 28 scenarios), `run.mjs`; `tests/canvas_scenarios.rs`
launches `jet dev --target=web` + Chromium. `tests/canvas.rs` (3049) is the
in-process projection suite.

### UE5 Blueprint's proven decomposition, and where Canvas sits

Blueprint earned maintainability by separating four layers plus an editor shell.

| UE5 layer | What it does | Canvas equivalent | Verdict |
|---|---|---|---|
| `EdGraph` / `EdGraphNode` / `EdGraphPin` | Persistent graph data model (serialized objects; nodes own pins, pins own links) | **Jet source AST/HIR** projected by `graph_projection.rs`. No persistent graph object | Advantage + cost. No drift, no binary asset — but there is nowhere to hang node-local state, so staged/positioned nodes live only in JS view state, off to the side |
| `K2Node` subclasses (`K2Node_CallFunction`, `K2Node_VariableGet`, `K2Node_IfThenElse`, `K2Node_Switch`…) | Semantic per-node behavior: pins, tooltip, menu category, expansion to lower graph | **Split across two files with no registry**: archetype/kind decided in `graph_projection.rs`; style/glyph/hover/label decided by a parallel `if (node.kind === …)` chain in `graph-rendering.js`; palette metadata in `query_actions.rs` | Weakest layer. No single node descriptor. Adding a kind = editing 4+ if-chains that must agree |
| `SGraphEditor` / `SGraphNode` / `SGraphPin` (Slate widgets) | Rendering + interaction as per-node widget objects | `graph-rendering.js` immediate-mode 2D + `input-events.js`. No per-node widget; hit-testing rebuilt each frame into a hit map | Works for render; interaction logic is a flat event handler, hard to extend to marquee/drag/rewire uniformly |
| `FKismetCompilerContext` | Compiles graph → bytecode | **The Jet front end itself** | Clean advantage — no separate compiler, no stale-compile class of bugs |
| `BlueprintActionDatabase` + `UBlueprintNodeSpawner` | Palette/context-menu action registry with ranking | `query_actions.rs` `actions` op | Present but leaky: #389 shows foreign-symbol phantoms and wrong ranking. No spawner abstraction, ranking is ad hoc |
| `FBlueprintEditor` shell (tabs: My Blueprint, Details, Palette, Compiler Results, Debug) | Editor window | `html.rs` static shell + the JS panels | Shell exists; panels are `innerHTML` string builders with per-panel logic, no shared component model |
| Details = property system reflecting `UPROPERTY` | Generic property editor | `inspector-connections.js` hand-written `innerHTML` per selection type | No reflection; every editable field is bespoke. This is why Details has "dead controls" (#377) |
| `FKismetDebugUtilities` (breakpoints, watches, exec pulse) | Debugger | Projection facts only; UI buttons in html.rs | Largest unbuilt layer |

The two genuine Jet advantages to keep: source-as-model (no EdGraph asset, no
merge pain, no stale compile) and front-end-as-compiler. The debt is the
**missing semantic-node registry** and the **hand-rolled Details/shell**.

### Target architecture

Name four seams and give each an owning module. The rule stays: Jet source is
the only semantic truth; everything below is projection + interaction.

### Seam 1 — Data-graph model (keep)

`graph_projection.rs` + `graph_json.rs`. Source → graph facts. No change in
principle. One addition: emit a stable `node_descriptor_id` per node so the
render layer never re-derives style from `kind` string matching.

### Seam 2 — Semantic node layer (build the missing registry)

New: `crates/jet-devserver/src/Canvas/node_catalog.rs` — one descriptor table, the single source
of truth for every node kind:

```
NodeDescriptor {
  id: "branch",
  archetype: Control,
  glyph: "◇",
  header: (…colors…),
  hover: "Chooses which path runs next.",
  palette: PaletteMeta { category: Flow, insertable: true, rank_terms: [...] },
  projection: fn(&Stmt) -> Option<Node>,   // how source becomes this node
  transaction: "insert_branch",            // which edit op creates it
  default_editors: [...],                  // per-input default-value widgets
}
```

Generate a JSON descriptor table served to the browser (e.g. `/canvas/node-catalog`
or embedded), so `graph-rendering.js` reads `descriptor.glyph/header/hover`
instead of its `if (node.kind === "branch")` ladder, and `drawing-palette.js`
reads `descriptor.palette` instead of ad-hoc ranking. This kills #389's class of
bug: the catalog is the only place a node can enter a menu, and a
catalog-vs-real-exports cross-check test (already requested in #389) becomes a
one-liner.

### Seam 3 — Rendering + interaction

Split `graph-rendering.js` (1196 lines mixes three jobs):

- `render.js` — draw only, driven by descriptor table + graph facts.
- `hit-test.js` — the hit map + pin/wire endpoint geometry (already partly
  separate via `__jetCanvasTest` hooks; make it a real module).
- `interaction.js` — one pointer state machine (idle → node-drag → wire-drag →
  rewire → marquee → menu). Today marquee, node-drag, and data-wire-drag are
  half-present and untested; a single FSM makes each a state, not a special
  case, and each state maps to one gesture scenario.

### Seam 4 — Source-sync (transaction bus)

`edit_actions.rs` `match op` stays, but each arm is registered by the node
descriptor's `transaction` field, so a new node kind's edit op is declared next
to its projection rule, not bolted onto a growing match by hand.

### Details panel — make it reflect, not hand-render

Replace the per-selection `innerHTML` in `inspector-connections.js` with a
field-descriptor list: `{label, value, editable, apply_op}`. Node/variable/
function detail views each supply a descriptor array; one renderer turns it into
rows + an Apply button. Every field is either live (`apply_op` set) or absent —
no dead controls, which is exactly #377's exit criterion.

### Migration steps (no big-bang rewrite — the postmortem warns against a 5th)

1. Introduce `node_catalog.rs` and the descriptor JSON without changing behavior;
   have `graph_projection.rs` stamp `node_descriptor_id`. Prove via existing
   `tests/canvas.rs` snapshots (unchanged output).
2. Point `graph-rendering.js` style/glyph/hover lookups at the descriptor table;
   delete the parallel if-chains. Existing gesture scenarios must stay green.
3. Route `drawing-palette.js` ranking through `descriptor.palette`; land the
   catalog-vs-exports cross-check test; close #389.
4. Extract `interaction.js` FSM from `input-events.js` + `graph-rendering.js`;
   add gesture scenarios for node-drag reposition, marquee, and **data-pin
   drag-to-wire** (the missing Blueprint gesture).
5. Convert Details to the field-descriptor renderer; close #377's dead-control
   bar.

Each step is guarded by the M0 harness — the anti-regression proof the last four
attempts lacked.

### Worked example A — add a new node type ("Switch"/dispatch insert)

Today (shotgun surgery, ~5 files, 4 must agree by hand):

1. `graph_projection.rs` — teach `project_stmt`/`project_expr_node` to emit the
   node with the right `archetype`/`kind`.
2. `graph-rendering.js` — add `if (node.kind === "switch") return …glyph ⇉…` in
   `nodeStyle`, another arm in the hover-text chain, maybe node-size logic.
3. `edit_actions.rs` — add an `"insert_switch"` arm to `match op`.
4. `query_actions.rs` — add it to the palette action set with a rank.
5. `scenario.mjs` — add an insert gesture scenario.

Miss any one and you get a node that projects but has no style, or inserts but
never appears in the menu (#389 is exactly this class).

After (registry): add one `NodeDescriptor` in `node_catalog.rs` (archetype,
glyph, hover, palette, projection closure, transaction op, default editors) +
one gesture scenario. Render, palette, and Details derive automatically. One
file of intent, one test.

### Worked example B — add a new panel ("Find in project" results)

Today: hand-write markup in `html.rs`, add a `<div id="…">`, write a bespoke
`innerHTML` builder + event wiring in a JS file, thread state through the shared
IIFE closure, add DOM ids the string-assertion test greps for.

After: panels declare `{id, title, mount(state) -> rows, on_action}` against the
same field-descriptor renderer used by Details. A find-results panel is a
descriptor array (`{label: match, apply_op: "jump_to_span"}`); the shell renders
it and the gesture test asserts a row click selects the node — the same shape as
every other panel, so the harness scenario is copy-adjust, not new machinery.

### Critical files
- `crates/jet-devserver/src/Canvas/graph_projection.rs`
- `crates/jet-devserver/src/Canvas/edit_actions.rs`
- `crates/jet-devserver/src/Canvas/query_actions.rs`
- `crates/jet-canvas/src/js/graph-rendering.js`
- `crates/jet-canvas/src/js/inspector-connections.js`
