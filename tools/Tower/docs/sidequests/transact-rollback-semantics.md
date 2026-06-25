# Plan: Scoped transactions (`#Transact`) rollback semantics — **DONE**

**Status (2026-06-25): SHIPPED — all three rollback layers built, exampled, and
tested.** This card (c72) is complete. The section below is the accurate state
for a cold handoff; the only open items are two explicitly-deferred,
ratified-as-optional corners (see "Optional follow-ups").

## What shipped

`#Transact(name) { … }` opens a transaction block (D-TXN1–D-TXN4 +
D-TXN-ROLLBACK, all ratified). On a clean exit it commits; on a `?`-failure or
early return it rolls back. Three layers, beginner-magic → expert-control:

1. **Layer 1 — auto-snapshot (magic default).** The compiler snapshots every
   root local/param the block *assigns* and restores them LIFO on failure. Zero
   annotation. Runtime lives in the vetted `jet_txn` prelude module
   (`Source/Prelude/Core.rs`), stripped from the golden memory-safety check like
   `jet_mem` (I1); the one raw-pointer writeback is sound because the
   transaction guard is dropped before the place it points at (LIFO teardown).
2. **Layer 2 — the `Rollback` trait (expert custom snapshot).** A type may
   `impl T: Rollback { type Snapshot = …  fn snapshot(self) -> Snapshot
   fn restore(~self, snap: ^Snapshot) }` to supply a cheap snapshot instead of a
   full clone. `#Transact` dispatches to `snapshot()`/`restore()` via
   `jet_txn::snapshot_custom` (`Source/Codegen/TIR/emit.rs`). The synthetic
   `user_Rollback` Rust trait is emitted only when a program references
   `Rollback` (`emit_synthetic_rollback_trait`, `Source/Codegen/mod.rs`), so
   non-users are byte-identical. `restore` takes `^Snapshot` (owned) to sidestep
   c150 (borrowed-param-to-field move bug) — semantically correct, restore
   consumes the snapshot.
3. **Layer 3 — explicit `name.on_rollback(() => { … })`.** A hand-written undo,
   Drop-backed, runs LIFO on failure, dropped un-run on commit. Mirror of
   `name.on_commit(…)`.

Irreversible effects (Net/Fs/Exec) directly in the block are rejected (E0746).

**Prerequisite cleared:** layer 2 needed working associated types (`type
Snapshot`); D-LIB2 associated-type *resolution* shipped as c149 (example
`113_associated_types.jet`, E0913 enforcement), which unblocked and delivered
layer 2.

## Where it lives

- Runtime: `Source/Prelude/Core.rs` — `JetTransaction`, `jet_txn::snapshot`
  (layer 1), `jet_txn::snapshot_custom` (layer 2).
- Codegen: `Source/Codegen/TIR/emit.rs` (snapshot dispatch + `.commit()`),
  `Source/Codegen/mod.rs` (`emit_synthetic_rollback_trait`,
  `program_has_rollback_impl`), `Source/Codegen/Imports.rs`.
- Syntax: `Source/Syntax.rs` — `TRAIT_ROLLBACK`, `TXN_ON_ROLLBACK`.
- Example: `examples/features/110_transact.jet` (layers 1+3, 6 paths),
  `examples/features/122_rollback_trait.jet` (layer 2, hand-impl).
- Tests: `tests/rollback.rs` (3) + effects suite (47/47) + golden.

## Accepted-by-design boundaries (NOT bugs)

The ratified D-TXN-ROLLBACK=C design deliberately scopes the *magic* layer and
leans on the expert layers for the rest:

- **Layer-1 auto-snapshot only catches roots the block directly assigns.** A
  value mutated *only* through a `~self` method call, or through a deep alias,
  is not auto-snapshotted. The fix for those cases is the expert tier: impl
  `Rollback`, or register an explicit `on_rollback` undo. This is the
  beginner-magic / expert-control split working as intended.
- **`on_rollback` composes with (does not suppress) layer-1 snapshots** — both
  run on the failure path, in LIFO registration order.

## Optional follow-ups (only if "100%" should close the corners above)

These are not required for the ratified contract; spec them as a *new* small
card if the owner wants them:

- **F1 — `derive Rollback`** (Snapshot = Self, field-wise full clone). Low
  value: it is behaviorally identical to layer-1 auto-snapshot (both full-clone),
  so it only adds trait-story symmetry for a type that wants to be explicitly
  Rollback-typed elsewhere. ~half a day: add `Rollback` to the derivable set,
  emit a `user_Rollback` impl with `type Snapshot = Self`,
  `snapshot(self)->Self` = clone, `restore(~self, ^Self)` = overwrite.
- **F2 — extend layer-1 auto-snapshot to `~self`-method mutations** of
  block-local roots (deeper escape analysis in the snapshot-root collector). This
  shrinks the expert-tier-required surface. Bounded but real sema work; pin new
  golden paths.

## Done criteria (met)

- [x] `#Transact` block + commit, all three rollback layers.
- [x] `Rollback` trait recognized, dispatched, assoc `type Snapshot` resolves.
- [x] Example + golden + `tests/rollback.rs` green; effects 47/47; full suite green.
- [x] All `unsafe` confined to the `jet_txn` prelude (I1); rustc never speaks (I2).
