# General Database Driver Interface

**Card:** c117 / c29iz43a. **Decision:** D-DBDRIVER1=A. **Status:** ready to build.

## Goal

Define one `core.db` driver interface with parameterized queries only, and make
SQLite the first implementation. Raw string-built execute APIs stay out of the
generic interface.

## Surface

No new syntax. Use existing trait, fallible-result, and effect machinery.

Initial API shape:

- `Driver.connect(...) -> Connection ? DbError`
- `Connection.query(sql: String, params: [DbParam]) -> Rows ? DbError`
- `Connection.query_one(...) -> Row ? DbError`
- `Connection.execute(sql: String, params: [DbParam]) -> Int ? DbError`
- transaction helpers reuse `#Transact` where applicable.

The API accepts SQL text plus a separate parameter list. It must not expose a
generic `execute_raw(sql)` escape.

## Build Plan

1. Specify `DbParam`, `DbValue`, `Row`, `Rows`, `Connection`, and `Driver` in
   `core.db`.
2. Refactor existing SQLite calls to implement/use the shared interface.
3. Sema/corelib:
   - register the generic method signatures;
   - keep return/error types coherent with existing `core.db`;
   - reject raw execute through a dedicated diagnostic only if an old raw path exists.
4. Codegen/TIR routes all DB calls through the same helper templates.
5. Examples:
   - SQLite test DB with parameterized query;
   - injection-looking input remains data;
   - transaction example if the current transaction layer is available.
6. Tests:
   - parameter round-trip for ints/text/bools/null;
   - query_one no-row behavior;
   - no raw execute surface in `core_module_items`.

## Verification

- `nix develop -c cargo test --test tir core_db`
- `nix develop -c cargo test --test golden`
- `nix develop -c cargo test`

