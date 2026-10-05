# #3268 — SQLite JSONB stays at the database boundary (D-CORE-JSONB1=A)

Date: 2026-09-29. Binary: `jet-debug-snapshot14` (via `~/.cache/jet-dev/safe-jet.sh`).

## Question

Under the ratified option A, prove the boundary: stock typed JSON plus
SQLite's own `jsonb()`/`json()` carry every element category and exact
numbers, with the SQLite version printed, no Jet JSONB codec, and no
PostgreSQL or interchange claim.

## Method

- Extended `Examples/features/io/db_jsonb_boundary.jet` with one row per
  element category (null, true, false, float, negative int, i53+1,
  beyond-i64 integer, escaped text with a non-ASCII char, nested array,
  nested object). Each row hands JSON text to `jsonb()` through an SQL hole
  and prints `hex(jsonb(x))`, `json(jsonb(x))` and whether the text came back
  identical. The existing typed round trip (`json.decode<Reading>`), exact
  `9007199254740993` and the external blob `X'8c57636f756e741332'` stay.
- Checked for any Jet JSONB surface: `Compiler/JetFoundation/Source/Registry/CoreCallRows.jet`
  export tables and `Core/encoding/` have no `jsonb` module (grep).
- Engine identity: `libsqlite3-sys 0.28.0` (`Cargo.lock`), bundled via
  rusqlite (`crates/jet-jit/Cargo.toml`). The witness prints `sqlite_version()`
  at run time so the golden pins the actual build.

## Evidence

**BLOCKED on the current binary.** `io/db_jsonb_boundary` fails to compile on
`run`, `run --interpret` and `build` because `Core/db/db.jet` fails the
checker (E0358 `DbPool`→`DBPool` at :28; E2417 `String!` failure domain at
:32/:64/:68/:72/:76/:80; E2404 ×3 in `policy`). The same happens for every
`use core.db` example. No golden written.

Reference only (Python `sqlite3` with SQLite 3.51.2, NOT the Jet binary and
NOT the bundled 3.45-era SQLite, so the golden must still come from Jet):

```
null -> 00 -> null same: True
true -> 01 -> true same: True
false -> 02 -> false same: True
0.1 -> 35302E31 -> 0.1 same: True
-17 -> 332D3137 -> -17 same: True
9007199254740993 -> C31039303037313939323534373430393933 -> 9007199254740993 same: True
123456789012345678901234567890 -> C31E3132...3930 -> 123456789012345678901234567890 same: True
"tab\tquote\"é" -> C80E7461625C7471756F74655C22C3A9 -> "tab\tquote\"é" same: True
[1,[2,{"k":null}]] -> 9B13316B13323C176B00 -> [1,[2,{"k":null}]] same: True
{"a":{"b":[true,false]}} -> 8C17615C17622B0102 -> {"a":{"b":[true,false]}} same: True
json(X'8c57636f756e741332') -> {"count":2}
```

Integers are stored by SQLite as their decimal text (`INT`/`TEXT5`-style
payloads), so exactness never depends on a Jet binary decoder.

## Verdict

**BLOCKED (core.db compile).** Status per criterion:
- c1/c9 (versioned per-category fixtures, exact numbers, golden on every
  tier): witness extended; run blocked.
- c2 (direct DB ops vs external buffer): witness covers both (stored column
  and the `X'8c…'` literal); run blocked.
- c3/c7 (not a general interchange standard; version and opaque warning
  visible; no PostgreSQL claim): the witness header says so and prints
  `sqlite_version()`; the core-library.md note from the plan is not yet
  written (left to the implementer; no doc edit claimed here).
- c4/c5: no JSONB codec, EncodingFormat row or DataTree variant exists
  (registry and Core/encoding grep); stock typed JSON is the path.
- c6: moot under A.
- c8: no speed claim is made.
