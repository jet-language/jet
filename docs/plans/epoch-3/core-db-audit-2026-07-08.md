# core.db audit

Card: #303. Decision: D-DBMIGRATE1=A.

`core.db` keeps one semantic path: SQL text plus `[DbValue]` parameters.
Checked `Sql` literals feed that path with `db.params(sql)`. Rows remain
inspectable maps, with typed `db.row_*` helpers for product code that wants a
typed column read and a direct error when the column is missing or has the wrong
shape.

## Shipped

| Area | Status |
|------|--------|
| SQLite driver | Shipped through the approved `rusqlite` runtime bridge. |
| Parameter-only queries | Shipped: `query`, `query_one`, and `execute` require `[DbValue]`. |
| Checked SQL literals | Shipped: `db.params(sql"...")` converts holes to bind values. |
| Typed row reads | Shipped: `row_int`, `row_float`, `row_text`, `row_bool`, `row_value`. |
| Transactions | Shipped: explicit methods plus `db.transaction(conn, label, statements)` rollback on first error. |
| Migrations | Shipped: `db.migrate(conn, name, statements)` creates `__jet_migrations`, records checksum, returns `0` when already applied, errors on checksum drift. |
| Prepared statements | Shipped: `query` and `execute` use SQLite's prepared statement cache under the single query path. |

## Not A Second Path

| Request | Resolution |
|---------|------------|
| Raw execute | Rejected by D-DBDRIVER1. All SQL calls carry parameters. |
| Separate prepare API | Not added. The runtime cache preserves one path. |
| Named-parameter mini-language | Not added. Checked `Sql` holes are source names and bind positionally through the same parameter list. |
| ORM-only builder | Not added. D-DBMIGRATE1 keeps SQL inspectable; future Canvas/query-builder work must lower to this same SQL+params plan. |

## Future Driver Breadth

Postgres/MySQL remain backend-driver work, not a second Core API. They must
implement the same parameterized query, typed row, transaction, and migration
contract before surfacing. No compiler dependency may be added for them.
