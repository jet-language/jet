# Canvas workspace architecture

This document describes Canvas's source-backed project boundary and the editor
seams around it. It is for the devserver, Canvas client, and Jetpack
contributors who need one model for single files, packages, and workspaces. The
wire shape is defined by [`canvas-protocol.md`](canvas-protocol.md); vocabulary
is in [Jet vocabulary](../vocabulary.md). The executable boundary is in
[`schema_api.rs`](../../../crates/jet-devserver/src/Canvas/schema_api.rs),
[`project_scan.rs`](../../../crates/jet-devserver/src/Canvas/project_scan.rs),
[`project_transactions.rs`](../../../crates/jet-devserver/src/Canvas/project_transactions.rs),
and the graph/editor seams are linked below.

Canvas projects source; it does not introduce a semantic project database. Jet
source files, `package.jet`, `workspace.jet`, environment source such as
`env.jet`, and `.jet/lock` are the semantic inputs. Canvas formats and validates
source transactions, then projects the resulting source again.

The single-file flows remain direct: `jet run foo.jet` and
`jet dev foo.jet --target=web` do not require a project asset. Canvas discovers
workspace mode when the entry file belongs to a package or workspace, or when
the user selects that mode explicitly.

## Source-backed boundary

The project graph is a view above the function/source graph. Project records
refer to child graphs with `source_id` and file-qualified source spans. Unknown
fields are forward-compatible only when they carry non-semantic facts; a client
must not treat an unrecognized field as an instruction or a second source of
truth.

The source boundary is deliberately explicit:

- package semantics stay in `package.jet` and package source;
- workspace membership and workspace evaluation stay in `workspace.jet`;
- environment packages and dev services stay in environment source, including
  `env.jet`;
- dependency resolution facts stay in `.jet/lock`;
- function and type semantics stay in ordinary `.jet` source.

Viewport, tabs, selection, recent commands, breakpoints, watches, and unsaved
UI preferences may be local. Shared visual intent uses source-anchored Canvas
comments only when the user asks to share it. Canvas never creates a binary
graph asset or a hidden semantic sidecar.

## Project document

`jet.canvas.project` is the project-level document. Its `schema_version` is `1`,
and its `project_revision` hashes the projected source-truth file set. A
representative response is:

```json
{
  "protocol": "jet.canvas.project",
  "schema_version": 1,
  "project_root": "/repo",
  "project_revision": "sha256-...",
  "entry": "apps/web/main.jet",
  "mode": "workspace",
  "workspace": {"path": "workspace.jet", "members": []},
  "packages": [],
  "targets": [],
  "outputs": [],
  "envs": [],
  "services": [],
  "files": [
    {"path": "apps/web/main.jet", "revision": "sha256-...", "kind": "source"}
  ],
  "parts": [],
  "part_conflicts": [],
  "locks": [],
  "diagnostics": [],
  "source_control": {"truth": "git-text"},
  "state_policy": {
    "semantic": "source",
    "local": ["tabs", "viewport", "selection", "breakpoints", "watches", "comment_boxes", "staged_nodes"],
    "shared_visual": "source-anchored-comments"
  }
}
```

`mode` is `single_file`, `package`, or `workspace`. `files` carries the
projected source-truth files and per-file revisions; `parts` and
`part_conflicts` describe projected source parts and scan conflicts. The
workspace and package records carry their source paths, so a client can open a
child graph without inventing a path mapping. `targets` and `outputs` remain
project facts; selecting one stays local until a command authority receives
the exact choice.

## Revisions and transactions

A project transaction carries the current `project_revision` and a `files` list
with the expected revision for every touched path. The server first requires an
exact project snapshot, then checks each touched file revision before writing.
This prevents a transaction from applying to a changed projected file set even
when the requested edit names only one path.

For example, adding a workspace member uses the project transaction envelope:

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
For `create_package`, `package_path` names the new package directory or
manifest path in the operation's package-creation fields.


`preview: true` returns the diff and audit without writing. Apply mode builds an
overlay for every changed source file, runs the formatter, rechecks the Jet
front end, and evaluates the relevant package/workspace or Jetpack environment
input. A diagnostic rejects the complete candidate before publication. The
response records `touched` files and the changed source transaction rather than
claiming that a graph-side write occurred.

The shared source seam compares the expected snapshot again immediately before
publish, writes each checked candidate to a synced temporary file, and atomically
replaces the source file. A multi-file operation publishes all touched files
through the same seam. If a later publish fails, explicit before/after snapshots
restore already-published files. Conflicts and I/O failures leave source and
browser undo history at the previous committed snapshot; no Canvas graph or
semantic sidecar is created.

## Command bridge

Canvas calls existing engines and Jetpack services rather than owning a second
compiler or command implementation. The command boundary includes `jet check`,
`jet test`, `jet build`, and `jet dev`, plus the package graph, lock, catalog,
overlay, provider, provenance, and environment-realization APIs. The project
scan and transaction implementations are the source of truth for the exact
operation set.

Every action exposes authority metadata for its effects: source edits, package
fetches, environment entry, service start/stop, secrets, network/cache use,
build outputs, and touched files. Beginner UI summarizes intent; expert UI can
inspect grants, hashes, lock reasons, and the diff. An external adapter is an
explicit authority boundary, not an implicit fallback for a failed source
transaction.

## Product surfaces

