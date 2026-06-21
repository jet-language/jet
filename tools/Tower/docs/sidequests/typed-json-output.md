# Plan: Typed JSON output / struct serialization (D-JSONOUT1)

**Status: plan — awaiting owner decision D-JSONOUT1.**

Unblocks: **Elena** (emit JSON from typed structs without hand-building the
dynamic `JSON` enum).

---

## Goal

`core.json.render`/`render_pretty` work (`30_json.jet`), but output is built by
hand-constructing the dynamic `JSON` enum (`Object(entries)`, `Array(...)`) — a
typed struct cannot serialize itself. Let a user render a struct value to JSON
directly: `json.render(order)` where `order: Order`, fields become keys.

Verified: `30_json.jet` produces compact JSON; the render path takes the dynamic
`JSON` value, not arbitrary structs. No struct→JSON serializer exists.

## Pipeline touch points

- **stdlib / sema / comptime**: serialize a struct by walking its fields. As with
  D-CSVROW1, the clean form is a `#[Serialize]` derive — but **user-defined
  derives are S56, deferred**. Note: `syntax-decisions.md` already references a
  built-in `#[Serialize]` *marker* (D-QUAL1 discussion, D-ATTR2 bare-marker form)
  distinct from user derives — confirm whether a *built-in* Serialize marker is
  already ratified-to-pursue before proposing a new mechanism.
- **codegen**: a field-walk that emits the `JSON` value (codegen stays dumb;
  sema/comptime drives the walk).
- **diagnostics**: a "type X is not serializable" error (e.g. contains a closure
  or a non-serializable handle).

## Invariants in play

- **I8 / one-path**: there should be one serialize story. If a built-in
  `#[Serialize]` marker is the intended mechanism, this plan implements *that*, not
  a parallel one. Do not reinvent S56.
- **I5** example: struct → JSON, round-trip with decode.
- Composes with **D-CSVROW1** (Elena's full pipeline) and the lenient-decode
  coercion logging already shipped (c10 / D-JSON3).

## Open questions (need owner decision — D-JSONOUT1)

1. **Mechanism** — (a) built-in `#[Serialize]` marker that the compiler honors via
   comptime field reflection (no user-derive system needed); (b) an explicit
   `to_json(self) -> JSON` method the user writes per type; (c) wait for S56
   user-derives. Confirm (a)'s status against D-ATTR2/D-QUAL1 first.
2. **Field-name mapping** — struct field names verbatim, or a rename attribute
   (`#json("user_id")`)? (rename interacts with the marker form.)
3. **Optionals / nulls** — `None` → omit key vs emit `null`? Make it explicit.
4. **Nested + collections** — recursive serialize of nested structs and `[T]`/
   `Map` fields; what's the v1 boundary (e.g. distinct types via `.raw()`)?
5. **Symmetry with decode** — should the same marker drive *both* render and the
   typed decode (D-CSVROW1 / a JSON `decode<T>`), so one annotation covers in+out?

## Test plan

1. `examples/features/json_typed.jet` — serialize an `Order` struct to JSON, then
   decode it back; golden output (I5).
2. Nested-struct + list-field serialize test.
3. `None`-handling snapshot (omit vs null per the decision).
4. Non-serializable field → diagnostic snapshot.
