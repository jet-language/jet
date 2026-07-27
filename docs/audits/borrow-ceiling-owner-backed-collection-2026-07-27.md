# Owner-backed collection borrow-ceiling audit

Card #1163 ports a library that owns books in a field and exposes element
windows through the shipped `View` / `ViewMut` boundary.

## Classification

- D-MEM-VIEWRET1 covers returned `View<Book>` and `ViewMut<Book>` windows into
  `Library.books`. Sema records parameter provenance; codegen lowers mutable
  field writes through the slice element.
- D-SHAPE-PLACE1 covers local exclusive windows and the hostile resize check.
  A live view into `lib.books` blocks `push` / replace with E0212.
- `Shared<T>` does not apply. One local owner holds the collection.
- `Pool<T>` and `Id<T>` do not apply. The port needs stable list indexes for
  the update, not generational identity.

## Result

The production native path accepts the complete owner-backed port. It returns a
read window, prints the selected title and page count, then returns write
windows that update the owning `Library.books` storage in place
(`412 → 422`, `271 → 280`). Inclusive ranges select one element with `i..i`.

Two checker precision failures were fixed on this card:

1. Declared `View<str>` rejected `str` as an unknown type (E0119) even though
   struct fields already accepted that spelling. `check_declared_type_rules`
   now allows `str` only as the `View` type argument.
2. `ViewMut<T>[i].field` assignment cloned through `jet_index_vec` and mutated
   a temporary, so the owner never changed. Index-field assign now treats
   `View` / `ViewMut` like list storage and writes through.

Nested `View<str>` into a `String` field of a collection element remains a
true teaching ceiling under today's string-view rules: only
`trim` / `after` / `before` (or an already-tracked string-view binding) may
fill a `View<str>` slot. Filling from a plain owned `String` place is E2307
(previously a false-green that became an ICE). Use `View<Book>` and read the
title through the element window, or materialize with `~`. Multi-owner
provenance choice stays on #1197 / D-MEMPROVENANCE2.

## Evidence

- `owner_backed_collection_returns_element_views` proves read and write
  windows compile with parameter provenance and write-through lowering.
- `owner_backed_collection_rejects_resize_while_view_live` proves E0212.
- `owner_backed_collection_rejects_plain_string_as_view_str_field` proves the
  nested `View<str>` ceiling fails closed with E2307.
- `owner_backed_collection_example_runs_production_pipeline` runs the
  executable memory example through the native production CLI.
- Scoped golden `memory/owner_backed_views` checks the same output.
