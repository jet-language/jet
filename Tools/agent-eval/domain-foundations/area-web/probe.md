# Web foundations probe

## What I built

I built an Orders package with SQLite migration and policy scope, HTTP list/detail/create routes, a login and protected route, static assets, and one local HTTP client flow. A nested web package builds a reactive browser island from the same scratch tree. The server uses a manual URL-form decoder and a `Cell<Int>` invalidation counter; the list query is rerun after create.

Files:

- `pkg/package.jet` — server authority.
- `pkg/run.jet` — server, routes, persistence, auth, and end-to-end flow.
- `pkg/public/index.html` — static host asset.
- `pkg/web/package.jet` — browser authority.
- `pkg/web/client.jet` — `#Target(Web)` reactive island.
- `pkg/web/index.html` — browser companion page.
- `query_probe.jet` — negative query/form API probe.

Research note: `~/.cache/jet-luna/dx2/web-frameworks/` was absent. I read the CMS/ecommerce, serverless-edge, browser-extension, and documentation-static-sites reports.

## What worked

- Package checking and route/module/tier proof: `jet check .../pkg` passed with entry resolution, module graph, Core closure, and AOT/JIT/interpreter proof.
- Typed path routes: `jet run .../pkg` returned `detail:200` for `/orders/1`; `:id` is available through `req.param("id")`.
- Query-bearing request target: the same run rendered `request-path=/orders?limit=3`; the raw target is available even though no typed query accessor exists.
- Server-rendered HTML: the root route returned `home:200`.
- SQLite persistence and refetch: a clean run returned `list-before:200 ... orders=1`, `create:201`, then `list-after:200 ... orders=2`.
- Form validation path: the valid URL-encoded client request created an order; missing/empty/nonpositive fields return 422 from `parse_order_form`.
- Session auth: the run returned `login:200:<jet_session=...>` and `protected:200`; `session_validate` accepted the protected session.
- Static assets: `asset:200` returned the complete HTML host page.
- Browser target: `jet build client.jet --target web` succeeded and wrote `build/app.wasm`, `build/app.js`, maps, manifest, runtime, and `index.html` in `pkg/web/build`.
- Dev loop: `jet dev . --watch=off` ran the same HTTP flow successfully. `jet dev . --watch --swap` printed `building`, `[restart] ...`, and `ready` after a source edit; SQLite state survived because it is durable. In-memory state retention across that restart was not proven.

## Gaps

### area-web-G1 — `defect` — blocks

Native code generation cannot currently compile this valid DB+HTTP+task server into a deployable executable. Exact proof: `jet build run.jet --verbose` checked the source, generated 5,719,119 bytes of Rust, then printed `internal compiler error: the generated Rust did not compile. This is a bug in jet` and named `pkg/build/run.rs`. This is shared by web, backend, and data workloads. `jet run` and `jet dev --watch=off` are usable workarounds, but they do not produce a native deployment artifact.

### area-web-G2 — `impossible` — hurts

`HTTPRequest` has no typed query-parameter accessor. Exact proof: `jet check .../query_probe.jet` reported `Error [E0102]: HTTPRequest has no method query` at `req.query("limit")`. This affects web routes; a package author must read `req.path()` and implement URL query parsing.

### area-web-G3 — `call-site`, `defect` — hurts

The server has no typed form decoder and field-validation result. Exact proof: the same negative probe's `req.form("item")` reported `Error [E0405]: ?? only works on a fallible value, not HTTPRequest`; the app had to read `req.text()`, split pairs, and validate every field in `run.jet`. The diagnostic also describes the failed expression inaccurately. This is shared by web and backend packages. A library can provide the missing decoder today, but every package otherwise repeats this parser and its error policy.

## Friction

- `response.header(name, value)` is a response builder that returns a new response; discarding that value made the handler return 500. The working form is `with_cookie :: response.header(...)`, an easy-to-miss call-site rule.
- The query result can be counted, but rendering each `[Row]` by looping caused a 500 in the default HTTP evaluator. The working route renders `rows.len()` and detail uses `query_one`; this is recorded as a runtime defect risk rather than hidden.
- The basic cache is library code only: the app manually increments a `Cell` after mutation and reruns the SQL query. No automatic query cache/refetch lifecycle was available.
- `jet-env` emitted a scratch-cwd warning about a missing relative `clean-nix-tmp.sh`; it did not prevent the checks or runs.

## Defects

- G1 is a native-build ICE with a valid checked source and generated Rust artifact.
- G3's missing `req.form` attempt reports E0405 about fallibility on `HTTPRequest`, not a missing method.
- Iterating a multi-row database result in the HTTP handler reached neither the loop body nor a handler response and produced 500; removing the loop and using `rows.len()` restored 200. The exact source pattern was `loop row in rows` after `scope.query(...)`.

## Battery notes

- Package/server/static skeleton — pure library and example code.
- Typed route/query/form helpers — pure library code, with query capability needing the core primitive above.
- SSR plus reactive island and serializable boundary — pure library code on top of the web target.
- SQLite migration, policy, query, mutation, and invalidation fixture — pure library code.
- Session login, cookie, protected route, and failure cases — pure library code.
- Deterministic local listener/client end-to-end harness — first-party tooling battery, not library-only.
- Web/native build smoke and manifest diagnostics — compiler/tooling battery, not library-only.
- Dev watcher edit with an observable state-retention check — compiler/tooling battery, not library-only.

## Verdict

Buildable today for a development server and browser artifact: routing, HTML, DB, auth, assets, and a valid end-to-end flow all ran.
Buildable with listed gaps fixed for production: the native server build currently ends in an ICE.
A router/forms/query suite can be written as packages, but query and form decoding are repeated at each application boundary.
The dev watcher rebuilds and restarts; durable DB state survives, while in-memory state retention remains unproven.
The browser daemon was unavailable, so no visual browser interaction was run; web artifact generation itself passed.
