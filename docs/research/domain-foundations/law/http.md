# http

## Ratified

- **D-URL1=A** — `core.url` and `core.mime` are separate typed modules; HTTP and web “consume typed values instead of re-solving string escaping.” — `docs/spec/syntax-decisions.md:3585-3589`
- **D-HTTPDEPTH1=A** — `core.http` owns “Client, Server, Router, middleware, streaming bodies, forms/multipart, cookies, redirects, timeouts, TLS policy, and SSE”; WebSocket has the separate `core.ws` home. — `docs/spec/syntax-decisions.md:3846-3849`
- **D-HTTP-ROUTE-SYNTAX2=A** — `:name` captures one decoded path segment and final `*name` captures remaining decoded segments; duplicate names, traversal, malformed percent/UTF-8, and compatibility aliases are rejected. — `docs/spec/syntax-decisions.md:3850-3855`
- **D-HTTP-JSON1=A** — typed `resp.json<T>(limit)`, `req.json<T>()`, and `server.json(status, value)` all ride `#Codable`; raw text/bytes remain expert paths. — `tower` (`D-HTTP-JSON1` outcome A)
- **D-HTTP-MSG1=A** — received messages get `resp.text()`/`req.text()` over the shared one-MiB cap, request builders get `json(value)`, and fixed routes accept zero-argument handlers. — `tower` (`D-HTTP-MSG1` outcome A)
- **D-HTTP-STATIC-FILES1=A** — static mounts normalize paths, refuse escapes, hide dotfiles, refuse symlink escape, and serve `index.html` by default; risky behavior requires explicit options. — `tower` (`D-HTTP-STATIC-FILES1` outcome A)
- **D-HTTP-CORS1=A** — CORS is an explicit policy, denies by default, and rejects `.Any` with credentials at policy construction. — `tower` (`D-HTTP-CORS1` outcome A)
- **D-WEBAPP-SERVE1=D** — `jet dev` auto-serves `web.app` with live reload, `jet run` serves plainly, and `fn dev`/`app.serve` are expert controls. — `tower` (`D-WEBAPP-SERVE1` outcome D)
- **D-FAIL-ERRWIRE1=D** — the `jet.err/v1` wire record mirrors `JetErr`; JS gets `JetError`, Wasm receives the same record, and report shape is renderer policy. — `tower` (`D-FAIL-ERRWIRE1` outcome D)
- **D-DBDRIVER1=A** — `core.db` uses one generic parameterized-only `Driver` trait; SQLite is first and async remains deferred. — `tower` (`D-DBDRIVER1` outcome A)
- **D-LIVEQUERY1=A / D-EFFDBREAD1=A** — `app.live` accepts only `DB.Read` functions; the closed Core table gives `conn.query/query_one` `DB.Read`, `execute` `DB.Write`, and transaction/close the plain `DB` root. — `docs/spec/syntax-decisions.md:4201-4227`; `tower` (outcomes A)
- **D-DX-SUITE1=C** — Router, Query, Forms, Table, Virtual, and Store ship as one `core.web` suite over shared signals and one devtools contract per module, with named selective imports. — `tower` (`D-DX-SUITE1` outcome C)
- **D-DX-WEBARCH1=D** — route rendering is derived from transitive page/loader/form/component effects; `RequestContext.Read` is never static, and `jet explain --web-graph` shows mode, reason, facts, and cache. — `tower` (`D-DX-WEBARCH1` outcome D)
- **D-DX-ENTRY1=C** — package entry inference is shallow and override-safe: explicit override, `@cmd home`, direct root `fn`, direct `src`, then stock fallback. — `tower` (`D-DX-ENTRY1` outcome C)
- **D-DX-LIVE1=A** — the dev loop preserves matching state across saves; module reload keeps type stability, and in-flight requests use old/new module versions safely. — `tower` (`D-DX-LIVE1` outcome A)
- **D-DX-PROD1=B** — release strips devtools by default while retaining a local same-user value rail; only the build-in network endpoint is available. — `tower` (`D-DX-PROD1` outcome B)
- **D-DX-PLUGIN1=D / D-DX-DEVTOOLS-UX1=D** — plugins use a marker plus typed `jet.devtools.v1` observation panel; the shared UX moves from pill to lens to workbench in one panel/stream. — `tower` (outcomes D)
- **D-DX-JOBS-UX1=E** — `jet jobs` is the explicit jobs surface; `jet run -- <name>` remains canonical for running a named job. — `tower` (`D-DX-JOBS-UX1` outcome E)

## Shipped

- `core.http` provides one-shot/configurable clients, HTTP/1.1 server and explicit TLS, typed request builders, JSON/forms/cookies/redirects, phase deadlines, router/mux, response/header, SSE, static/range files, CORS, access-log/request-id, and request body/path/param/header/trailer inspection. — `docs/reference/core-library.md:658-739`
- Typed JSON is shipped in both directions: `req.json<T>()`, `server.json(status, value)`, and client response JSON ride `#Codable`; raw body/text/bytes remain available. — `docs/reference/core-library.md:728-731,749`
- Static files, CORS, route params/wildcards, request IDs, graceful shutdown, pooling, HTTP/2, bounded parsing, and WebSocket's separate `core.net.ws` are documented shipped surfaces with the cited safety/transport limits. — `docs/reference/core-library.md:747-771`
- The HTTP reference explicitly marks built-in server `timeout`/`body_limit` middleware open: transport deadlines and the default one-MiB framing cap ship, but they are not Handler wrappers. — `docs/reference/core-library.md:750-754`
- File-backed SQLite, typed JSON routes, session/role auth, durable local queue, structured logs, OpenAPI JSON export, load smoke, graceful shutdown, and native release all ran in the backend probe. — `~/.cache/jet-luna/dx3/area-backend/probe.md:7-18,20-41`
- Orders routing, path params, query-bearing raw target, HTML, SQLite persistence/refetch, manual form validation, session auth, static assets, browser WASM, and the dev watcher ran in the web probe. — `~/.cache/jet-luna/dx3/area-web/probe.md:19-30`
- The backend probe hand-wrote a valid OpenAPI artifact and checked only basic substrings; this proves ordinary package code can export JSON, not that route contracts are derived or validated. — `~/.cache/jet-luna/dx3/area-backend/probe.md:14,49-51`
- `D-DX-WEBARCH1`'s route graph, derived modes, effects, cache fact, and `jet explain --web-graph` are the ratified web architecture direction; the current probe's server/browser flow is not evidence that every graph field is implemented. — `tower` (`D-DX-WEBARCH1` outcome D)

