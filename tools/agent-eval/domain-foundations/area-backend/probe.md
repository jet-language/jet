# Backend services probe

## What I built

I built one file-backed backend package in `pkg/`: typed JSON HTTP routes, checked SQLite migration/query, session authentication with a role gate, a durable local service queue, structured logs/counters, an exported OpenAPI document, a contract smoke check, a 1,000-task load run with a p99 assertion, and graceful shutdown. The package uses only current Jet core modules and writes its database, state log, and OpenAPI artifact inside this scratch directory. Supporting probes are `pool_probe.jet`, `openapi_probe.jet`, `timeout_probe.jet`, `signal_probe.jet`, `metrics_probe.jet`, `postgres_probe.jet`, and `shared_counter_probe.jet`.

## What worked

- Package graph and tier proof: `scripts/agent/jet-env jet check .../pkg` -> `check: passed ... diagnostics=0`; entry, module graph, Core closure, and AOT/JIT lowering proofs were all reported.
- File-backed SQLite: `db.open(path)`, checked `SQL{...}`, migration, policy-bound scope, insert, query, typed row access, and close all ran; output `db-item:bootstrap`.
- Typed HTTP JSON: `server.mux`, typed `req.json<OrderRequest>()`, `server.json`, and typed response decode ran in one native server. Output: `health:200`, `order:202`, `queued:true`.
- Session authentication and role policy: a valid session with `x-role: writer` received 202; the same session with `x-role: reader` received `denied:401`.
- Durable local queue: `service.delivery_durable`, a state event log, bounded worker, `tree.send_durable`, typed delivery state, and receive ran. Output: `delivery:delivering`, `worker:order:3`.
- OpenAPI artifact: `files.write` exported valid JSON at `pkg/openapi.json`; `jq -e . .../openapi.json` parsed it. The in-program basic contract check printed `contract:openapi-basic-pass`.
- Concurrent load smoke: 1,000 Jet tasks issued health requests and collected 1,000 timing samples; sorted p99 assertion passed. Output: `load:vus=1000 p99-ms=3`.
- Graceful server shutdown: `server.shutdown(100ms)` and `serving.join()` completed. Output: `shutdown-cancelled:0`.
- Structured observability: release output included `trace_id:"backend-probe"` and `metric.counter.requests` / `metric.counter.errors` JSON fields.
- Native release: `scripts/agent/jet-env jet build --release .../pkg` -> `jet Built build/run in 32.7s`.

Final release run command:

```text
scripts/agent/jet-env jet run --release /home/nate/.cache/jet-luna/dx3/area-backend/pkg
```

Final release output:

```text
db-item:bootstrap
openapi:true
contract:openapi-basic-pass
health:200
denied:401
order:202
queued:true
delivery:delivering
load:vus=1000 p99-ms=3
worker:order:3
shutdown-cancelled:0
{"level":"info","body":"backend.summary","trace_id":"backend-probe","ts":1788389211417,"metric.counter.requests":960,"metric.counter.errors":1}
```

## Gaps

### area-backend-G1 — bounded database pool

**Tags:** `impossible`, `boilerplate`; **severity:** blocks. A production service lacks a checked bounded pool with acquisition limits, health/reset, and drain lifecycle. `jet check .../pool_probe.jet` reports `Error [E1004]: core.db has no item pool` and lists only `open`, `open_memory`, policy, row, transaction, and migration operations. The package therefore uses one `db.open(path)` connection at `pkg/run.jet:140`. This is shared by data, web, and CLI services. A library author can write an application pool around separately opened SQLite handles, but cannot obtain a standard reset/readiness contract.

### area-backend-G2 — typed API contract metadata and OpenAPI validation

