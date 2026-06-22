# Plan: Scoped transactions (`#transact`) rollback semantics

**Status:** planned, gated on the D-EFF2/D-EFF3 effect-system sub-decisions.

## Decisions

- **D-TXN1=A**: `#transact { }` rolls back mutations on `?` failure by calling a
  `Rollback` trait in reverse mutation order. Clean exit commits.
- **D-TXN2=A**: irreversible effects inside `#transact { }` are rejected. The fix is
  to move the effect after the block or queue it in `on_commit { }`.

## Scope

This plan implements the semantic contract only after the D-EFF1 engine is buildable.
D-EFF1 is itself gated on D-EFF2 and D-EFF3, so this card stays planned rather than
building.

## Implementation Steps

1. Add `Rollback` as a blessed trait in the core prelude surface.
2. Parse and preserve `#transact { }` as a scoped effect region.
3. In sema, track mutable values touched inside the region and require each touched
   type to implement `Rollback`.
4. Use the effect classifier from D-EFF1 to reject irreversible effects in the block.
5. Add `on_commit { }` as the explicit home for irreversible work that should happen
   only after a successful transaction.
6. Lower the block to normal code plus reverse-order rollback calls on failure paths.

## Diagnostics

- Non-`Rollback` mutation in `#transact` names the type and suggests implementing
  `Rollback` or moving the mutation outside the block.
- Irreversible effect in `#transact` names the effect and suggests moving it after the
  block or wrapping it in `on_commit { }`.

## Tests

- UI snapshot: successful rollback path with two touched values.
- UI snapshot: mutation of a non-`Rollback` type.
- UI snapshot: irreversible net/fs/subprocess effect inside `#transact`.
- Golden example: failed update rolls back prior mutations; successful update commits.
