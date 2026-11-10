# D-ROUTE-METHOD1: public routes for a custom HTTP method (2026-11-10)

New ballot for card #6206, criterion 35: "A route with a Custom HTTP method checks and serves."
Author and reviewer: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5). Code citations are on
dev c460ba91e5. Probes: `$JET_SCRATCH_ROOT/lanes/BallotLead/out/D-ROUTE-METHOD1/probes`, run with
`$JET_SCRATCH_ROOT/lanes/BallotLead/out/probe.sh DIR run`; outputs are in `probes/out43/`.

## 1. The gap

HTTP allows extension methods beyond the nine standard ones (RFC 9110 section 9.1: a method is a
case-sensitive token). Jet's server parses and dispatches any method today; only registration is closed:

- `sema_http_route_verbs` (`Compiler/JetSema/Source/Sema/Calls/HttpRouting.jet:877-888`) derives the verbs from
  core.http's `Method` enum (`Core/Http/Http.jet:60-71`) and skips `Custom`.
- Each verb call lowers to the private `__route` with the method text (`HttpRouting.jet:1137-1150`), the mux's
  `__route` (`Core/Http/Server.jet:197-199`) or the router's (`Core/Http/Http.jet:595-601`).
- Dispatch compares the stored method string with the request's (`Server.jet:694-697`) and answers 405 with an
  Allow list built from the matched routes (`Server.jet:699-703`).
- Route markers: seven structs, `Get` to `Options` (`Server.jet:27-75`); the marker verb is the struct's name
  (`Compiler/JetSema/Source/Sema/Calls/HttpMount.jet:34-45`). There is no `Trace`, `Connect` or `Custom` marker.
- The OpenAPI renderer knows eight methods (`Core/Web/Web.jet:829`) and stamps `3.1.0` (`Web.jet:1280`); an
  unknown method is a contract stop (`Web.jet:1104`).
- The client already takes a method string: `client.request(method: String, url)` (`Core/Http/Client.jet:68`).

## 2. Probes (a0.0.43)

| Probe | What it does | Result |
|---|---|---|
| r01_verbs | mux with GET and DELETE on `/cache/:key`; a client sends GET, then PURGE | `get:200:get:a`, `purge:405:allow=DELETE, GET, HEAD, OPTIONS` |
| r02_private_route | `mux.__route("PURGE", ...)` from user code | E0067 `__route` is reserved for Jet |
| r03_verb_method | `mux.purge(...)` | E0102 `HTTPMux` has no method `purge` |
| r04_marker | `#Server.Custom("PURGE", "/cache/:key")` on a mounted struct | E0927 not a known applied rule |
| r05_openapi_connect | `mux.connect(...)`, then `web.openapi(mux)` | exit 101: "internal error: runtime stop `E2805` has no active runtime row ... please report this as a Jet defect" |
| r05b_connect_only | r05 without the document | `registered` |
| r05c_openapi_get | GET and DELETE, then `web.openapi(mux)` | `true true true` (both methods, version 3.1.0) |
| r06_method_value | `http.Method.custom("PURGE")` and three bad tokens | backend crash: `ir.build unlowered ... runtime route JetHTTPMethod::custom` (4 faults) |
| r07_const_path | `router.get(CACHE_PATH, ...)` with `CACHE_PATH: "/cache/:key"` | E2805 "route path must be a static String"; its Fix says "Use a literal or compile-time String constant" |

Reading: the wire path already serves a custom method (r01); only the public registration is missing
(r02-r04). Three defects turned up on the way (section 6).

## 3. Repository census (`census.sh`, dev worktree)

| Count | Value |
|---|---:|
| Verb calls on mux or router values (`get` 111, `post` 26, `put` 4, `head` 2, `patch`, `delete`, `options`, `trace`, `connect` 1 each) | 148 |
| `#Server.<Verb>(` marker lines | 34 |
| Marker structs in Server.jet | 7 of 9 standard methods |
| Files outside Core calling `.__route(` | 1 (a proposal sample under Docs/proposals) |
| Client requests with an extension method | 0 |

No existing caller changes under any option: every option only adds surface.

## 4. Peer languages (sources read 2026-11-10)

| System | How a custom method is routed | Source |
|---|---|---|
| Go 1.22+ ServeMux | The pattern is `[METHOD ][HOST]/[PATH]`; `validMethod` accepts any RFC token, so `"PURGE /cache/{key}"` works. GET also matches HEAD. | https://github.com/golang/go/blob/master/src/net/http/pattern.go (lines 70-105); request.go:858-873 |
| ASP.NET Core | `MapMethods(pattern, IEnumerable<string> httpMethods, handler)` takes any method strings. | https://learn.microsoft.com/en-us/dotnet/api/microsoft.aspnetcore.builder.endpointroutebuilderextensions.mapmethods |
| Express | `app[method]` for each name in Node's `http.METHODS` (lib/utils.js:15, 29; lib/application.js:471), plus `app.all` (application.js:494). A method outside Node's list cannot be routed by name. | https://github.com/expressjs/express/blob/master/lib/application.js |
| Axum | `MethodFilter` has ten fixed bits, CONNECT to QUERY (method_filter.rs:29-47); `TryFrom<Method>` refuses every other method (lines 90-107). `on(filter, h)` cannot name PURGE; `any(h)` serves it but skips the Allow header (method_routing.rs:510-517). | https://github.com/tokio-rs/axum/blob/main/axum/src/routing/method_filter.rs |
| OpenAPI 3.2.0 | Path Item keeps fixed fields get to trace plus query, and adds `additionalOperations`, a map keyed by the method "with the same capitalization that is to be sent". | https://spec.openapis.org/oas/v3.2.0.html#path-item-object |

