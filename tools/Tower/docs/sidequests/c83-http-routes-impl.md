# Implementation plan: c83 — HTTP route registration + `:param` dispatch

**Status: ratified, ready to build.** D-ROUTE1 = A (ratified 2026-06-22).
Implementation phase of the pre-ratification `http-routing.md` stub.

## 1. Ratified decision + spec ref

- **D-ROUTE1 = A** — `syntax-decisions.md:2087`: HTTP route registration & dispatch
  for `jet.http`. Register routes with **path patterns + `:param` extraction parsed
  for the handler**, replacing the manual `request.path` if/match ladder. Option A is
  the **builder/registration** surface (vs the rejected match-block / attribute forms).
  Implements c83.
- Concretely (per the stub's open-Q1 now resolved to A): a router value with
  `router.get("/users/:id", handler)` / `.post(…)` etc.; the handler reads path params
  from the request (`req.param("id")`); dispatch matches method+path, extracting
  `:param` segments. Static segments beat param segments; param beats wildcard;
  unmatched path → 404, matched path with unmatched method → 405.

## 2. Where the code lives (grep findings)

`jet.http` is **not** a `.jet` file — it is a compiler-known namespace implemented as
Rust templates:
- Runtime templates: `Source/Prelude/CoreLib.rs` — `JetHttpRequest`/`JetHttpResponse`
  structs (lines ~1013–1036), `jet_http_serve` (1184), `jet_http_parse_request` (1210),
  `jet_http_format_response` (1228).
- Type registration: `Source/Sema/CheckerItems.rs` (compiler-known constructable
  structs HttpRequest/HttpResponse) + `Source/Sema/CheckerCoreLib.rs` (`("jet.http", …)`
  call resolution, method types: `("HttpRequest", "method"|"path"|"body") -> String`).
- Codegen name mapping: `Source/Codegen/Context.rs` (`"HttpRequest" => "JetHttpRequest"`).
- The existing example: `examples/features/57_http_server.jet` (hand-branches on
  `req.path()`).

So this is **mostly stdlib (Rust templates) + a thin slice of sema** to type the new
router type/methods. **No lexer/parser/AST/grammar change** (I8 — routing is a library,
not language syntax; the stub is explicit about this).

## 3. Failing-test-first targets

1. **`examples/features/75_http_routes.jet`** + `expected/75_http_routes.out` (golden,
   I5): build a router, register `GET /` , `GET /users/:id`, `POST /users`; drive a few
   requests in-process (same one-task-per-connection + channel pattern as `57_…` for
   golden determinism); print each response body, including the extracted `:id`.
2. **`tests/ui/http_param_typed.rs`** *(only if Q2 typed params are in scope — see §4
   note)*: `req.param("id")` returns `String`; assert the type.
3. **Router unit behaviour** via the golden: a request to `/users/42` routes to the
   `:id` handler and `req.param("id") == "42"`; `/users` POST routes to the POST
   handler; an unknown path returns 404; known path + wrong method returns 405.
4. **`tests/ui/http_dup_route.rs`** *(optional, see §4)*: registering the same
   method+pattern twice → **E2804** at registration (or a documented runtime panic).

## 4. Pipeline work, in order

### Runtime templates — `Source/Prelude/CoreLib.rs` (the bulk)
- Add a **`JetHttpRouter`** struct: `Vec<(Method, PatternSegments, Handler)>` where a
  pattern is parsed once at registration into segments (`Static(String)` |
  `Param(String)`). Handler type: `Arc<dyn Fn(JetHttpRequest) -> JetHttpResponse + Send + Sync>`.
- `jet_http_router_new() -> JetHttpRouter`.
- `jet_http_router_register(&mut router, method, pattern, handler)` — splits the pattern
  on `/`, classifies each segment; (optional) checks for an existing identical
  method+pattern → panic/E2804.
- **Dispatch**: a `jet_http_router_dispatch(&router, req) -> JetHttpResponse` that:
  splits `req.path` into segments; for each registered route, match method first, then
  segment-by-segment (static must equal; param captures); on full match, populate the
  request's params map and call the handler. Precedence: try static-heaviest routes
  first (sort routes so all-static patterns match before param patterns; param before
  any wildcard). No match path → 404; path matched but method didn't → 405. Wire the
  router into the existing `jet_http_serve` loop so `serve(addr, router)` works (overload
  or a new `jet_http_serve_router`).
- **Param storage**: add a `params: BTreeMap<String,String>` field to `JetHttpRequest`
  (or a side map filled at dispatch). `req.param(name)` returns `String` (Q2 = always
  String for v1 — typed `:id<Int>` is a follow-up, not in D-ROUTE1's A scope; note it).

### Sema — `Source/Sema/CheckerCoreLib.rs` + `CheckerItems.rs`
- Register the **HttpRouter** type as a compiler-known constructable type
  (`CheckerItems.rs`, alongside HttpRequest/HttpResponse).
- Add `("jet.http", "router")` → returns `HttpRouter` (arity 0).
- Method types on `HttpRouter`: `.get(String, Fn(HttpRequest)->HttpResponse) -> Unit`
  (and `.post`/`.put`/`.delete`); on `HttpRequest`: `.param(String) -> String` (or
  `String?` if a missing param should be optional — pick `String?` and teach that an
  unmatched param is `none`). Add these to the `("HttpRequest", …)` and new
  `("HttpRouter", …)` arms next to the existing `method`/`path`/`body` arms.
- `("jet.http", "serve")` already exists for a closure handler; extend it to also accept
  an `HttpRouter` as the handler argument (or add `serve` overload resolution).

### Codegen — `Source/Codegen/Context.rs`
- Add `"HttpRouter" => "JetHttpRouter"` to the core type-name map (mirror the existing
  HttpRequest/HttpResponse entries). Codegen stays dumb — sema already typed everything.

### Lexer / Parser / AST
**None.** No new syntax (I8).

## 5. Diagnostics

The networking block uses E28xx (E2801–E2803 exist). Next free: **E2804**.

| Code | What | Why | Fix |
|------|------|-----|-----|
| **E2804** *(optional)* | "two routes both match `<METHOD> <pattern>`" | a duplicate route makes dispatch ambiguous. | "remove one, or make the patterns distinct" |

D-ROUTE1's stub marks duplicate-route detection as *desirable but optional for v1* — it
may begin as a registration-time panic and graduate to E2804 later. If shipped as a
diagnostic, it needs a ui snapshot + `jet explain` (I4). 404/405 are runtime responses,
not diagnostics. **Minimum viable: no new diagnostic** (routing is pure stdlib).

## 6. Examples

- `examples/features/75_http_routes.jet` (the golden in §3.1). Expected output: the
  response bodies for `/`, `/users/42` (showing `id=42`), the POST, plus the 404/405
  bodies — deterministic, one connection at a time like `57_…`.

## 7. Exit criteria

- `75_http_routes.jet` builds and matches golden output; param extraction, 404, 405 all
  exercised.
- Router type + methods type-check (sema arms added); `serve(addr, router)` works.
- `tests/golden.rs` green; no `unsafe` (the templates are safe std).
- If E2804 shipped: ui snapshot + explain.

## 8. Effort / risk + one-pass judgment

Almost entirely stdlib Rust in one file (`CoreLib.rs`) plus four small sema arms and one
codegen name entry — no compiler-grammar surface. The pattern matcher and precedence are
standard, well-bounded work. Main risk is the golden-test determinism around the server
loop, but `57_http_server.jet` already established the one-connection-per-task + channel
pattern to copy. The optional E2804 is the only judgment call; ship it as a runtime
panic first if time-boxed.

**Completable in one focused agent pass? YES.** Self-contained, mostly stdlib, no
upstream gate. Lower-risk than c74 because it touches no grammar.
