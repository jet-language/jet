# Plan: Typed CSV row structs (D-CSVROW1)

**Status: plan — awaiting owner decision D-CSVROW1.**

Unblocks: **Elena** (CSV-to-JSON ETL — typed field mapping instead of index
juggling).

---

## Goal

`jet.csv` returns rows as `[String]` (confirmed by `51_csv.jet` outputting raw
rows); mapping a column to a typed field is manual indexing (`row[2].to_int()`),
brittle and unreadable. Let a user describe a row as a struct and decode each
record into it with typed, named fields and a clear error on a malformed row.

Verified: `51_csv.jet` outputs `name: score` header + rows as strings; there is
no row-struct decode path in `jet.csv`.

## Pipeline touch points

- **stdlib** (`jet.csv`): a decode that maps a record to a struct by header name
  or column order, coercing field types and producing a typed error per bad row.
- **sema / comptime**: typed decode needs to know the struct's fields and types.
  A `#[Decode]`-style derive is the clean form but **user-defined derives are S56,
  deferred to Epoch 3**. The v1-reachable forms are: a comptime reflection over
  struct fields, or an explicit field-mapping function the user writes. The
  decision must pick a path that does **not** depend on unratified S56.
- **diagnostics**: a malformed-row error naming the column and expected type,
  composable with the ratified `T ? E` / `??` skip idiom (`13_errors.jet`).

## Invariants in play

- **I8** don't invent a derive system here — that's S56's job (deferred). Pick a
  v1 form that works without it (explicit mapping or built-in comptime field walk).
- **I5** example: typed-row ETL with one malformed row skipped via `??`.
- Plays with **D-JSONOUT1** (typed JSON output) — Elena's pipeline is CSV-in →
  typed struct → JSON-out; the two should compose.

## Open questions (need owner decision — D-CSVROW1)

1. **Decode surface** — (a) a built-in comptime `csv.decode<Row>(record)` that
   reflects over `Row`'s fields (needs comptime field reflection, which exists
   per S57/S60 — confirm reach); (b) an explicit per-row mapping closure the user
   writes (`csv.rows(file).map(|r| Order{ id: r[0].int()?, … })`); (c) wait for
   S56 derives and ship `#[CsvRow]` then. Must avoid blocking on S56.
2. **Header vs positional mapping** — map by header name (robust to column
   reorder) or by position? Both, with header as default?
3. **Type coercion + failure** — per-field coercion (`"42"`→Int) failure produces
   a typed error carrying row#, column, value, expected type — define its shape so
   `??` skipping reads cleanly.
4. **Missing/extra columns** — error, or fill `Option`/default? (leaning: error,
   with an explicit opt-in for optional columns).

## Test plan

1. `examples/features/csv_typed.jet` — decode a fixture CSV into `Order` rows,
   skip one malformed row via `??`, print the typed totals; golden output (I5).
2. Per-field coercion-failure error snapshot (row + column + expected type).
3. Header-reorder test: same struct decodes correctly when columns are reordered
   (if header mapping is chosen).
4. Missing-column behavior test.
