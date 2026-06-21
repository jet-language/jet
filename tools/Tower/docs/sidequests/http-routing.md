# Plan: HTTP routing layer / middleware (D-ROUTE1)

**Status: plan — awaiting owner decision D-ROUTE1.**

Unblocks: **Tariq** (REST microservice), **Amara** (automation webhooks).

---

## Goal

Today `jet.http` exposes a single handler: `57_http_server.jet` runs one closure
that branches on `request.path` by hand. A real service needs to register routes
(`GET /users/:id`), dispatch to per-route handlers, and run cross-cutting
middleware (logging, auth, body parsing). The user-facing goal: declare routes
declaratively, get a typed `request`/`response`, and have path params extracted
for you — no hand-written `if request.path == …` ladder.

Verified today: `57_http_server.jet` outputs `hello from Jet! method=GET path=/`
from a single handler; there is no route table, no param extraction, no
middleware in the stdlib (`grep route|middleware Source/` → nothing).

---

## Pipeline touch points

- **stdlib only** (`jet.http` ring package; no compiler change) for the router
  data structure, path-pattern matcher, and middleware chain. Builds on the
  existing server loop in the `jet.http` package.
- **sema**: only if route handlers get a special typed signature or if path
  params are typed (`:id` → `Int` vs `String`). Pattern-string routes need no
  sema work; a macro/derive form would (gated on S56, deferred).
- **codegen**: none if router is pure stdlib.
- **diagnostics**: a duplicate-route or unreachable-route diagnostic is desirable
  but optional for v1 (could be a runtime panic on registration first, a sema/
  comptime check later).

## Invariants in play

- **I8** simplicity ratchet — routing is a library, not a language feature. Keep
  it in `jet.http`; do not add route syntax to the grammar unless a decision
  explicitly calls for it.
- **I5** ships with `examples/features/` route example + golden output.
- L1101 noise (the spawned-server-task warning) is a *separate* gap — see
  `task-detach.md` (D-DETACH1); routing should not have to solve it.

## Open questions (need owner decision — D-ROUTE1)

1. **Route registration surface** — builder method chain
   (`router.get("/users/:id", handler)`), a route table value, an attribute on a
   handler fn (`#route("GET", "/users/:id")`), or a match-style block. This is the
   user-facing shape and the core decision.
2. **Path-param extraction** — how does a handler read `:id`? From a typed
   `params` map (`request.param("id")`), positional handler args, or a
   destructured pattern? Typed params (`:id<Int>`) vs always-String.
3. **Middleware shape** — a list of `before`/`after` functions, a wrapping
   `fn(next) -> handler` onion, or tagged regions. Interacts with a future
   effect system (D-EFF1) if middleware declares what it touches.
4. **Method + path matching precedence** — static vs param vs wildcard ordering;
   trailing-slash policy; 404/405 defaults.

## Test plan

1. `examples/features/http_routes.jet` — register 2–3 routes incl. one with a
   param, hit them, print responses; golden-tested (I5).
2. Param extraction unit test.
3. Middleware ordering test (before/after run in declared order).
4. (If a duplicate-route diagnostic is in scope) a snapshot for it.
