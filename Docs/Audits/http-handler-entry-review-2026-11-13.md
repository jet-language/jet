# Adversarial review: the web package's door into Core's HTTP server (D-HTTP-HANDLER-ENTRY1=A, rework)

Date: 2026-11-13. Author: Claude Opus, lane G3Core2, who also wrote the
reworked ballot (v3). This file is the claim-by-claim evidence the rework rests
on, with each claim attacked as the review rule in AGENTS.md asks. It does not
replace the fresh-agent review the rule requires: until a reviewer who did not
write A signs the verdict below, no text may call A better than the designs it
is compared with. The v2 review by BallotReview-3 found A needs four public
items and keeps `HTTPHandler` in Core; v3 keeps both findings.

Code basis: dev at 1e3a93d615. The probes ran on the batch-8 compiler (dev
a03eea6ac0) with `NO_COLOR=1`; their sources are in
`$JET_SCRATCH_ROOT/lanes/G3Core2/probe/b6445/`. A is not built, so every
statement about A's behavior is a prediction from the code A's cutover changes,
and is marked as one.

## What A claims

A keeps a server in `core.http.server` that answers every request with one
`HTTPHandler`, and moves the route table and everything about routes into the
web package. Core gains four public items, each replacing a private call:
`server.listen`, `HTTPServer.serve_once`, `server.static_dir` (returns an
`HTTPHandler`) and an opaque `RoutePattern` type (`route_pattern(text)`,
`match(request)`, `overlaps(other)`).

1. One serve call that takes one handler is the shape the strongest HTTP
   ecosystems ship.
2. One route table holds every route, so the route clash check (E2804) sees
   every clash: literal routes at check time, computed ones when added.
3. Middleware keeps Core's `HTTPHandler` and the compiler's handler capture
   check.
4. The folder escape checks keep one home, and a folder needs no package.
5. A run-time route clash still prints E2804 with its why and fix lines.
6. `listen` is not a general primitive of the kind D-WEB-RUNTIME-API1=A
   turned down.
7. No code can forge a request's route tag: only `RoutePattern.match` sets it.

## Measured facts

