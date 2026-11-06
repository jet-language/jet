# Probe area-backend — Backend services and APIs

Read `$JET_STATE_ROOT/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: backend.

## Build this

A JSON API service as one Jet package: HTTP routes with typed request/response bodies and an exported OpenAPI document; SQLite or Postgres access through a bounded connection pool; a background queue with at-least-once delivery and a worker; JWT or session auth with roles; structured logs, metrics, and a trace id per request; graceful shutdown; a load test at 1,000 virtual users with p99 asserted; a contract test against the OpenAPI document; and a container-ready release build. Start from Examples/features/net/http_*.jet (service, middleware, tasks, lifecycle, limits), Examples/features/io/db*.jet and log_*.jet, Examples/features/concurrency/**, Examples/features/effects/**.

## Answer these

1. Can pools, queues, and OpenAPI export be plain libraries on today's primitives?
2. Does the effect system give a library author what it needs for cancellation, deadlines, and graceful shutdown?
3. What does a backend battery need?

## Research to mine

Domain census and reports: `$JET_STATE_ROOT/dx2/api-design-contracts/`, `$JET_STATE_ROOT/dx2/databases-storage-engines/`, `$JET_STATE_ROOT/dx2/messaging-eventing/`, `$JET_STATE_ROOT/dx2/identity-auth/`, `$JET_STATE_ROOT/dx2/observability-platforms/`, `$JET_STATE_ROOT/dx2/distributed-systems/` (report.md, census.json, claims.json). Deleted ballots with worked code: `$JET_STATE_ROOT/dx2/ballots/` (grep the mechanism name). Family syntheses: `$JET_STATE_ROOT/dx2/_families/*/synthesis.md`.

## Output

`$JET_STATE_ROOT/dx3/area-backend/probe.md`, `gaps.json`, `batteries.json`, and the code under `$JET_STATE_ROOT/dx3/area-backend/pkg/`. Gap ids start with `area-backend-G`.