Fair best cases. Go's pattern form is compact and already validated, but it is a second grammar inside a string.
ASP.NET's list form is the most general: one route can answer several methods. Express shows that a fixed
per-name list leaves gaps. Axum shows the cost of a catch-all: the server can no longer list Allow.

## 5. The same program in each option

A cache with `GET /cache/:key` and `PURGE /cache/:key`:

```
// A: a verb named for the Custom case.
mux.get("/cache/:key", (key: String) -> Ok(server.response(200, "hit {key}")))
mux.custom("PURGE", "/cache/:key", (key: String) -> Ok(server.response(200, "purged {key}")))
Routes: struct {
    #Server.Custom("PURGE", "/cache/:key")
    purge: fn(self, key: String) -> HTTPResponse HTTPError! { Ok(server.response(200, "purged {key}")) }
}

// B: route takes a Method value.
mux.route(.Custom("PURGE"), "/cache/:key", (key: String) -> Ok(server.response(200, "purged {key}")))
//   #Server.Route(.Custom("PURGE"), "/cache/:key")

// C: method inside the pattern.
mux.handle("PURGE /cache/:key", (key: String) -> Ok(server.response(200, "purged {key}")))

// D: catch-all.
mux.any("/cache/:key", (request: HTTPRequest) -> {
    if request.method != "PURGE" -> return Ok(server.response(405, "no"))
    Ok(server.response(200, "purged"))
})
```

## 6. Defects found (for cards)

1. `web.openapi(mux)` on a mux with a public `mux.connect` route exits 101 with "runtime stop `E2805` has no
   active runtime row ... report this as a Jet defect", hiding the real message, "unsupported OpenAPI HTTP method"
   (`Web.jet:1104`; the host form is `Compiler/JetRuntime/Source/RouteWeb.jet:3-15`). Probes r05, r05b.
2. `http.Method.custom(token)`, a public constructor the compiler knows (`Compiler/JetSema/Source/Sema/Calls/Http.jet:166, 177, 191, 321`),
   crashes the backend: `ir.build unlowered ... JetHTTPMethod::custom` (r06). The one case that uses it,
   `tests/cases/corelib/core_http_nominal_message_and_body_surface_is_executable.case`, is in known-failing under
   #3665 for a different error, so the crash is never reached.
3. E2805's Fix tells a router author to "use a literal or compile-time String constant", but a module constant
   path is refused (r07; message at `HttpRouting.jet:988`).
4. Route markers cover 7 of the 9 standard methods; there is no `#Server.Trace` or `#Server.Connect`
   (`Server.jet:27-75`), though criterion 6 of #6206 says each of the nine checks as a call.

## 7. Adversarial review, claim by claim

| # | Claim | Attack | Evidence | Verdict |
|---|---|---|---|---|
| 1 | The server already serves custom methods; only registration is missing. | Maybe the parser refuses PURGE. | r01: PURGE reaches dispatch and gets 405 with Allow. | Pass. |
| 2 | Public code cannot register one today. | Try every spelling. | r02 E0067, r03 E0102, r04 E0927. | Pass. |
| 3 | A keeps one verb list. | Custom needs a different arity. | Verbs are derived from `Method` (`HttpRouting.jet:877-888`); custom is one row with a leading token argument. | Pass, with the arity difference stated. |
| 4 | A's token is checked while building. | A run-time token on a mux is not. | Mux paths already allow run-time values (`Server.jet:203-208`). | Failed as first written. Fixed: run-time tokens on a mux are checked at registration with E2805 and the calling line; routers stay literal-only. |
| 5 | A named constant gives the token one spelling. | Constants may not count as static. | r07: a constant path is refused despite E2805's fix text. | Failed as first written. Fixed: A's build criterion accepts constants for tokens and paths both. |
| 6 | A custom route appears in the OpenAPI document. | 3.1 has no field for it. | `Web.jet:829`; OpenAPI 3.2 `additionalOperations`. | Failed as first written. Fixed: render under 3.2 `additionalOperations`, stamp 3.2.0 only when needed. |
| 7 | Allow lists include the custom route. | | Allow is built from matched route method strings (`Server.jet:695, 702`). | Pass. |
| 8 | `custom("GET")` cannot create a second spelling. | Case games: "Get", "get". | RFC 9110 makes methods case-sensitive, so "Get" is a distinct method. | Pass by rule: any case-insensitive match of a standard name is refused, naming `mux.get`. |
| 9 | B's typed value is safer. | Does `Method.custom` work? | r06 backend crash. | B's claim fails today; B also gives GET a second spelling. |
| 10 | C matches Go, so it is familiar. | Jet patterns use `:id`, Go uses `{id}`. | Go pattern.go:70-82. | Partly: the shape is familiar, the grammar is not shared. |
| 11 | D is the smallest change. | What breaks? | Axum's `any` skips Allow (method_routing.rs:510-517). | Pass, with its cost: no 405, Allow or document entry. |
| 12 | No existing caller changes. | | census: every option only adds. | Pass. |

