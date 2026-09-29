# #3094 migration/transaction ownership and #3105 SQL binding/scope — evidence

Date: 2026-09-29. Binary: `jet-debug-snapshot14` (via `~/.cache/jet-luna/safe-jet.sh`).

## Status: BLOCKED (core.db compile)

Every program with `use core.db` fails to compile on `run`, `run --interpret`
and `build` because `Core/db/db.jet` fails the checker:

```
E0358 `DbPool` is spelled `DBPool`                        Core/db/db.jet:28
E2417 Explicit failure domain `String (text)` is not an Error type   :32 :64 :68 :72 :76 :80
E2404 `?` can't turn the `Err` from has_policy_control / is_policy_identifier_start / is_policy_identifier_continue into `String`   :35 :38 :43
```

Observed on `io/db`, `io/db_checked_sql`, `io/db_policy`, `io/db_pool`,
`io/db_jsonb_boundary` (all three tiers). Main: the DBPool/error-domain
cutover lands in snapshot16/17; the witnesses below are ready for that run.
The `core.db.*` intrinsic bypass is not used as proof (the contract is the
public facade).

## #3094 witness: `Examples/features/io/db_migration_proof.jet`

SQLite file `/tmp/jet_db_migration_proof.sqlite`. Prints: `v1` steps; `v2`
(first step creates `audit`, second fails) outcome; applied-record counts for
`v1`/`v2` from `__jet_migrations`; whether `audit` exists; commit and failed
batch row counts; a write from a **second connection** after commit and after
the failed batch (a lingering transaction or leaked connection would hold the
SQLite write lock); outer and nested `begin()` values; rows after rollback;
query after `close()`.

Provider source read (what the run should show):
`crates/jet-codegen/src/Prelude/CoreLib/JetStd/DBPluginWire.rs:2410-2438`
`jet_db_transaction`: `begin`, execute each step, on any error `rollback` and
return the error; on commit failure `rollback` and `Err("could not commit
transaction")`. `jet_db_migrate` (:2440-2447) routes through
`jet_db_migration_request` with a ledger table `__jet_migrations` (:1938,
insert at :2191 with a `status` column). No savepoint API exists in
`Core/db/db.jet` (only `transaction` and `migrate`, :91-97).

c4 (no database entity becomes an endpoint schema): OpenAPI projection lives
in `crates/jet-codegen/src/Prelude/CoreLib/Top/OpenAPI.rs`; its public
entry points take `Vec<JetOpenApiRouteInput>` / an `HTTPRouter`
(`jet_web_openapi_route_facts` :1245, `jet_web_openapi_document_from_routes`
:1254, `Core/web/web.jet:90` `openapi(router)`). A case-insensitive search of
OpenAPI.rs and `crates/jet-jit/src/net_http_hosts.rs` for
`db|sql|table|entity|DBScope|DBValue|migrat` finds no database reference.
Route types are the only schema source. **c4 met by source inspection.**

## #3105 witness: `Examples/features/io/db_bound_values.jet`

Injection-shaped name `x'); DROP TABLE person; --` through a hole, read back
verbatim, row count intact; a hole in table-name position; `owner == user`
scopes for alice and bob with per-scope counts; alice inserting with
`owner = bob`; an owner-changing UPDATE; a failed policy-scoped transaction
leaves alice's count unchanged; a query after `close()`.

Classification of the silent INSERT owner rewrite (plan change 2):
`DBPluginWire.rs:948-971` replaces the bound owner value with the scope user
(`scoped_params[owner_index] = DBValue::Text(user)`) instead of rejecting a
mismatch, while an UPDATE that changes the owner is refused (:972-976).
D-DBPOLICY-BIND1=A (card #1160, option A text): "Prelude applies the predicate
to returned rows and write targets." The law does not say whether a mismatched
write target is rewritten or refused. The rewrite silently stores different
data than the program wrote, and is asymmetric with the UPDATE refusal. This is
an owner-level ambiguity; the recommended follow-up is a bug card asking for
refusal (`owner policy does not allow writing another user's owner value`),
decided by the owner. Not changed here. The witness prints the stored owner of
row 5 so the golden records whichever behavior ships.

Owner-policy constraint seen in source (:949-960): an owner-scoped INSERT must
bind every column through a hole (`?`), or it fails with "owner policy requires
INSERT columns and `?` values with an owner column" / "bind count does not
match". The witness binds every value.

## Verdicts

- #3094: BLOCKED. c4 met (above); c1-c3, c5 await the witness run.
- #3105: BLOCKED. All criteria await the witness run; owner-rewrite
  classification recorded above.
