# Epoch 7 — jetos and visual proof tooling

**Status:** mixed. The first `jet os` runtime slice is ratified and shipped:
config discovery, check/build/switch/rollback/generations/init/lift/image,
full-word option namespaces, generation names, installer proof artifacts, and
proof-before-switch checks. Remaining Epoch 7 work here is Studio, deeper visual
proof tooling, and public GUI/proof protocols.

This folder holds durable plans for the two GUI/proof cards:

- [`blueprint-editor.md`](blueprint-editor.md) — #182, Blueprint-class visual
  editor for Jet source.
- [`jetos-studio.md`](jetos-studio.md) — #235, jetos Studio over canonical Jet
  modules, with plan/diff/proof before activation.

## Law inherited

- `D-WD7`: jetos Studio is a GUI/source editor over canonical Jet modules with
  diff preview and expert provenance. No GUI-owned split-brain state.
- `D-WD8`: jetos activation has plan/diff; VM proof and rollback proof are
  required for risk classes such as boot, kernel, filesystem, and service
  changes.
- `D-WD12`: `jet prove` is a progressive proof/replay product over contracts,
  refinements, effects, budgets, property tests, and replay facts.
- `D-JPK-OS*`: ratified `jet os` command, host, generation, namespace, init,
  secret, image, disk, and disable-policy surfaces now define the CLI slice.
- `D-JOS-STUDIO-*`: ratified Studio launch/view/state/proof/protocol choices
  define the GUI plan, but implementation remains future work.

## Shared architecture rule

Both cards are projections over source facts. The checked Jet program remains
the truth. UI state may cache layout, scroll, search, collapsed panels, and
last-opened views under generated state, but it must never own semantic data.
Every persistent semantic change is a Jet source edit, a lock/proof artifact,
or a generated build artifact with provenance.

## Implementation order

1. Build shared semantic-index and source-edit transaction services.
2. Ship read-only projections before write-capable flows.
3. Add write flows only through formatter-backed text edits.
4. Add proof/live/debug overlays after the underlying CLI fact producers exist.
5. Gate every user-facing command, GUI default, persistent artifact, and public
   protocol through Tower ballots before implementation.
