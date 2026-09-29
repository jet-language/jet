# #3076 — Request authority, deadline and transaction continuity

Closer09, 2026-09-29. Binary via `~/.cache/jet-luna/safe-jet.sh` (jet-current
= `jet-debug-snapshot16`). Source head `e9c708fa7`.

## Question

Can one executed example show that two alternating principals keep separate
principals and transaction outcomes over one DB connection/pool, that
commit, rollback and a lost reply after commit are distinguished, that a
request whose deadline expired before dispatch writes nothing, and that the
pool and application lifetimes stay distinct, using explicit existing values
and no DI scope?

## Method

Drafted `~/.cache/jet-test-scratch/Closer09/net/request_continuity.jet`
(package `net/package.jet` = the repo's `Examples/features/net/package.jet`
plus `DB`). Design: one `DBConnection`/`DBScope` and one `DbPool` (admission,
`acquire(deadline: 100ms)`) created before `http_server.bind`; one `POST
/order` handler that reads `x-user`, `x-qty`, `x-budget-ms`, `x-slow-reply`
from its own request; expired budget → 504 before taking a lease or opening a
transaction; `qty <= 0` → insert then `rollback()` → 422; otherwise
`commit()` → 200, and with `x-slow-reply` the handler sleeps 300 ms after
commit while the client's `read_timeout(100)` gives up; the client then
counts rows and reports `committed-reply-lost` only when the row exists.

## Evidence

The draft cannot compile on the current binary because the Core `db` and
`http` facades embedded in the snapshot fail their own checks. The shipped
examples reproduce it without my code:

- `Examples/features/io/db_pool.jet` (copied to `net/db_pool_copy.jet`),
  `safe-jet.sh run` → exit 1:
  `E0358 DbPool is spelled DBPool` at `Core/db/db.jet:28:43`,
  `E2417 Explicit failure domain String (text) is not an Error type` at
  `Core/db/db.jet:32:57`, `E2404 … has_policy_control …` at `:35:71`, …
- `Examples/features/net/http_server_middleware.jet` (copied to
  `net/mw_copy.jet`), `safe-jet.sh run` → exit 1: `E0108 This Err holds
  HTTPError, but <corelib>/Core/http::…::HTTPError was expected` (13:37,
  26:37), `E0113 outer promises to return HTTPHandler …` (8:12, 20:12),
  `E2404 … mux() …` (32:24).
- My draft additionally hit `E0104 serve expects 4 arguments, got 2` at
  `Core/http/http.jet:457:22` and, when the pool type is spelled out,
  `E0358 DbPool is spelled DBPool` followed by `E0119 There's no type called
  DBPool` (the snapshot's type is still `DbPool`).
- `git status Core/db/db.jet` shows an uncommitted working-tree rewrite by
  another worker (`DbPool`→`DBPool`, `policy` failure domain `String`→
  `DBError`), i.e. the Core/db surface is mid-cutover and the snapshot carries
  the old text under the new rules.

## Verdict

BLOCKED on the current binary. No criterion can be exercised: any program that
imports `core.db` or builds a `core.http.server` middleware/mux on
snapshot16 fails in the Core facade before user code runs. The draft example
and its design stay in scratch; nothing was added to `Examples/`.

## Follow-up

Once a snapshot with the finished `core.db` / `core.http` cutover lands, run
the draft on default run, `--interpret` and AOT and bless
`Examples/features/net/request_continuity.jet` only if all tiers agree. The
net package budget needs `DB` added (`Examples/features/net/package.jet`).
Design note for criterion 4: `DbLease` exposes only `close`, so the pool is an
admission bound, not a connection source; the connection/scope and the pool
are both created once before bind, and no per-request object graph is built.