**Tags:** `impossible`, `boilerplate`; **severity:** hurts. `jet check .../openapi_probe.jet` reports `Error [E1001]: There is no core module core.openapi`. The package hand-writes the document in `pkg/run.jet:72-83` and checks only three substrings in `pkg/run.jet:84-88`; every route must be duplicated in the document, with no method/input/status/auth consistency check. The existing HTTP graph does not expose the standard contract fields. This is shared by web, CLI, and AI-facing APIs. A package can hand-maintain JSON, as this probe does.

### area-backend-G3 — request-scoped server deadline and cancellation

**Tags:** `impossible`; **severity:** hurts. `jet check .../timeout_probe.jet` reports `Error [E1004]: core.http.server has no item timeout`; the available list contains routing and serving helpers but no timeout wrapper. The reference marks server timeout middleware open at `docs/reference/core-library.md:753`. Client phase timeouts exist, but a server cannot attach a route deadline that cancels in-flight work. This is shared by web, games, GUI, and embedded services. The workaround is manual clock checks plus transport/client limits.

### area-backend-G4 — typed process-signal to shutdown propagation

**Tags:** `impossible`; **severity:** hurts. `jet check .../signal_probe.jet` reports `Error [E1004]: core.process has no item signal` and lists only `exit`, `run`, `cmd`, `pipeline`, `argv`, and `args`. `server.shutdown` itself works, but a deployed service has no typed SIGTERM subscription to initiate that drain. This is shared by CLI, GUI, and embedded long-running programs. The workaround is an external supervisor endpoint or an explicit unsafe/native bridge.

### area-backend-G5 — default evaluator defect for trace context

**Tags:** `defect`; **severity:** hurts. `scripts/agent/jet-env jet run .../pkg` reports `Error [E0956]: core.log.set_trace_id() isn't supported by the current evaluator yet` at `pkg/run.jet:136`. The same source succeeds with `jet run --release` and `jet build --release`; this blocks the ordinary development run for any service using the documented operation.

## Friction

- OpenAPI authoring is 12 lines of escaped JSON plus three substring assertions for two routes. Adding one route requires editing both the mux and the document. This is author boilerplate, not a new library category.
- The load harness is about 28 lines of task-array, sender cloning, joins, channel draining, sorting, and percentile indexing. It is ordinary package code, but a first-party battery should hide it.
- `core.log` supplies counters and trace context, but there is no `core.metrics` registry or Prometheus scrape surface: `jet check .../metrics_probe.jet` reports `Error [E1001]: There is no core module core.metrics`. A text registry is package code, so this is friction rather than a primitive gap.
- PostgreSQL was not exercised: `jet check .../postgres_probe.jet` reports `Error [E1004]: core.db has no item open_postgres`. The existing backend-neutral `Driver` contract leaves a PostgreSQL adapter/bridge as ordinary ecosystem work, not a language primitive. SQLite is the only database proven here.
- The load assertion counted 1,000 completed client timing samples, but the final app counter printed `metric.counter.requests:960`; the harness did not assert HTTP status for every load response, so this is a caveat rather than a performance win claim.

## Defects

- Default `jet run` cannot evaluate documented `core.log.set_trace_id` (`E0956` above). Native release execution and release build are green.

## Battery notes

See `batteries.json`. The battery should include typed HTTP JSON routes, SQLite migration/policy helpers, a durable local queue worker, session/role middleware, structured log/metric sinks, OpenAPI contract metadata/validation, a bounded DB pool, request deadlines plus shutdown signals, and a concurrent load/contract harness. The first five and the load harness are pure library code; the contract, pool, and cancellation/signal parts require missing capability support.

## Verdict

The core is buildable today for a small single-process backend: typed HTTP, file SQLite, local durable queue receipts, sessions, logs, load smoke, and graceful server drain all ran. It is not production-complete because there is no bounded DB pool and no request cancellation or signal-driven shutdown path. OpenAPI export is possible only as duplicated hand-written JSON, and the default evaluator has a trace-context defect. PostgreSQL and OCI image packaging were not run; release-native package build did pass. Fix G1 and G3 first for a production service; G2 and G4/G5 remove major operational friction.
