# Indexed simulation borrow-ceiling audit

Card #1162 ports a particle/grid update that keeps two indexed particle edit
windows live while it reads forces from a grid.

## Classification

- D-SHAPE-PLACE1 covers the port. `&particles[0]` and `&particles[2]` are
  constant, disjoint places. Sema accepts both exclusive windows, and TIR
  lowers them through safe `split_at_mut` operations.
- D-MEM-VIEWRET1 does not apply. No view is returned or stored.
- `Shared<T>` does not apply. The update has one local owner and does not need
  cross-task shared mutation.
- `Pool<T>` and `Id<T>` do not apply. The simulation indexes one stable list
  during the update and does not need persistent generational identity.

## Result

The existing production native path accepts the complete static-index update.
It changes particles 0 and 2 while both edit windows are live, preserves the
middle particle through an owned copy, and prints `17,20,45`. Generated user
code uses safe structural splits. No raw reference, lifetime syntax, fallback,
skip, or second view mechanism is present.

No compiler precision bug was found. A second edit of `particles[0]` is hostile
overlap and fails in sema with E0212. Two runtime-selected indexes also fail
with E0212 because their places are conservatively overlapping. A source-level
claim that the indexes differ is not a proof. Runtime-disjoint access remains
the owner-gated D-MEMDISJOINT1 question on card #1198 and is not implemented or
routed around here.

Default development evaluation reports E0956 because its TIR evaluator does
not yet execute split-view statements. This is the existing recorded
`memory/place_windows` JIT boundary, not an alias-checker failure. The
integration test selects the explicit production native path with
`jet run --release`; it does not try and fall back from another backend.

## Evidence

- `indexed_simulation_static_update_lowers_to_safe_splits` proves successful
  front-end acceptance and safe structural-split lowering.
- `indexed_simulation_rejects_dynamic_disjointness_claim` proves the #1198
  boundary fails closed with E0212.
- `indexed_simulation_rejects_hostile_overlap` proves same-index mutable
  aliasing fails closed with E0212.
- `indexed_simulation_example_runs_production_pipeline` runs the executable
  memory example through the native production CLI and checks exact output.
- The scoped `memory/place_windows` golden builds generated Rust, enforces the
  generated-unsafe policy, and checks the same output.

Independent fresh-context review is required before card closure.
