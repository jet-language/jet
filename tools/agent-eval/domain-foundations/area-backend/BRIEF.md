# Probe area-backend — Backend services and APIs

Read `~/.cache/jet-luna/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: backend.

## Build this

A JSON API service as one Jet package: HTTP routes with typed request/response bodies and an exported OpenAPI document; SQLite or Postgres access through a bounded connection pool; a background queue with at-least-once delivery and a worker; JWT or session auth with roles; structured logs, metrics, and a trace id per request; graceful shutdown; a load test at 1,000 virtual users with p99 asserted; a contract test against the OpenAPI document; and a container-ready release build. Start from examples/features/net/http_*.jet (service, middleware, tasks, lifecycle, limits), examples/features/io/db*.jet and log_*.jet, examples/features/concurrency/**, examples/features/effects/**.

## Answer these

1. Can pools, queues, and OpenAPI export be plain libraries on today's primitives?
2. Does the effect system give a library author what it needs for cancellation, deadlines, and graceful shutdown?
3. What does a backend battery need?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/api-design-contracts/`, `~/.cache/jet-luna/dx2/databases-storage-engines/`, `~/.cache/jet-luna/dx2/messaging-eventing/`, `~/.cache/jet-luna/dx2/identity-auth/`, `~/.cache/jet-luna/dx2/observability-platforms/`, `~/.cache/jet-luna/dx2/distributed-systems/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/area-backend/probe.md`, `gaps.json`, `batteries.json`, and the code under `~/.cache/jet-luna/dx3/area-backend/pkg/`. Gap ids start with `area-backend-G`.