## Undecided

- Should `HTTPRequest` gain a typed query accessor (for example, a checked `req.query<T>()`) that preserves URL decoding, repeated values, defaults, and validation errors?
- Should `HTTPRequest` gain a typed form decoder (for example, `req.form<T>()`) with field validation and one canonical error result?
- Should `core.db.pool` provide a bounded generic pool with acquisition limits, health/reset, readiness, and graceful drain lifecycle?
- Should route facts become typed API contract metadata with compiler consistency checks for method, path, input, output, status, and auth, plus a canonical OpenAPI export?
- Should `core.http.server` provide request-scoped timeout/cancellation middleware that cancels in-flight work rather than only checking a clock manually?
- Should `core.process` expose typed termination-signal subscriptions that initiate the existing server graceful-drain path?
- Should response compression and HTTP/2 request compression middleware receive a bounded, explicit public policy and spelling?
- Should the partial access-log path expose final streamed wire bytes through a completion hook without exposing authorization, cookies, body, or query?
- Should the default evaluator support `core.log.set_trace_id` and ordinary multi-row HTTP rendering without E0956/500 defects?

## Conflicts

- **Required e14 D-DX coverage of the listed probe gaps:** `area-web-G2` (typed query accessor) is covered only at the product/design level by **D-DX-SUITE1=C** (`core.web` includes Query) and **D-DX-WEBARCH1=D** (derived route facts); no current `req.query` accessor is shipped, so the HTTP spelling remains a gap. `area-web-G3` (typed form decoder) is covered only at the product/design level by **D-DX-SUITE1=C** (Forms); `req.form` is absent and manual decoding remains. `area-backend-G1` (DB pool) is covered by no D-DX ruling and remains undecided. `area-backend-G2` (route contract metadata plus OpenAPI export) is covered only in route-fact/graph form by **D-DX-WEBARCH1=D** and its `jet explain --web-graph`; no OpenAPI export is ratified by that ruling, so export remains undecided. `area-backend-G3` (request timeout/cancellation middleware) is covered by no D-DX ruling; **D-HTTPDEPTH1=A** owns timeouts generally, but the reference still marks server timeout middleware open, so this gap remains undecided. — `~/.cache/jet-luna/dx3/area-web/probe.md:38-44`; `~/.cache/jet-luna/dx3/area-backend/probe.md:43-55`
- D-HTTPDEPTH1 owns timeout, forms, and middleware concepts, but it does not mean every named helper exists. The reference explicitly says server timeout/body-limit wrappers are open; do not call client phase deadlines a server cancellation contract. — `docs/spec/syntax-decisions.md:3846-3849`; `docs/reference/core-library.md:753`
- D-DX-SUITE1's Query and Forms are a ratified suite direction, not proof that `req.query` or `req.form` currently exists. The negative probe's exact failures are E0102 and E0405. — `~/.cache/jet-luna/dx3/area-web/probe.md:32-44`
- D-DX-WEBARCH1 derives route facts and displays the web graph; it does not ratify `core.openapi`. Handwritten OpenAPI remains package code until a separate contract/export decision lands. — `~/.cache/jet-luna/dx3/area-backend/probe.md:49-51`
- D-HTTP-ROUTE-SYNTAX2 fixes `:name`/final `*name` decoding and rejection rules. A query or contract proposal must not overload path parameters, reintroduce encoded-slash aliases, or silently weaken traversal rejection. — `docs/spec/syntax-decisions.md:3850-3855`
- **D-HTTPLIB1, D-HTTPLIB3, and D-HTTPLIB4 are open spec-only records.** Their source imports must not be presented as selected Tower outcomes or used to reopen the ratified JSON/static/CORS/message choices. — `tower` (IDs open/spec-only)
- D-HTTP-JSON1 and D-HTTP-MSG1 retain raw text/bytes/body as expert floors and name behavior at the call site; content-type-driven implicit decoding or a second body lifecycle would conflict with them. — `tower` (outcomes A)
- D-DBDRIVER1 makes SQLite-first generic parameterized `Driver` the current database contract; a PostgreSQL adapter is ecosystem work, not a new language-level database mechanism. — `tower` (`D-DBDRIVER1` outcome A); `~/.cache/jet-luna/dx3/area-backend/probe.md:69-70`
- The web/backend native build ICE and default-evaluator E0956 are defects shared across areas, not arguments for duplicating HTTP, router, or backend semantics. — `~/.cache/jet-luna/dx3/area-web/probe.md:34-36`; `~/.cache/jet-luna/dx3/area-backend/probe.md:61-63`
- `D-DX-ENTRY1`, `D-DX-LIVE1`, and `D-DX-PROD1` already decide entry precedence, reload state/type stability, and release devtool stripping. A new HTTP ballot must not silently change package entry, hot-reload, or release-observability policy. — `tower` (outcomes C/A/B)
