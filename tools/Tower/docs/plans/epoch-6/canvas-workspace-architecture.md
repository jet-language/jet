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

- `Source/CmdDevWeb.rs` serves `/canvas` for one watched file.
- `Source/Canvas.rs` exposes one `source_id`, one `revision`, one graph
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
- future build graph commands: `jet graph`, `jet query build`,
  `jet explain-build`

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
