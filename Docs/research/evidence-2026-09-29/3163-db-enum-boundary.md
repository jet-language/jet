# #3163 — Closed Jet enums at the database boundary

Date: 2026-09-29. Binary: `jet-debug-snapshot14` (via `~/.cache/jet-dev/safe-jet.sh`).

## Question

How does a closed Jet enum cross the database boundary today (portable wire
name vs native PostgreSQL enum), and what happens on case addition, rename,
ordering and mixed-version readers? Close with existing coverage or precise,
independently decidable additions.

## Existing decisions and binding path (c1)

Read: `Core/db/db.jet` (whole file), `Examples/features/io/db_checked_sql.jet`
(D-DBMIGRATE1), `Examples/features/io/db_policy.jet` (D-DBPOLICY-BIND1=A),
`Compiler/JetFoundation/Source/Registry/CorePlatformSignatures.jet:269-283`
(core.db signatures), `crates/jet-codegen/src/Prelude/CoreLib/JetStd/DBPluginWire.rs`.

- `core.db` has no enum path. Values cross as `DBValue`; holes in an `SQL{…}`
  literal bind values; typed rows go through `db.decode<T: Decode>(row)`
  (`Core/db/db.jet:87-89`), which forwards to the compiler-lowered
  `core.db.decode<T>` and returns `T [FieldError]!`.
- A fieldless `#Codable` enum's wire form is its case name (same law as JSON:
  `Examples/features/serde/json_integer_fidelity.jet` decodes `"On"` into a
  unit enum). So the existing binding is **TEXT holding the canonical case
  name**, decoded by the `Decode` derive, unknown names rejected as `[FieldError]`.
- Engine: SQLite only (rusqlite bundled; `libsqlite3-sys 0.28.0` in
  `Cargo.lock`). There is no PostgreSQL client in the tree, so a native enum
  cell cannot run.

## PostgreSQL primary evidence (c2), read 2026-09-29

- PostgreSQL 17 §8.7 <https://www.postgresql.org/docs/17/datatype-enum.html>
  (page footer: PostgreSQL 17.11): `CREATE TYPE mood AS ENUM (...)`; "The
  ordering of the values in an enum type is the order in which the values were
  listed when the type was created"; labels are case sensitive; "Existing
  values cannot be removed from an enum type, nor can the sort ordering of such
  values be changed, short of dropping and re-creating the enum type"; each enum
  type is distinct (`operator does not exist: mood = happiness`).
- PostgreSQL 17 ALTER TYPE <https://www.postgresql.org/docs/17/sql-altertype.html>:
  `ALTER TYPE name ADD VALUE [ IF NOT EXISTS ] new_enum_value [ { BEFORE | AFTER } neighbor_enum_value ]`,
  `ALTER TYPE name RENAME VALUE existing_enum_value TO new_enum_value`; Notes:
  "If ALTER TYPE ... ADD VALUE ... is executed inside a transaction block, the
  new value cannot be used until after the transaction has been committed."

Consequences for a Jet mapping: native PG ordering is declaration order, SQLite
TEXT ordering is collation (byte) order; a Jet case rename needs `RENAME VALUE`
in PG but is a data migration (`UPDATE ... SET status = 'New' WHERE status = 'Old'`)
under TEXT; removal is impossible natively.

## Experiment (c2, c4)

Witness `Examples/features/io/db_enum_binding.jet` (new): binds each case name
through an SQL hole into `TEXT`, decodes with `db.decode<Task>`, adds a
`Blocked` case written by a newer program and read by the old (`Status`) and new
(`StatusV2`) readers, stores an unknown lowercase `active`, prints
`ORDER BY status`, and exercises a `CHECK (status IN (...))` constraint. No
string-interpolated SQL: every value is a hole.

**BLOCKED on the current binary**: every program that does `use core.db`
fails to compile because `Core/db/db.jet` itself fails the checker
(E0358 `DbPool`→`DBPool` at db.jet:28; E2417 `String!` failure domain at
:32, :64, :68, :72, :76, :80; E2404 ×3 in `policy`). Observed on
`io/db`, `io/db_checked_sql`, `io/db_policy`, `io/db_pool`,
`io/db_jsonb_boundary` for `run`, `run --interpret` and `build`. Main reports
the DBPool/error-domain cutover lands in snapshot16/17. No golden written.

SQLite-only reference (Python `sqlite3`, SQLite 3.51.2, NOT Jet) for the two
database-owned rows of the witness:

```
ORDER BY status: Active Archived Blocked Pending      # text order, not Pending<Active<Archived<Blocked
CHECK accepts Active
CHECK rejects active - CHECK constraint failed: status IN ('Pending','Active','Archived','Blocked')
```

## Verdict

**BLOCKED (core.db compile).** c1 and c2's primary-source half are done
(above). c3 closure recommendation, pending the witness run:

- Existing coverage = SQLite `TEXT` + canonical case name via `Decode` + an
  optional `CHECK` constraint listing names. No new API is needed for SQLite.
- Independently decidable additions (none is proposed for adoption here):
  1. Native PostgreSQL enums need a PostgreSQL provider first (#3308, driver
     decision, optional distribution, not stock). Only after that: a ballot on
     mapping a Jet enum to `CREATE TYPE ... AS ENUM`, with declaration-order
     sorting and `ADD VALUE` outside transactions.
  2. Whether `jet` migrations should generate the `CHECK (... IN ...)` list
     from the enum declaration (a CoreLib/migration-tool choice, stock).
- No driver is imported.
