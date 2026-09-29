# #3308 — Persistence jobs mapped to core.db

Date: 2026-09-29. Binary: `jet-debug-snapshot14`. Status: execution BLOCKED (core.db compile).

## Blocker (observed)

`safe-jet.sh run Examples/features/io/db_pool.jet` and `.../db_checked_sql.jet` both fail inside Core/db/db.jet:
`E0358 DbPool is spelled DBPool` (:28:43), `E2417 explicit failure domain String is not an Error type` (:32:57),
and `E2404` (:35:71, :38:9). Every `use core.db` program is uncompilable, so no db cell can run. Main assigned the fix to WaveDbFix.
The draft witness is parked at `~/.cache/jet-test-scratch/Closer01/parked/db_contracts.jet`; it never got past Core/db.

## Mapping (criterion 3, source read)

Sources: Core/db/db.jet, `Compiler/JetSema/Source/Sema/Calls/Database.jet:15-74`, `crates/jet-jit/Cargo.toml` (rusqlite bundled).

| peer job | Jet owner | status |
|---|---|---|
| SQLite (Python `sqlite3`, rusqlite) | core.db: `open(url)`, `open_memory()`, `DBScope.query/query_one/execute`, `SQL{…{hole}…}` bound values, `db.decode<T>(row)`, `transaction`, `migrate`, `begin/commit/rollback/close` | shipped engine (the only one) |
| JDBC / Go `database/sql` / ADO.NET: connection + prepared statement + pool | the same core.db provider model; `pool(url, max)` → `DbPool.acquire(deadline:)`, `ready`, `receipt`, `drain`; `DbLease.close` | pool shipped (D-FOUND-COREAPI1). A `DbLease` has only `close`: no query through a lease |
| PostgreSQL / MySQL / SQL Server drivers | none | independent owner/dependency choice. A registry or URL name does not imply parity |
| dbm / key-value file store (Python `dbm`, `shelve`) | none; nearest is SQLite through core.db, or core.files | not provided; needs its own ballot if wanted |
| statement cancellation (`Statement.cancel`, `ctx` cancel) | none; no DB handle has `cancel` | uncovered; needs an owner-gated surface |
| JSONB / binary column encoding | separate owner (#3163-era db enum/JSONB work); not part of this map | — |

## Verdict

- Criterion 3 is met by the mapping above: engines and drivers are independent owner choices, and no parity is inferred.
- Criteria 1, 2 and 4 are BLOCKED on the core.db compile failure.
