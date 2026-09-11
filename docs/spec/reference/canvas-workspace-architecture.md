# Canvas Workspace Architecture

Vocabulary: [Jet vocabulary](../../spec/vocabulary.md).

Canvas is a source-backed package/workspace manager. Jet source files,
`package.jet`, `workspace.jet`, environment source, and `.jet/lock` remain the
only semantic state. Canvas projects them, edits them through checked source
transactions, and reprojects after formatting and front-end validation.

R9 stays intact. `jet run foo.jet` and `jet dev foo.jet --target=web` remain
ceremony-free single-file flows. Workspace mode is discovered when the entry
file belongs to a package/workspace, or selected explicitly from Canvas.


## Workspace shape

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

The source write is one shared Canvas transaction seam. It compares the
expected source snapshot again immediately before publish, writes the checked
candidate to a synced temporary file, and atomically replaces the source file.
Project transactions use the same seam for every touched file and restore
completed files from their explicit before/after snapshots if a later publish
fails. A conflict or I/O failure leaves source and browser undo history at the
last committed snapshot; no Canvas graph or semantic sidecar is created.

No hidden Canvas DB. Local-only state may store viewport, tabs, selection,
recent commands, breakpoints, watches, and unsaved UI preferences. Shared visual
intent uses existing source-anchored comments only when the user asks to share it.

### Command Bridge

Canvas calls existing engines instead of owning replacements:

- `jet check`, `jet test`, `jet build`, `jet dev`
- Jetpack package graph, lock, catalog, overlay, provider, provenance, and
  environment realization APIs

Actions need honest authority metadata: source edits, package fetches, env
entry, service start/stop, secrets, network/cache, build outputs, and touched
files. Beginner UI summarizes intent; expert UI shows exact grants, hashes,
lock reasons, and diff.

## Product Surface

- Workspace Map: packages, members, files, imports, direct deps, catalog deps,
  targets, envs, services, lock/provenance, diagnostics, dirty state.
- Package Pane: `payload`, package kind, version, edition, runtime, exports,
  targets, effects, grants, public API, package visibility.
- Dependency Pane: add/remove/update deps through `package.jet` edits, with lock
  preview, strict-visibility errors, source channel, hash, and overlay facts.
- Targets/Tasks Pane: build/test/run/dev/doc/package/publish actions from the
  package/build graph. Runs through existing CLI/driver surfaces.
- Dev Pane: env packages, services, ports, logs, secrets, trust prompts, app
  preview, Canvas preview.
- Source Graph Pane: existing function graph, scoped by package/file, with
  cross-file references, rename impact, source jumps, and package boundaries.
  The `My Canvas` component tree is the compact source-backed entry point for
  files, function graphs, and the current function's typed variables; its
  `New`, `Callback`, and `Add` affordances use the existing checked source
  transactions. `on_*` functions expose callback handler labels and graph
  navigation from the current `event_views` projection; no handler graph or
  callback registry is stored outside Jet source.
- Diagnostics Pane: grouped by workspace, package, file, target, and manifest.
  Only Jet diagnostics appear.
- Trust/Provenance Pane: grants, lock reasons, envelopes, SBOM/audit facts,
  service authority, cache/network writes.


## Ratified Decisions

Ratified 2026-07-08:

- `D-CANVAS-WORKSPACE1=B`: package/workspace graph over source truth. Canvas
  opens a project graph built from `workspace.jet`, `package.jet`, source files, env
  source, and `.jet/lock`; file graphs remain child views.
- `D-CANVAS-WORKSPACE-STATE1=A`: semantic facts persist in source; private
  viewport/tabs/selection/debug watches stay local; shared visual intent uses
  explicit source-anchored comments.
- `D-CANVAS-WORKSPACE-AUTH1=A`: cross-file edits use previewed source
  transactions with touched-file revisions, formatter, front-end proof, package
  validation, and audit payloads.
- `D-CANVAS-WORKSPACE-NAV1=A`: one semantic project tree facets packages,
  targets, files/modules, symbols, graphs, diagnostics, deps, and Git state.

## Editor architecture

The workspace/project layer is paired with an editor decomposition. The seams
below are architecture contracts, not a second semantic model.

Blueprint separates the editor into a persistent graph model, semantic node
behavior, rendering and interaction, a compiler/validation path, an action
registry, an editor shell, a reflected details surface, and debugger state.
Canvas keeps those responsibilities source-backed: Jet source AST/HIR is the
model, the front end is the compiler, and graph JSON plus checked transactions
are projections and edits rather than a second semantic store.

The durable Jet advantages are source-as-model (no binary graph asset, merge
pain, or stale compile) and the front end as compiler. The semantic-node
registry and the descriptor-driven Details surface keep projection coverage
and editable fields in one source-backed model.
### Editor seams

The editor has four source-backed seams. Jet source is the only semantic truth;
everything below is projection and interaction.

### Seam 1 — Data-graph model

`graph_projection.rs` + `graph_json.rs` map source to graph facts. Emit a
stable `node_descriptor_id` per node so the render layer never re-derives
style from `kind` string matching.

### Seam 2 — Semantic node layer

A descriptor table in `crates/jet-devserver/src/Canvas/node_catalog.rs` is the
single source of truth for every node kind:

```
NodeDescriptor {
  id: "branch",
  archetype: Control,
  glyph: "◇",
  header: (…colors…),
  hover: "Chooses which path runs next.",
  palette: PaletteMeta { category: Flow, insertable: true, rank_terms: [...] },
  projection: fn(&Stmt) => Option<Node>,
  transaction: "insert_branch",
  default_editors: [...],
}
```

The graph protocol embeds the descriptor table. `graph-rendering.js` uses its
presentation and hover facts; `drawing-palette.js` uses visibility, ranking,
category, glyph, color, and insertion facts. The transaction path checks the
descriptor before creating a source transaction. The descriptor set rejects
missing, duplicate, orphaned, non-exported, non-insertable, and
transaction-mismatched entries.

### Seam 3 — Rendering + interaction

Split `graph-rendering.js` into:

- `render.js` — draw only, driven by descriptor table and graph facts.
- `hit-test.js` — hit map plus pin/wire endpoint geometry.
- `interaction.js` — one pointer state machine (idle → node-drag → wire-drag →
  rewire → marquee → menu), with each state mapped to a gesture scenario.

### Seam 4 — Source-sync (transaction bus)

`edit_actions.rs` keeps the transaction bus, with each arm registered by the
node descriptor's `transaction` field beside its projection rule.

### Details panel — make it reflect, not hand-render

Use a field-descriptor list
`{label, value, editable, apply_op}` instead of per-selection `innerHTML`.
Node, variable, and function detail views provide descriptor arrays; one
renderer turns them into rows and an Apply button. Every field is either live
(`apply_op` set) or absent, with no dead controls. Composite values keep one
source-backed inline anchor; leaf validation preserves the original source on
refusal or stale revision.

