# Framework transplants E3 closeout

Shipped-law reconciliation for D-LIVEQUERY1, D-SCHEDULE1, D-AUTH1, D-SYNC1, and
related framework transplant cards (#505/#506/#1157–#1161).

## Proposal vs shipped

| Law | Proposal claim | Shipped proof |
|-----|----------------|---------------|
| D-LIVEQUERY1=A | Live queries + `#Transact` write-set invalidation + Signal push | `Prelude/CoreLib/Top/LiveQuery.rs`; `examples/features/tooling/app_live.jet` |
| D-SCHEDULE1=A | `#Every` on `#Job`/`#Task` only; one schedule fact | Syntax + `CheckerSchedule.rs`; `examples/features/devloop/schedule_every.jet`; UI E0925/E0926 |
| D-AUTH1=A | sessions, password, OAuth, magic link | `AuthSession.rs` / `Auth.rs`; `examples/features/crypto/auth_sessions.jet` |
| D-SYNC1 | CRDT sync values | SyncLite + sync examples (card #1159 done) |
| D-DBPOLICY1 | row policy | card #1160 done |
| D-LINTPOLICY1 | warnings non-blocking by default | pkg lint policy tests; dossier bypass facts |

## Non-goals / honesty

- Full Convex-class multi-tenant authorization matrices beyond the shipped
  footprint/session APIs remain product follow-ups, not silent stubs.
- Browser `core.ws` wire protocol rides existing HTTP/WS surfaces; live registry
  counts `ws_pushes` as the Signal-side receipt for invalidation.

Source proposal: `docs/proposals/framework-lessons.md`.
