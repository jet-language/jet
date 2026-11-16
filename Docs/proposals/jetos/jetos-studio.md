# jetos Studio — GUI over canonical Jet modules

Product law and the Studio surface decisions are recorded in the ratified
decision set below. This page keeps the source-backed surface and its rationale.

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

## Install-trust surface

Studio renders `D-JOS-INSTALLTRUST1=A` as a capability-first install flow:

- Anyone may install software they choose or build. An owner-built local
  install and `jet run` do not require Jet notarization, catalog approval, or a
  category decision.
- Third-party and foreign apps run sandboxed with deny-by-default access to
  files, network, devices, and agents. Studio shows each requested capability
  and records an explicit grant in Jet configuration; installing an app never
  grants authority implicitly.
- Hangar hashes, optional publisher signatures, provenance, and catalog
  ranking are identity and supply-chain signals. Studio may warn about an
  unsigned remote artifact, but those signals never become a hard gate on
  running owner software.
- Generation and rollback facts stay visible with the install so a bad install
  can be repaired without hiding the recovery path.
- Family and enterprise locks are expert opt-in tightenings. They are not the
  beginner default, and the same law applies to future jetos device classes
  unless a later ballot amends it.

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
app, browser fallback, CI screenshots, and remoting all consume the same local
protocol.

**Source editor.** Every control writes a named edit transaction to Jet source:
set option, add module import, enable service, add package, remove field, split
module, move setting. Transactions produce a diff before saving.

**Proof broker.** Wraps `jetos plan`, VM proof, rollback proof, and `jet prove`
lenses. It stores artifacts under generated state with stable JSON schemas for
CI and GUI replay.

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
