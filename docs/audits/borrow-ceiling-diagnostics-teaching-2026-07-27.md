# Borrow ceiling: checker precision, diagnostics, and teaching (#1164)

Closeout after the three capability ports (#745 zero-copy parser, #1162 indexed
simulation, #1163 owner-backed collection). No new mechanism. No lifetime
syntax. No reopen of D-MEM1.

## Classification

- D-MEM-VIEWRET1 already ships returned and stored `View` / `ViewMut` with
  public provenance. Stale teaching that said provenance was "not carried
  through APIs and TIR yet" is wrong.
- D-SHAPE-PLACE1 / `Shared<T>` / `Pool<T>` do not need new surface here.
- Nested `View<str>` into a collection element's `String` field remains the
  teaching ceiling under today's string-view rules (#1163). Multi-owner
  provenance choice stays on #1197.

## Precision fixes

1. Direct `View`/`ViewMut` returns no longer also walk the aggregate view
   return path. Local-owned `return list[a..b]` reports **E2305 once**.
2. Non-view returns no longer re-check string-view idents as view escapes.
   `return d` when `d` is a string view into a local and the return type is
   `String` reports **E2307 only** (copy with `~`), not a second E2305.
3. UI snapshots for view fixtures no longer claim `View` is an unknown type
   (stale E0119 noise after `View` became a core generic).

## Teaching fixes

1. `report_view_return_boundary` / `report_string_view_boundary` now describe
   the shipped stable-source rule, not a missing TIR feature.
2. Owned `String` into a `View<str>` field uses a dedicated E2307 that names
   the ceiling: use `.trim()`/`.after()`/`.before()`, return a `View` of the
   owning element, or store owned `String` and copy with `~`.
3. `docs/spec/spec.md` Named-views section matches D-MEM-VIEWRET1=B and points
   at `returned_views.jet` / `owner_backed_views.jet`.

## Evidence

- `local_owned_view_return_reports_e2305_once`
- `string_view_as_owned_return_teaches_copy_once`
- `owner_backed_collection_rejects_plain_string_as_view_str_field` (message)
- UI: `tests/ui/owned_string_as_view_str.jet` plus refreshed view snapshots