| # | Read or probe | Observed |
|---|---|---|
| R1 | `Core/Web/Web.jet` on dev | 36 calls to 15 private `core.http.server` names: `__handler` 10, `__route_add` 7, `__layer_add` 4, `__static_files`, `__mux_routes` and `__bind` 2 each, and `__server_on`, `__serve_once`, `__route_parse`, `__route_openapi_path`, `__route_add_dynamic`, `__route_add_custom`, `__request_id_handler`, `__mux_new`, `__cors_handler` once each. Also `rt.__coded_stop` 3 times and `http.__router_mux` once. |
| R2 | `Core/Http/Server.jet` on dev | `HTTPHandler` at line 154 with `handle`; `HTTPServer` at 278 with `local_addr`, `serve`, `wait` and `shutdown(grace:)`. No public `mux`, `bind` or `serve` function. `__serve_once` at 568, `__static_files` at 607, `__static_serve` at 1832. |
| R3 | `Compiler/JetSema/Source/Sema/Expressions/Lambdas.jet:1474` | A lambda becomes an `HTTPHandler` only when the expected type is `<corelib>/core.http.server::HTTPHandler`. |
| R4 | `Compiler/JetSema/Source/Sema/Calls/Http.jet:400-423` | The handler capture check runs on that conversion (`SemaSendCrossing.HTTPHandler`). |
| P1 | `wrap/main.jet`, `jet run` | A list of stored route functions, wrapped in a lambda where an `HTTPHandler` is expected, answers a parsed `GET /hi` with `200`. The first two attempts were refused: E0120 (`routes` must be owned, `&&[Route]`, to be captured by a stored lambda) and E2404 (the wrapper's implicit `Err!` cannot flow into `HTTPError`; fixed with `-> HTTPHandler Never!`). |
| P2 | `diag/main.jet`, `jet run` | A struct with `impl Diagnostic { code: "E2804" … }` words a clash; `panic` prints `Stop [E3001]: panic: error[E2804]: Two routes both match …`, then `why:` and `fix:` lines, and exits 70. |
| P3 | `clash/main.jet`, `jet check` | Two `app.get("/a/:id", …)` rows on one `web.app()`: E2804 `Two routes both match \`GET /a/:id\``, labelled `first registered here`. |
| P4 | `clash_computed/main.jet`, `jet check` | The same clash, with the second pattern built at run time (`pattern = "{pattern}:id"`): `ok: main.jet has no problems`. |
| R5 | `Compiler/JetSema/Source/Sema/Calls/HttpRouting.jet:417-440` | Route registrations refuse an interpolated route literal (E2805) and check a constant pattern; a non-constant argument gets no pattern and is not checked. |

## Claim 1: one serve call is the strongest ecosystems' shape

**Holds.**

Strongest alternatives, fairly stated:

- Go `net/http`: `ListenAndServe(addr, handler)` takes one `Handler`, and
  `FileServer` is a `Handler`. Go also ships a public `ServeMux` in the
  standard library, B's shape, and since Go 1.22 `Request.SetPathValue` lets a
  router outside it tag a request (pkg.go.dev/net/http). Go's best case is one
  standard router everyone shares; Jet gets the same by having one router, in
  the web package.
- Deno: `Deno.serve(handler)`; routers such as Hono are libraries; std
  `serveDir(req, opts)` answers a folder (docs.deno.com/api/deno/~/Deno.serve,
  jsr.io/@std/http/doc/file-server).
- Node: `http.createServer(listener)`; Express routes above it
  (nodejs.org/api/http.html).
- Rust: hyper serves one `Service`; axum's `Router` is itself a `Service`
  (docs.rs/axum). Same split as A.
- ASP.NET Core and Spring: routing is built into the framework host, which is
  C's shape. Their best case is one integrated pipeline with routing-aware
  middleware; A gets that inside the web package, which owns both.
- Python: `socketserver`'s `handle_request()` answers one request on a bound
  server (docs.python.org/3/library/socketserver.html), the shape of
  `serve_once`.

Attack: is A copying the lowest common denominator? No: every listed
ecosystem with a separate router puts it above a one-handler server, and the
two that integrate routing (ASP.NET, Spring) still expose a request pipeline.
What Go does that A does not: a router in the standard library. That is B,
which D-ROUTE-ONE1=A rejected because two tables can then hold routes for one
address.

## Claim 2: one table, so E2804 sees every clash

**Holds only within stated limits.**

Measured by P3 and P4: a literal or constant pattern is checked at check time
(R5); a pattern computed at run time is not. Under A, the package checks a
computed route when it is added, with `RoutePattern.overlaps` against the
table's rows, and stops (claim 5). Go has no check-time route check at all;
its `ServeMux` panics at registration. Second attack: a program may build two
`web.app()` values on two ports. Each table is checked on its own; a clash
across them is not a clash, because they serve different ports. [INFERENCE]
The `#RouteTable` check is per table value, as it is on dev.

## Claim 3: middleware keeps `HTTPHandler` and its capture check

**Holds**, measured by P1 with R3 and R4.

Attack: the package author pays the ownership cost. P1 needed `&&[Route]` and
an explicit `Never!` result, and the compiler named both fixes (E0120, E2404).
That cost is the capture check doing its job; moving `HTTPHandler` into the
package instead would need a fifth compiler marker (v2 review).

## Claim 4: file safety keeps one home, and a folder needs no package

**Holds** [INFERENCE: not built]. `static_dir` returns a handler whose body is
`__static_serve` (R2), so dot-dot, dotfile and symlink checks, ETag, 304 and
ranges stay in one file. Go needs `StripPrefix` around `FileServer` to mount
under a prefix; A folds `prefix` into `static_dir`. Edge case: a prefix that
does not match the request path must answer 404, not serve the root. That is a
test the build card must carry.

## Claim 5: a run-time route clash keeps its explanation

**Holds only within stated limits**, measured by P2. The stop text carries
E2804, its why and its fix. The stop itself is E3001, a panic, because
`rt.__coded_stop` is private to Core (R1). Go's `ServeMux` also panics on a
conflicting pattern at registration. The limit applies only to computed
patterns (P4); literal and constant ones keep the full check-time E2804 (P3).
Gap: `jet explain` on a package-worded stop was not checked.

## Claim 6: `listen` is not a general primitive

**Holds.** D-WEB-RUNTIME-API1=A turned down process-wide shared state, a
publish reachable from anywhere and the startup environment. Listening on a
port is `core.http.server`'s own job; `listen` holds no route table (R2: the
server already owns `serve`, `wait` and `shutdown`).

## Claim 7: the route tag cannot be forged

**Holds** [INFERENCE: not built]. On dev the path values and matched template
are private request fields written by `__pick`. Under A the only public writer
is `RoutePattern.match`, which sets them only when the request path matches
the pattern. Code can build its own pattern and match it, but the tag it gets
is still true for that request. Go's `Request.SetPathValue` lets any code set
any value; A is stricter. Cost: the one-pattern parser and matcher stay in
Core, a narrow amendment to D-WEB-HOME1=A, which moves the router.

## Verdict

A holds, within the limits of claims 2, 4 and 5. Gaps for the build card
(#6445): the prefix-mismatch 404 test for `static_dir`; `jet explain` coverage
for the package's run-time stops; a test that `RoutePattern.match` refuses a
path that does not match. Fresh agent: Main read ballot v3 and asked for the
route tag and the literal-route split, which r2 adds.