## 8. Each loss and the two attempts to remove it

- A, the method is a string: (1) build-time token grammar and no standard name in any case; (2) named constants,
  fixing E2805's refusal (r07). Remaining: a valid misspelling such as PRUGE still builds.
- A, OpenAPI 3.1 has no slot: (1) render under 3.2 `additionalOperations`; (2) stamp 3.2.0 only when a document
  holds such a route, and move CONNECT there, fixing defect 1. Remaining: a 3.1-only tool may reject such a document.
- A, one more call and marker: (1) derive from the `Custom` case; (2) lower to the same `__route`, so dispatch,
  Allow and the E2804 duplicate check are unchanged. Remaining: one marker struct and one verb row.
- A, run-time mux tokens: (1) check at registration with E2805 and the calling line; (2) keep routers literal-only.
  Remaining: the same rule mux paths follow today.
- B, a second spelling for GET: (1) refuse standard cases, which is A with a heavier spelling; (2) lint toward
  `mux.get`, which still allows two. Not removed without becoming A.
- C, a second grammar: (1) refuse standard names in the method form; (2) share the path parser after the first
  space. Remaining: two ways to read a route string.
- D, no 405 or Allow: (1) let the handler declare its methods, which recreates A's token; (2) answer OPTIONS from
  the handler, which moves routing into user code. Not removed.

## 9. Verdict

A passes after three fixes (claims 4, 5 and 6). B fails on a crash and a doubled spelling, C on a second grammar,
D on lost 405 and Allow answers. Building A also closes defect 1. Defects 2-4 need cards whatever the vote.

## Different-family review (GPT, 2026-11-10)

This section supersedes conflicting conclusions above. A is a proposed design, not an implemented success.

| Claim | Attack and independently checked evidence | Verdict |
|---|---|---|
| Custom methods already work | Reran all nine r01-r07 programs on a0.0.43. r01 receives PURGE and returns 405; it does not serve a registered custom handler. r02-r04 refuse the attempted APIs. | Holds within stated limits: receive and reject, not register and serve. |
| Case-insensitive standard-name rejection is safe | RFC 9110 section 9.1 says method names are case-sensitive. New `review6-case.jet` runs and prints `lowercase:405` beside a GET route. Rejecting `get` would exclude a distinct legal method. | Fails. Refuse only exact standard names and preserve exact case through all route metadata. |
| Existing lowering can be reused unchanged | Read origin/dev HttpRouting and Server. The standard-verb contract builder uppercases its input. That must not touch extension tokens. | Fails. Share registration, but preserve case in contract, duplicate key, dispatch, Allow and operation ID. |
| B is worse because Method.custom crashes | Reran r06: exit 101, unlowered Method.custom. A is also unbuilt; a repairable implementation defect cannot rank proposed APIs. | Fails. B now describes one typed registration API with full per-verb cutover, not two APIs. |
| C necessarily duplicates standard routes | A method-and-path API can replace per-verb registration. Go pattern.go validates that shape; the path subgrammar remains Jet's. | Fails. C now specifies a clean cutover. |
| OpenAPI covers arbitrary methods | Read OpenAPI 3.2 Path Item: additionalOperations preserves capitalization and excludes methods with fixed fields. Reran r05/r05b/r05c: CONNECT document exits 101, registration succeeds, ordinary document succeeds. | Holds only as a required implementation change, with downstream 3.1-tool cost. |
| Catch-all preserves complete route metadata | Axum method_routing.rs:510-517 calls skip_allow_header. A user handler can manually build 405/Allow, but the router cannot infer its accepted methods. | Holds within stated limits; D needs metadata or manual duplication. |
| Module constants work for static paths | Reran r07: E2805 despite its constant advice. | Fails today; fix is shared, not A-exclusive. |

Peer sources read independently: Go pattern.go, ASP.NET MapMethods, Axum method_filter.rs and method_routing.rs, Express application.js, RFC 9110 and OpenAPI 3.2. Earlier census counts were not repeated and are not used to rank options. The claim that missing Trace/Connect markers violates a criterion about call forms is withdrawn; that criterion alone proves no marker defect.

Recommendation: agree with A only after these repairs. B is a strong coherent alternative, not ruled out by I8 or today's crash. Do not post the earlier case-folding rule, the assertion that r01 serves a custom route, or the crash-based ranking.