The project tree may facet the same source-backed facts into:

- a workspace map for packages, members, files, imports, dependencies, targets,
  environments, services, locks, diagnostics, and dirty state;
- a package pane for manifest fields, version, edition, runtime, exports,
  targets, effects, grants, public API, and visibility;
- a dependency pane that edits `package.jet` with lock preview, visibility
  diagnostics, source channel, hash, and overlay facts;
- a targets/tasks pane that dispatches build, test, run, dev, doc, package, and
  publish through existing CLI/driver surfaces;
- a dev pane for environment packages, services, ports, logs, secrets, trust,
  and preview;
- a source graph pane scoped by package and file, with references, rename
  impact, source jumps, and package boundaries;
- a diagnostics pane grouped by workspace, package, file, target, or manifest;
- a trust/provenance pane for grants, lock reasons, envelopes, audit facts,
  service authority, and cache/network writes.

The `My Canvas` tree is the compact source-backed entry point for files,
function graphs, and typed variables. Its `New`, `Callback`, and `Add`
affordances use checked source transactions. `on_*` functions expose callback
labels and graph navigation from `event_views`; no handler graph or callback
registry is stored outside Jet source.

## Ratified boundaries

The following decision IDs define the boundary and remain citations rather than
separate data models:

- `D-CANVAS-WORKSPACE1=B`: the package/workspace graph is built over
  `workspace.jet`, `package.jet`, source files, environment source, and
  `.jet/lock`; file graphs remain child views.
- `D-CANVAS-WORKSPACE-STATE1=A`: semantic facts persist in source; private
  viewport, tabs, selection, and debug watches stay local; shared visual intent
  uses explicit source-anchored comments.
- `D-CANVAS-WORKSPACE-AUTH1=A`: cross-file edits use previewed source
  transactions with touched-file revisions, formatter and front-end proof,
  package validation, and audit payloads.
- `D-CANVAS-WORKSPACE-NAV1=A`: one semantic project tree facets packages,
  targets, files/modules, symbols, graphs, diagnostics, dependencies, and Git
  state.

## Editor architecture

The project layer and function editor share one semantic model. Jet source AST
and semantic facts are the model; the front end is the compiler; graph JSON and
checked transactions are projections and edits. Rendering, interaction, action
lookup, Details, and debugging may have separate components, but none may
become a second semantic store.

This source-as-model boundary avoids a binary graph asset, merge-only graph
state, and stale compilation. The node registry and descriptor-driven Details
surface keep projection coverage and editable fields in one source-backed
contract.

### Data-graph model

[`graph_projection.rs`](../../../crates/jet-devserver/src/Canvas/graph_projection.rs)
and [`graph_json.rs`](../../../crates/jet-devserver/src/Canvas/graph_json.rs) map
source to graph facts. A stable `node_descriptor_id` lets the render layer use
descriptor metadata instead of re-deriving style from `kind` string matching.

### Semantic node descriptors

[`node_catalog.rs`](../../../crates/jet-devserver/src/Canvas/node_catalog.rs) owns
presentation, palette, transaction, and default-editor metadata for each node
kind. The descriptor shape is conceptually:

```rust
NodeDescriptor {
    id,
    kind,
    archetype,
    projected,
    presentation: {
        label,
        glyph,
        hover,
        accent,
        header,
        style_archetype,
        layout_family,
        shape,
    },
    palette: {
        visible,
        insertable,
        category,
        rank,
        rank_terms,
    },
    transaction,
    default_editor,
}
```

Projection match functions remain in graph projection; they are not a function
field in the descriptor table. The registry validator rejects duplicate or
orphaned entries, visibility and insertability mismatches, and transaction
names that do not match the supported edit path. The graph protocol embeds the
validated descriptor data. `graph-rendering.js` consumes presentation and
hover facts; `drawing-palette.js` consumes visibility, ranking, category, and
insertion facts.

### Rendering and interaction

Keep rendering, hit testing, and interaction as separate responsibilities. The
existing browser modules—`graph-rendering.js`, `drawing-palette.js`,
`project-navigation.js`, `inspector-connections.js`, and `editing-history.js`—
should consume graph facts and descriptor metadata rather than recover meaning
from CSS classes or display labels. A single pointer state machine covers idle,
node drag, wire drag, rewire, marquee, and menu states; each transition maps to
a documented gesture.

Placement remains free. Saved view positions win over automatic layout; Tidy is
explicit. A compatible pin drop creates a checked source transaction, while a
staged palette node remains local until a compatible wire materializes it.
Unknown or stale source revisions refuse the operation without mutating source.

### Source-sync bus

[`edit_actions.rs`](../../../crates/jet-devserver/src/Canvas/edit_actions.rs)
keeps the edit transaction boundary. Each operation validates the requested
revision, produces ordinary Jet source, formats and checks it, then publishes
through the shared source seam. Undo, redo, source-backed paste, comment hints,
collapse hints, and convergence previews use the same revision-guarded path.

### Details panel

Details uses field descriptors rather than per-selection HTML:
`{label, value, editable, apply_op}`. Node, variable, and function views return
field arrays; one renderer turns them into rows and an Apply action. A field is
live only when `apply_op` names a checked source transaction; otherwise it is
absent, never a dead control. Composite values retain one source expression and
inline anchor. Leaf validation preserves the original source when the edit is
invalid or stale.
