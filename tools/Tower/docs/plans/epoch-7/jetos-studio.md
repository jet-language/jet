# jetos Studio — GUI over canonical Jet modules (#235)

**Status:** durable plan. Product law exists (`D-WD7`, `D-WD8`, `D-WD12`) and
Studio surface ballots are ratified:

- `D-JOS-STUDIO-LAUNCH1=A`: users open Studio with `jetos studio`.
- `D-JOS-STUDIO-VIEW1=A`: default view is guided control center plus source,
  diff, and provenance.
- `D-JOS-STUDIO-STATE1=A`: Studio may persist generated local UI cache only,
  never semantic settings outside Jet source.
- `D-JOS-STUDIO-PROOFGATE1=B`: risky activation is hard-gated by risk class.
- `D-JOS-STUDIO-PROTOCOL1=C`: schemas become public after read-only Studio and
  before switch flows.
- `D-JOS-STUDIO-HOST1=A`: Studio uses one local Jet-owned projection/edit
  service. Studio is separate from Canvas. Default launch opens the installed
  first-party jetos Studio app from the jetos system profile when available;
  browser and headless review modes are fallbacks over the same protocol.

## Goal

Studio is a first-party GUI for editing, reviewing, proving, and applying jetos
configuration. It edits canonical Jet modules, shows the exact source diff, and
surfaces proof/rollback facts before activation.

Studio is not Canvas. Canvas is the general source-backed visual code editor.
Studio is the jetos control-center application installed into a jetos system,
with a browser fallback that consumes the same local projection/edit service.
Current system generations install `sw/bin/jetos-studio`,
`share/applications/jetos-studio.desktop`, `studio/app.json`, and
`studio/data.json`; the root-shaped projection exposes them under
`/run/current-system`. `jetos studio --serve <loopback:port>` serves the app and
projection data as the browser fallback. `GET /studio/source` serves the
selected `config.jet` for the source pane. `POST /studio/transaction` supports
the first source transaction, `set-option`, returning a source diff and writing
`config.jet` only when requested. `POST /studio/run` shells back through the
canonical `jet os check|plan|build|proof|generations` commands for the selected
host and returns captured output for Studio status panes.

Beginner path: a control-panel UI lets someone enable services, users, fonts,
packages, backups, and desktop settings without learning the module language
first.

Expert path: every screen can expose the exact option declaration, module source,
merge provenance, package closure, grants, generated files, VM proof, rollback
proof, and JSON artifact.

Hybrid path: the GUI is another source editor over the same modules. There is no
GUI database, no generated private config language, and no setting that cannot
be reviewed as Jet source.

## Target architecture

```
Jet modules
  -> parser/sema/module evaluator
  -> option schema + merged system plan + provenance
  -> Studio projection service
  -> GUI model
  -> source edit transaction
  -> formatter + diff preview
  -> proof broker
  -> plan / VM proof / rollback proof artifacts
  -> activation handoff
```

**Studio projection service.** Reads module facts, option declarations, merge
results, lock/provenance data, and proof artifacts. It never invents facts.
This service is the D-JOS-STUDIO-HOST1 host boundary: the installed jetos Studio
app, browser fallback, CI screenshots, and future remoting all consume the same
local protocol.

**Source editor.** Every control writes a named edit transaction to Jet source:
set option, add module import, enable service, add package, remove field, split
module, move setting. Transactions produce a diff before saving.

**Proof broker.** Wraps future `jetos plan`, VM proof, rollback proof, and
`jet prove` lenses. It stores artifacts under generated state with stable JSON
schemas for CI and GUI replay.

**Activation handoff.** Studio never activates by hidden side effect. It hands a
proved generation candidate to the ratified jetos activation command after the
user confirms the diff and proof state.

## Data model

`StudioWorkspace`:

- source root and active host
- module documents and parse/check status
- option schema index
- merged plan snapshot
- lock/provenance snapshot
- proof run index

`ModuleDoc`:

- file path, module declarations, imports/find roots
- source spans for each setting
- comments/doc text anchored to setting or declaration spans

`OptionSchema`:

- option path, type, default, docs, allowed values, risk class
- owning module/package
- proof requirements derived from risk class and policy

`SettingBinding`:

- option path, current value, source span, priority/merge rule
- origin module, previous overridden values, provenance edges

`ChangeSet`:

- edit transactions
- formatted text diff
- semantic diff: added/removed/changed options, packages, services, files,
  grants, effects, generated artifacts, and risk classes

`ProofPlan`:

- required checks from risk classes and policy
- plan-only checks, VM boot checks, service health checks, rollback proof,
  replay/prove lenses

`ProofRun`:

- inputs fingerprint, toolchain/lock identity, artifacts, stdout/stderr,
  diagnostics, pass/fail, rerun reason

`GenerationCandidate`:

- source revision, lock identity, merged plan fingerprint, proof fingerprint,
  generation name, activation target, rollback target

## Synchronization rules

- Source is truth. GUI state is thrown away and rebuilt from source whenever the
  source revision changes.
- Edits are transactions against spans and semantic paths. If a setting moved,
  the transaction rebases by option path; if two edits conflict, Studio shows a
  source diff conflict.
- Unknown text stays visible. Studio may render an "unsupported by this panel"
  row, but expert source edit remains available.
- Comments are preserved. A GUI description edit writes doc/comment text only
  where the corresponding source convention is ratified.
- Generated UI cache may store panel layout, collapse state, filters, and last
  proof run pointer, but never semantic settings.
- Proof artifacts are immutable facts keyed by source+lock+toolchain+plan. A
  source edit invalidates stale proof visibly.
- Activation requires current source revision, current diff approval, and
  current proof status matching policy.

## Implementation slices

### ST0 — read-only source-backed Studio app

Render discovered modules, parse/check diagnostics, option paths that exist
today, and raw source panes behind `jetos studio`. Default launch targets the
installed jetos Studio app; if unavailable, it serves and opens the browser
fallback. No jetos activation.

Exit: open fixture repo in the Studio app or browser fallback, show module tree,
source pane, diagnostics, and empty proof state. No write path.

### ST1 — option schema and provenance explorer

Show typed options, docs, defaults, current value, value origin, overridden
values, and merge rule in guided panels with source/diff/provenance one click
away.

Exit: fixture with three modules renders one merged value tree and provenance
edges; scalar conflict points at both source spans.

### ST2 — write transactions and diff preview

Add set option, enable/disable service, add package, edit user, create module,
and split module transactions. Every transaction shows formatted source diff and
semantic diff before save.

Exit: transaction tests prove diff -> save -> re-project yields the same GUI
state; no GUI-owned state file changes.

### ST3 — proof dashboard

Attach plan, risk classification, VM proof, rollback proof, and `jet prove`
lenses. Minor low-risk changes can show plan/diff only; boot/kernel/filesystem/
service-risk changes require VM and rollback proof per `D-WD8` and
`D-JOS-STUDIO-PROOFGATE1=B`.

Exit: proof fixtures cover pass, fail, stale, skipped-by-policy, and missing
producer states.

### ST4 — generation candidate and activation handoff

Studio creates a generation candidate from the current source/lock/proof
fingerprints and invokes the ratified activation command only after confirmation.
Public plan/proof schemas must exist before this write path ships.

Exit: dry-run activation harness proves Studio cannot activate stale or
unproved candidates.

### ST5 — lift/import assistant

Add guided import from existing machine/module sources after the corresponding
jetos lift/import surface is ratified. Generated Jet source is canonical and
editable; TODO diagnostics carry migration status.

Exit: import fixture creates modules with TODO diagnostics and no hidden state.

## Test plan

- Link sanity: every plan/proposal/doc index path resolves.
- Projection fixtures for module tree, option schema, merged values, provenance,
  lock identity, and proof state.
- Transaction round-trip: GUI edit -> formatted source diff -> reparse ->
  identical semantic value.
- Split-brain guard: no semantic data written outside Jet source, lock, proof,
  or generated build artifacts.
- Proof policy tests: risk-class matrix, stale proof invalidation, CI JSON
  artifact schema, VM proof failure, rollback proof failure.
- GUI tests: narrow/desktop screenshots, keyboard navigation, diff preview,
  source jump, provenance drill-down, proof badge states.
- Activation guard tests: stale source, stale lock, stale proof, missing
  rollback target, declined confirmation.

## Ratified surface decisions

- Launch: `jetos studio` opens the installed first-party jetos Studio app when
  available, with browser fallback over the same protocol.
- View: guided panels are default; source, diff, and provenance stay adjacent.
- State: generated local cache may remember view preferences; source remains the
  only semantic config store.
- Proof gate: boot/kernel/filesystem/service-risk activation requires proof
  before switch; lower-risk changes require plan/diff unless policy says more.
- Protocol: read-only Studio may use internal fields, but switch flows require
  public plan/proof schemas shared with CI.
- Host runtime: one local projection/edit service powers a separate jetos Studio
  app first; browser and headless review modes wrap that same protocol.
