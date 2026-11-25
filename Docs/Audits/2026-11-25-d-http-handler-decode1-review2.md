# D-HTTP-HANDLER-DECODE1: second adversarial review

Reviewer: HandlerDecodeReview2 (Claude Opus), fresh agent. Author: G1Checkers (Claude Opus).
Earlier pass: HandlerDecodeReview, [2026-11-25-d-http-handler-decode1-review.md](2026-11-25-d-http-handler-decode1-review.md).
Draft: `$JET_SCRATCH_ROOT/tmp/g1gen2/g1c/D-HTTP-HANDLER-DECODE1.json` (validator: ok).
Repaired copy: `$JET_SCRATCH_ROOT/tmp/orch/land/D-HTTP-HANDLER-DECODE1.reviewed2.json` (validator: ok).
Code read at dev tip de8a554abe1. `git diff 35cc4235d3b HEAD` changes only the first review's audit
file, so the draft's line numbers still hold.

Verdict: **back to the author; do not post.** The repaired copy fixes every claim that could be
fixed in place, but recommendation A, as drafted, changes the meaning of programs at three handler
sites (B1-B3 below). The owner standard asks for a recommendation with no downsides. Removing these
changes the trait, so A needs a redesign and a fresh adversarial pass. The copy names B1-B3 as
"Unresolved, blocks posting" losses with the best fix found for each.

## Findings

F1 (sample does not parse). Option A's code had `{ … }` bodies for `answer`, `done` and `failed`:
probe optA gives Jet.StrayCharacter at 12:61, 13:38 and 14:61. The surface sample s0 fails the same
way (4:66, 5:56, 6:5). Repaired: real bodies, and the unused `index` renamed `_index`. The repaired
code (optA2) leaves only Jet.ReservedDunderName, expected because the impl lives in core.http; the
same impl on a local stand-in type (pFinal) checks clean.

F2 (trait as written refuses its own adapter). The trait's reply members had no `Never!`, so they
fail with `Err`. Probe pA: four Jet.ErrorNotPropagatable, "`rejected()` fails with `Err`, which can't
propagate into `HTTPError` here", and the same for `done`, `failed` and `answer`, inside a hand-built
`HTTPResponse HTTPError!` adapter. Probe pA3, identical but with `Never!`, checks clean. Repaired in
the trait and the impl.

F3 (effects clause wrong; it would refuse handlers accepted today). The draft said the adapter "is
checked against the parameter's function type like any lambda, so `__body`'s `Net` fits
`fn(HTTPRequest) -[Net]> …`". No site bounds a handler's effects today: the router checks the
adapted handler with no effect bound (Calls/HTTPRouting.jet:1278-1287), the App adapter is exempt
from its slot's row (Effects/Checks.jet:3423-3429), and mount callbacks use `effect_bound: None`
(Calls/HTTPMount.jet:1260-1272). Probe pEff2: a typed handler using IO and Time checks on a router
verb and an App verb. Probe pEff: an App page using IO and Time is accepted by `route`, and the same
body is refused with Jet.CallbackEffectBoundExceeded against a `fn() -[Net, FS.Read]> WebPage`
parameter. Repaired: the public handler parameters carry no effect row; the private slot rows
(`-[Net]` at Core/HTTP/HTTP.jet:757, `-[Net, FS.Read]` at Core/Web/Web.jet:135-432 and
Core/HTTP/Server.jet:202), the exemption and the unbounded mount check go in the cutover.

F4 (blocking, B1). The trait makes `answer`, `done` and `failed` mandatory, so a route handler
returning something other than HTTPResponse is now accepted and answered through `answer`. Today it
is refused (probe pRet: Jet.BindingTypeMismatch "This needs HTTPResponse, but the value is
String"). No ratified text sets that route's status or body, so the copy marks the sample bodies as
this review's reading. Under D-RULE-BODYFACTS1=A ("rule rejects only when `returns` is empty on a
router") such a route has an empty `returns` list: a router refuses it with E2805, while an App verb
is accepted and `web.openapi` fails when it runs.

F5 (blocking, B2). The rule tries a declared `From` before `answer`. A loader, action, form or data
handler returning a WebPage would answer a page; today `sema_app_handler_adapter` wraps those kinds
with `__json` (Calls/Methods.jet:1369-1433), and WebPage is Encode (probe pFrom prints `1`). A
route, page or layout handler returning any Encode value would answer JSON; today it is refused
(probe pPage: Jet.ValueTypeMismatch "`__page` wants `WebPage` for argument 1, but this is String").
Probe pFrom also shows `impl AppReply.From(WebPage)` converts (prints `true`).

F6 (blocking, B3). Mounted methods lose their decode errors. Today `__take<T: Decode>(index) -> T
[FieldError]!` (Core/App/App.jet:8562-8564) fails with the decoder's field list; the server answers
422 with that list and commits nothing (Core/Web/Web.jet:2781, 2931, 3009, 3332), and a Remote caller
sees `CallError.Invalid(fields)` (Core/App/App.jet:7827-7833). Through `input -> T?` and
`rejected(name) -> Reply`, the reason is gone and the rejection is an ordinary reply.

F7 (site 2 inventory). The draft listed route, page, layout, loader, action, form and data. Methods.jet
also adapts `mount` (passes `__path()` and answers `__done`) and the `pending`, `not_found` and
`error` boundaries, which take no inputs (E2810); App handlers must be named functions (E2810,
Methods.jet:1881-1886, 1437-1447). The `__AppCall`/`__AppReply` range is Web.jet:90-132, not
:90-124. Repaired.

F8 (wrong citation; incomplete affected set). The draft cited Serde.jet:69-73 for auto-derived
Codable. The rule is spec.md:1122-1126: Encode is derived "when every field qualifies", and a package
may deny `auto_derive`. So a failure type with any non-Encode field is also refused under A, not only
denied packages, `#!Codable` and compiler-owned types. The failure text also changes: today it is
"the `{name}` handler failed: {err}" (Methods.jet:1410-1419, not :1421), and `failed` gets only the
value. Repaired.

F9 (expert path limit). A package can't write the adapter generically over the trait. Probe pA2:
Jet.UnknownType "`input` is not generic ... Trait method declarations do not carry independent type
parameters", Jet.UnknownFunction "`S` has no method `input`", and Jet.ReturnTypeMismatch "This value
is `Reply`, but `refuse` returns S.Reply". The rule calls the impl on the concrete source (pA3), so
A still works; the copy states that a package router names its source type.

F10 (site 4 claimed, not replaced). The draft lists "Site 4: the state decode" as replaced, and its
edge case says screens keep "their AppRoutes.jet lowering in Core's app module". A screen view's
parameters are `app.Remote<S>` services from the app graph, and its state is built field by field
(`sema_app_state_made`, Calls/AppRoutes.jet:543-554); the opener is compiler code
(AppRoutes.jet:513-554, 42 lines), not Core. Repaired: screens and mount's method listing stay in
the compiler under every option, and neither has an owning card criterion.

F11 (option B understated). B's "mounted methods change the same way" can't hold: a mounted method's
typed parameters are also the `Remote<S>` call a client makes. App handlers would need a public App
call type instead of the private `__AppCall`. B's typed-lambda count is 41, not 42 (F12). B's E2805
checks run over the recorded `input` calls; probe optB shows today's check refuses B's sample with
Jet.InvalidRoute, as expected for a proposal that changes it. Repaired.

F12 (counts). The scan below gives 43 route lambdas with no inputs (46 less three string or
diagnostic lines), 112 request-only, one `fn(req)` with no type, 31 typed-only (32 less
MapSetMethods.jet:19), 10 request plus typed, and nine `^`-parameter Store `.patch` calls (the draft
said eight). Line counts hold: HTTPRouting.jet 1,406, HTTP.jet 408, HTTPRouting.jet:1353-1406 is 54
lines, Methods.jet:1369-1433 is 65. Repaired in the detail.

F13 (untracked gap). The draft says lambda FunctionInfo is "owed by card #6851". D-RULE-REWRITE1=A
assigns it to that card, but card #6851's criterion 30 ("CompilerArg.function carries the
FunctionInfo ... of a function-valued argument") is met for named functions only (spec.md:144-146),
and no open criterion covers lambdas. Repaired in the text; the card needs a criterion (Tower, not
written by this review).

F14 (smaller claims). "Stack traces name the written handler" was not probed; marked [INFERENCE].
Any parameter typed `fn(HTTPRequest) -> HTTPResponse`, with or without `HTTPError!`, adapts, including
one in a user's own API; Core's other request callbacks return HookDecision or HTTPAdmit
(Web.jet:844, Server.jet:294), so they are not adapted. Added as an edge case. Tip, check
instructions and the first review's path updated; `reviewPasses` records this pass with the
required prefix; why, gains and tradeoff now name the unsettled reply side.

F15 (checked and held). syntax-decisions.md:4540 (D-META-RECONCILE1), :4580-4581 (no call-site
expansion), :4651-4658 (D-META-COMPILER1 narrowing); Spellings.jet:41 and :49; `__route_input` and
`__route_rejected` (HTTP.jet:317-336); `server.response` (Server.jet:936) and `server.json` (:956);
E0112 (HTTPRouting.jet:1201-1210) and E2805 (:1185); the CheckedText (prelude.jet:57-70) and cypher
(cypher.jet:13-15) precedents; the From rail (spec.md:1992-2012); D-RULE-DISPATCH-BUDGET1's 19.8 µs
and 28-31 ms against a 1 µs budget; D-RULE-TYPEARGS1's JSON never mentions handlers; the E0067
claim (card #6851 criterion 39, met). Option C is honestly presented as reopening D-RULE-REWRITE1's
losing B and the no-macros rulings; D-CONC-CROSS1=A is treated as a fixed gate.

## Blocking reasons and suggested fixes

- B1 (F4): route handlers accept values refused today, with an unset status and body, and collide with
  D-RULE-BODYFACTS1's `returns`. Suggested fix: make the value, unit and failure members optional
  per source, for example a second trait that HTTPRequest does not implement, so route handlers keep
  today's refusal.
- B2 (F5): App answers switch between page and JSON. Suggested fix: one source per kind, where page
  kinds have no `answer` and data kinds have no `From(WebPage)`, so each kind accepts exactly what it
  accepts today.
- B3 (F6): mounted methods lose 422 and `CallError.Invalid`. Suggested fix: let `input` fail with the
  source's own error and pass that error to `rejected`, with a reply that can carry it.

Each fix changes the trait's shape, so the redesign needs a fresh adversarial pass.

## Probes (batch48-chain s1 compiler; scratch deleted after the review)

Each probe is one `main.jet` in its own directory, run from that directory as
`env -u DISPLAY -u WAYLAND_DISPLAY -u XDG_SESSION_TYPE JET_TOOLCHAIN_ROOT=<dev worktree> JET=<seed> <seed> check main.jet`
(`run` where noted). Output is verbatim.

### ex1: the draft's explainer program (`run`)

```jet
use {
    server: core.http.server
    web: core.web
}

#Codable
NewJoke: struct {
    text: String,
}

run: fn() {
    app: web.app()
    // `id` comes from the path and must decode as an Int.
    app.get("/jokes/:id", fn(id: Int) { Ok(server.response(200, "joke {id}")) })
    // This handler takes the request itself.
    app.post("/jokes", fn(req: HTTPRequest) {
        joke: req.json<NewJoke>()
        Ok(server.response(201, "added {joke.text}"))
    })
    print(web.openapi(app) ?? "no document")
}
```

```text
{"openapi":"3.1.0","info":{"title":"Jet HTTP Router","version":"1"},"paths":{"/jokes":{"post":{"operationId":"jet_post__jokes","requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","properties":{"text":{"type":"string"}},"additionalProperties":false,"required":["text"]}}}},"responses":{"201":{"description":"HTTP response"}}}},"/jokes/{id}":{"get":{"operationId":"jet_get__jokes__id_","parameters":[{"name":"id","in":"path","required":true,"schema":{"type":"integer","format":"int64"}}],"responses":{"200":{"description":"HTTP response"}}}}}}
```

### ex2: the explainer's sketch of the compiler's adapter, bound to a name

```jet
use server: core.http.server

run: fn() {
    // The explainer's sketch of the function the compiler writes, bound to a name.
    adapted: fn(__jet_http_request: HTTPRequest) {
        id: __jet_http_request.__route_input<Int>("id") ?? return Ok(__jet_http_request.__route_rejected())
        Ok(server.response(200, "joke {id}"))
    }
    print(adapted)
}
```

```text
Error: Jet.ReservedDunderName: `__jet_http_request` is reserved for Jet
  --> main.jet:5:17
    |
  5 |     adapted: fn(__jet_http_request: HTTPRequest) {
    |                 ^^^^^^^^^^^^^^^^^^
 Why: Double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools
 Fix: Rename it without the second leading underscore
More: jet-lang.dev/e/Jet.ReservedDunderName

Error: Jet.ReservedDunderName: `__jet_http_request` is reserved for Jet
  --> main.jet:6:13
    |
  6 |         id: __jet_http_request.__route_input<Int>("id") ?? return Ok(__jet_http_request.__route_rejected())
    |             ^^^^^^^^^^^^^^^^^^
 Why: Double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools
 Fix: Rename it without the second leading underscore
More: jet-lang.dev/e/Jet.ReservedDunderName

Error: Jet.ReservedDunderName: `__route_input` is reserved for Jet
  --> main.jet:6:32
    |
  6 |         id: __jet_http_request.__route_input<Int>("id") ?? return Ok(__jet_http_request.__route_rejected())
    |                                ^^^^^^^^^^^^^
 Why: Double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools
 Fix: Rename it without the second leading underscore
More: jet-lang.dev/e/Jet.ReservedDunderName

Error: Jet.ReservedDunderName: `__jet_http_request` is reserved for Jet
  --> main.jet:6:70
    |
  6 |         id: __jet_http_request.__route_input<Int>("id") ?? return Ok(__jet_http_request.__route_rejected())
    |                                                                      ^^^^^^^^^^^^^^^^^^
 Why: Double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools
 Fix: Rename it without the second leading underscore
More: jet-lang.dev/e/Jet.ReservedDunderName

Error: Jet.ReservedDunderName: `__route_rejected` is reserved for Jet
  --> main.jet:6:89
    |
  6 |         id: __jet_http_request.__route_input<Int>("id") ?? return Ok(__jet_http_request.__route_rejected())
    |                                                                                         ^^^^^^^^^^^^^^^^
 Why: Double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools
 Fix: Rename it without the second leading underscore
More: jet-lang.dev/e/Jet.ReservedDunderName

5 problems found
Run `jet explain Jet.ReservedDunderName` to learn more
```

It parses; only the reserved names it is labeled as compiler output for are refused.

### optA: the draft's option A code, verbatim (F1)

```jet
use {
    server: core.http.server
    web: core.web
}

// PROPOSED, in core.http: a request says how it gives a named input.
impl HTTPRequest.HandlerSource {
    Reply: HTTPResponse,

    input: fn<T: Decode>(self, name: String, index: Int) -> T? { self.__route_input<T>(name) }
    rejected: fn(self, name: String) -> HTTPResponse { self.__route_rejected() }
    answer: fn<R: Encode>(self, value: R) -> HTTPResponse { … }
    done: fn(self) -> HTTPResponse { … }
    failed: fn<E: Encode>(self, error: E) -> HTTPResponse { … }
}

run: fn() {
    app: web.app()
    // Unchanged: the language adapts this through HTTPRequest's impl.
    app.get("/jokes/:id", fn(id: Int) { Ok(server.response(200, "joke {id}")) })
    print(web.openapi(app) ?? "no document")
}
```

```text
Error: Jet.ReservedDunderName: `__route_input` is reserved for Jet
  --> main.jet:10:71
    |
 10 |     input: fn<T: Decode>(self, name: String, index: Int) -> T? { self.__route_input<T>(name) }
    |                                                                       ^^^^^^^^^^^^^
 Why: Double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools
 Fix: Rename it without the second leading underscore
More: jet-lang.dev/e/Jet.ReservedDunderName

Error: Jet.ReservedDunderName: `__route_rejected` is reserved for Jet
  --> main.jet:11:61
    |
 11 |     rejected: fn(self, name: String) -> HTTPResponse { self.__route_rejected() }
    |                                                             ^^^^^^^^^^^^^^^^
 Why: Double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools
 Fix: Rename it without the second leading underscore
More: jet-lang.dev/e/Jet.ReservedDunderName

Error: Jet.StrayCharacter: The character `…` doesn't mean anything here (yet)
  --> main.jet:12:61
    |
 12 |     answer: fn<R: Encode>(self, value: R) -> HTTPResponse { … }
    |                                                             ^
 Why: Unprintable characters are shown as `\0`, `\t`, or `U+XXXX` so you can identify them
 Fix: Remove it, or use supported syntax
More: jet-lang.dev/e/Jet.StrayCharacter

Error: Jet.StrayCharacter: The character `…` doesn't mean anything here (yet)
  --> main.jet:13:38
    |
 13 |     done: fn(self) -> HTTPResponse { … }
    |                                      ^
 Why: Unprintable characters are shown as `\0`, `\t`, or `U+XXXX` so you can identify them
 Fix: Remove it, or use supported syntax
More: jet-lang.dev/e/Jet.StrayCharacter

Error: Jet.StrayCharacter: The character `…` doesn't mean anything here (yet)
  --> main.jet:14:61
    |
 14 |     failed: fn<E: Encode>(self, error: E) -> HTTPResponse { … }
    |                                                             ^
 Why: Unprintable characters are shown as `\0`, `\t`, or `U+XXXX` so you can identify them
 Fix: Remove it, or use supported syntax
More: jet-lang.dev/e/Jet.StrayCharacter

5 problems found
Run `jet explain Jet.ReservedDunderName` to learn more
```

### optA trait: the draft's trait, verbatim, alone in `trait.jet`

```jet
// PROPOSED, Prelude
HandlerSource: trait {
    Reply: type,
    // Input number `index`, named `name`; None when it is missing or does not decode.
    input: fn<T: Decode>(self, name: String, index: Int) -> T?,
    // The answer when input `name` is missing or does not decode.
    rejected: fn(self, name: String) -> Reply,
    // The answer for a handler value that is not a Reply, for a handler that returns nothing, and for a handler's own failure.
    answer: fn<R: Encode>(self, value: R) -> Reply,
    done: fn(self) -> Reply,
    failed: fn<E: Encode>(self, error: E) -> Reply,
}
```

```text
OK: `trait.jet` has no problems
```

### optA2: the repaired option A code (F1)

```jet
use {
    server: core.http.server
    web: core.web
}

// PROPOSED, in core.http: a request says how it gives a named input.
impl HTTPRequest.HandlerSource {
    Reply: HTTPResponse,

    input: fn<T: Decode>(self, name: String, _index: Int) -> T? { self.__route_input<T>(name) }
    rejected: fn(self, name: String) -> HTTPResponse Never! { self.__route_rejected() }
    // Not decided by today's code: a router handler returning anything else is refused today.
    answer: fn<R: Encode>(self, value: R) -> HTTPResponse Never! { server.json(200, value) ?? server.response(500, "{err}") }
    done: fn(self) -> HTTPResponse Never! { server.response(204, "") }
    failed: fn<E: Encode>(self, error: E) -> HTTPResponse Never! { server.json(500, error) ?? server.response(500, "{err}") }
}

run: fn() {
    app: web.app()
    // Unchanged: the language adapts this through HTTPRequest's impl.
    app.get("/jokes/:id", fn(id: Int) { Ok(server.response(200, "joke {id}")) })
    print(web.openapi(app) ?? "no document")
}
```

```text
Error: Jet.ReservedDunderName: `__route_input` is reserved for Jet
  --> main.jet:10:72
    |
 10 |     input: fn<T: Decode>(self, name: String, _index: Int) -> T? { self.__route_input<T>(name) }
    |                                                                        ^^^^^^^^^^^^^
 Why: Double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools
 Fix: Rename it without the second leading underscore
More: jet-lang.dev/e/Jet.ReservedDunderName

Error: Jet.ReservedDunderName: `__route_rejected` is reserved for Jet
  --> main.jet:11:68
    |
 11 |     rejected: fn(self, name: String) -> HTTPResponse Never! { self.__route_rejected() }
    |                                                                    ^^^^^^^^^^^^^^^^
 Why: Double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools
 Fix: Rename it without the second leading underscore
More: jet-lang.dev/e/Jet.ReservedDunderName

2 problems found
Run `jet explain Jet.ReservedDunderName` to learn more
```

### pA1: the draft's impl form on Core's HTTPRequest from a user file

```jet
use web: core.web

HandlerSource: trait {
    Reply: type,
    input: fn<T: Decode>(self, name: String, index: Int) -> T?,
    rejected: fn(self, name: String) -> Reply Never!,
}

// The draft's impl form written on Core's own HTTPRequest from a user file.
impl HTTPRequest.HandlerSource {
    Reply: HTTPResponse,
    input: fn<T: Decode>(self, name: String, _index: Int) -> T? { None }
    rejected: fn(self, name: String) -> HTTPResponse Never! { panic("no {name}") }
}

run: fn() {
    print(web.app())
}
```

```text
Error: Jet.ImplOnBuiltinType: `impl HTTPRequest.HandlerSource` needs a user-defined type
  --> main.jet:10:6
    |
 10 | impl HTTPRequest.HandlerSource {
    |      ^^^^^^^^^^^
 Why: No struct, enum, or distinct type named `HTTPRequest` is declared here, and an `impl` only extends a declared type
 Fix: Declare `HTTPRequest: struct`, `HTTPRequest: enum`, or `HTTPRequest: distinct …` first, or check the spelling
More: jet-lang.dev/e/Jet.ImplOnBuiltinType

1 problem found
Run `jet explain Jet.ImplOnBuiltinType` to learn more
```

Expected: the impl must live in core.http. The remaining probes use a local stand-in `Req`.

### pA: the draft's trait and impl on a stand-in, with a hand-built adapter (F2)

```jet
use {
    server: core.http.server
    web: core.web
    json: core.encoding.json
}

// The draft's trait, verbatim: no `Never!` on the reply members.
HandlerSource: trait {
    Reply: type,
    input: fn<T: Decode>(self, name: String, index: Int) -> T?,
    rejected: fn(self, name: String) -> Reply,
    answer: fn<R: Encode>(self, value: R) -> Reply,
    done: fn(self) -> Reply,
    failed: fn<E: Encode>(self, error: E) -> Reply,
}

// The draft's impl shape on a local stand-in (a user file can't impl Core's HTTPRequest).
Req: struct {
    request: HTTPRequest,
}

impl Req.HandlerSource {
    Reply: HTTPResponse,

    input: fn<T: Decode>(self, name: String, _index: Int) -> T? {
        text: self.request.param(name) ?? return None
        if json.decode<T>(text) == .Ok(value) -> return Val(value)
        None
    }
    rejected: fn(self, name: String) -> HTTPResponse { server.response(400, "bad {name}") }
    answer: fn<R: Encode>(self, value: R) -> HTTPResponse { server.json(200, value) ?? server.response(500, "") }
    done: fn(self) -> HTTPResponse { server.response(204, "") }
    failed: fn<E: Encode>(self, error: E) -> HTTPResponse { server.json(500, error) ?? server.response(500, "") }
}

// What the rule would build for `fn(id: Int)`, written by hand on the concrete source.
adapt: fn(source: Req) -> HTTPResponse HTTPError! {
    id: source.input<Int>("id", 0) ?? return Ok(source.rejected("id"))
    if id == 0 -> return Ok(source.done())
    if id == 1 -> return Ok(source.failed("boom"))
    Ok(source.answer("joke {id}"))
}

run: fn() {
    app: web.app()
    app.get("/jokes/:id", fn(id: Int) { Ok(server.response(200, "joke {id}")) })
    print(web.openapi(app) ?? "no document")
}
```

```text
Error: Jet.ErrorNotPropagatable: `rejected()` fails with `Err`, which can't propagate into `HTTPError` here
  --> main.jet:38:56
    |
 38 |     id: source.input<Int>("id", 0) ?? return Ok(source.rejected("id"))
    |                                                        ^^^^^^^^
 Why: `rejected()` is defined at `Core or generated callable (no user source location)` with effective failure contract `inferred failure contract`; the caller's `explicit HTTPError!` contract has no route from `Err` to `HTTPError`
 Fix: Declare the caller with `Err` as its failure domain, or add `impl HTTPError.From(Err) { … }`; otherwise handle `rejected()` locally with `??`
More: jet-lang.dev/e/Jet.ErrorNotPropagatable

Error: Jet.ErrorNotPropagatable: `done()` fails with `Err`, which can't propagate into `HTTPError` here
  --> main.jet:39:36
    |
 39 |     if id == 0 -> return Ok(source.done())
    |                                    ^^^^
 Why: `done()` is defined at `Core or generated callable (no user source location)` with effective failure contract `inferred failure contract`; the caller's `explicit HTTPError!` contract has no route from `Err` to `HTTPError`
 Fix: Declare the caller with `Err` as its failure domain, or add `impl HTTPError.From(Err) { … }`; otherwise handle `done()` locally with `??`
More: jet-lang.dev/e/Jet.ErrorNotPropagatable

Error: Jet.ErrorNotPropagatable: `failed()` fails with `Err`, which can't propagate into `HTTPError` here
  --> main.jet:40:36
    |
 40 |     if id == 1 -> return Ok(source.failed("boom"))
    |                                    ^^^^^^
 Why: `failed()` is defined at `Core or generated callable (no user source location)` with effective failure contract `inferred failure contract`; the caller's `explicit HTTPError!` contract has no route from `Err` to `HTTPError`
 Fix: Declare the caller with `Err` as its failure domain, or add `impl HTTPError.From(Err) { … }`; otherwise handle `failed()` locally with `??`
More: jet-lang.dev/e/Jet.ErrorNotPropagatable

Error: Jet.ErrorNotPropagatable: `answer()` fails with `Err`, which can't propagate into `HTTPError` here
  --> main.jet:41:15
    |
 41 |     Ok(source.answer("joke {id}"))
    |               ^^^^^^
 Why: `answer()` is defined at `Core or generated callable (no user source location)` with effective failure contract `inferred failure contract`; the caller's `explicit HTTPError!` contract has no route from `Err` to `HTTPError`
 Fix: Declare the caller with `Err` as its failure domain, or add `impl HTTPError.From(Err) { … }`; otherwise handle `answer()` locally with `??`
More: jet-lang.dev/e/Jet.ErrorNotPropagatable

4 problems found
Run `jet explain Jet.ErrorNotPropagatable` to learn more
```

### pA3: pA with `Never!` on the reply members (F2)

pA's source with these changes only: the comment on line 7 reads "with `Never!` on the reply
members"; trait lines 11-14 end `-> Reply Never!,`; impl lines 30-33 return `HTTPResponse Never!`.

```text
OK: `main.jet` has no problems
```

### pFinal: the repaired trait and the repaired sample's impl on the stand-in

```jet
use {
    server: core.http.server
    web: core.web
    json: core.encoding.json
}

// The reviewed trait, as in the repaired ballot.
HandlerSource: trait {
    Reply: type,
    // Input number `index`, named `name`; None when it is missing or does not decode.
    input: fn<T: Decode>(self, name: String, index: Int) -> T?,
    // The answer when input `name` is missing or does not decode.
    rejected: fn(self, name: String) -> Reply Never!,
    // The answer for a handler value that is not a Reply, for a handler that returns nothing, and for a handler's own failure.
    answer: fn<R: Encode>(self, value: R) -> Reply Never!,
    done: fn(self) -> Reply Never!,
    failed: fn<E: Encode>(self, error: E) -> Reply Never!,
}

// Stand-in for HTTPRequest: a user file can't impl Core's type.
Req: struct {
    request: HTTPRequest,
}

// The repaired sample's impl, with `__route_input`/`__route_rejected` replaced by public calls.
impl Req.HandlerSource {
    Reply: HTTPResponse,

    input: fn<T: Decode>(self, name: String, _index: Int) -> T? {
        text: self.request.param(name) ?? return None
        if json.decode<T>(text) == .Ok(value) -> return Val(value)
        None
    }
    rejected: fn(self, name: String) -> HTTPResponse Never! { server.response(400, "bad {name}") }
    answer: fn<R: Encode>(self, value: R) -> HTTPResponse Never! { server.json(200, value) ?? server.response(500, "{err}") }
    done: fn(self) -> HTTPResponse Never! { server.response(204, "") }
    failed: fn<E: Encode>(self, error: E) -> HTTPResponse Never! { server.json(500, error) ?? server.response(500, "{err}") }
}

// What the rule would build for `fn(id: Int)`, by hand on the concrete source.
adapt: fn(source: Req) -> HTTPResponse HTTPError! {
    id: source.input<Int>("id", 0) ?? return Ok(source.rejected("id"))
    if id == 0 -> return Ok(source.done())
    if id == 1 -> return Ok(source.failed("boom"))
    Ok(source.answer("joke {id}"))
}

run: fn() {
    app: web.app()
    app.get("/jokes/:id", fn(id: Int) { Ok(server.response(200, "joke {id}")) })
    print(web.openapi(app) ?? "no document")
}
```

```text
OK: `main.jet` has no problems
```

### pA2: a package's generic adapter over the trait bound (F9)

```jet
use server: core.http.server

HandlerSource: trait {
    Reply: type,
    input: fn<T: Decode>(self, name: String, index: Int) -> T?,
    rejected: fn(self, name: String) -> Reply Never!,
}

Req: struct {
    path_id: String,
}

impl Req.HandlerSource {
    Reply: HTTPResponse,
    input: fn<T: Decode>(self, name: String, _index: Int) -> T? { None }
    rejected: fn(self, name: String) -> HTTPResponse Never! { server.response(400, "bad {name}") }
}

// A package's generic adapter over any source: the expert path written by hand.
adapt: fn<S: HandlerSource>(source: S) -> S.Reply {
    id: source.input<Int>("id", 0) ?? return source.rejected("id")
    source.rejected("got {id}")
}

// No generic member involved: only the associated type through the bound.
refuse: fn<S: HandlerSource>(source: S) -> S.Reply {
    source.rejected("id")
}

run: fn() {
    print(adapt(Req{path_id: "7"}))
    print(refuse(Req{path_id: "7"}))
}
```

```text
Error: Jet.UnknownType: `input` is not generic
  --> main.jet:21:16
    |
 21 |     id: source.input<Int>("id", 0) ?? return source.rejected("id")
    |                ^^^^^
 Why: Trait method declarations do not carry independent type parameters
 Fix: Remove the explicit method type arguments
More: jet-lang.dev/e/Jet.UnknownType

Error: Jet.UnknownFunction: `S` has no method `input`
  --> main.jet:21:16
    |
 21 |     id: source.input<Int>("id", 0) ?? return source.rejected("id")
    |                ^^^^^
 Why: None of type parameter `S`'s bounds (`HandlerSource`) provides `input`
 Fix: Add a bound that defines `input`, or use a method from `HandlerSource`
More: jet-lang.dev/e/Jet.UnknownFunction

Error: Jet.ReturnTypeMismatch: This value is `Reply`, but `refuse` returns S.Reply
  --> main.jet:27:12
    |
 27 |     source.rejected("id")
    |            ^^^^^^^^
 Why: `refuse` declares S.Reply as its result, so the value it hands back must be S.Reply
 Fix: Hand back a S.Reply value: convert this value to S.Reply, or return the value you meant
More: jet-lang.dev/e/Jet.ReturnTypeMismatch

Error: Jet.ValueTypeMismatch: `print` doesn't know how to show `S.Reply`
  --> main.jet:31:11
    |
 31 |     print(adapt(Req{path_id: "7"}))
    |           ^^^^^
 Why: Print shows values that have a display
 Fix: Print one of its parts instead
More: jet-lang.dev/e/Jet.ValueTypeMismatch

Error: Jet.ValueTypeMismatch: `print` doesn't know how to show `S.Reply`
  --> main.jet:32:11
    |
 32 |     print(refuse(Req{path_id: "7"}))
    |           ^^^^^^
 Why: Print shows values that have a display
 Fix: Print one of its parts instead
More: jet-lang.dev/e/Jet.ValueTypeMismatch

5 problems found
Run `jet explain Jet.UnknownType` to learn more
```

### pEff: an App page using IO and Time, unbounded and bounded (F3)

```jet
package: {
    name: "peff"
    authority: { allow: [FS.Read, IO, Net, Time] }
}

use {
    time: core.time
    web: core.web
}

// A page handler that uses IO and Time, outside the App slot's `-[Net, FS.Read]` row.
gallery: fn() -> WebPage {
    print("rendering")
    web.page("Gallery", "at {time.unix_seconds(time.now())}")
}

// The same handler checked against the slot's row, as the draft's effects clause would.
bounded: fn(page: fn() -[Net, FS.Read]> WebPage) -> WebPage { page() }

run: fn() {
    app: web.app().route("/", gallery)
    shown: bounded(fn() {
        print("rendering")
        web.page("Gallery", "at {time.unix_seconds(time.now())}")
    })
    print(shown.title)
    print(web.openapi(app) ?? "no document")
}
```

```text
Error: Jet.CallbackEffectBoundExceeded: This callback uses the effect `IO, Time`, which the parameter doesn't allow
  --> main.jet:22:20
    |
 22 |     shown: bounded(fn() {
    |                    ^^^^^^
 Why: The parameter is `fn(…) -[Net, FS.Read]>`, so the callback may use only those effects; `IO, Time` is outside that bound
 Fix: Pass a callback within `-[Net, FS.Read]>`, or add `IO, Time` to the parameter's bound
 Denial: invocation
More: jet-lang.dev/e/Jet.CallbackEffectBoundExceeded

1 problem found
Run `jet explain Jet.CallbackEffectBoundExceeded` to learn more
```

Line 21's `route("/", gallery)` is accepted; only the bounded parameter refuses the same body.

### pEff2: typed router and App verbs using IO and Time (F3)

```jet
package: {
    name: "peff2"
    authority: { allow: [FS.Read, IO, Net, Time] }
}

use {
    http: core.http
    server: core.http.server
    time: core.time
    web: core.web
}

run: fn() {
    router := http.router()
    // A typed router handler with IO and Time, outside `__route`'s `-[Net]` row.
    router.get("/a/:id", fn(id: Int) {
        print("a")
        Ok(server.response(200, "{id} {time.unix_seconds(time.now())}"))
    })
    app: web.app()
    // The same on an App verb, outside `__http_route`'s `-[Net, FS.Read]` row.
    app.get("/b/:id", fn(id: Int) {
        print("b")
        Ok(server.response(200, "{id} {time.unix_seconds(time.now())}"))
    })
    print(web.openapi(app) ?? "no document")
}
```

```text
Warning: Jet.MutableNeverReassigned: `router` never changes, so declare it with `:`
  --> main.jet:14:5
    |
 14 |     router := http.router()
    |     ^^^^^^
 Why: `:=` tells the reader the value changes later, but nothing in its scope writes it
 Fix: Write `router: …` instead of `router := …`
More: jet-lang.dev/e/Jet.MutableNeverReassigned

OK: `main.jet` has no errors (1 warning)
Run `jet explain Jet.MutableNeverReassigned` to learn more
```

### pRet: a router handler returning a String (F4)

```jet
use http: core.http

run: fn() {
    router: http.router()
    // A router handler whose value is not an HTTPResponse.
    router.get("/a/:id", fn(id: Int) { Ok("joke {id}") })
    print(router)
}
```

```text
Error: Jet.BindingTypeMismatch: This needs HTTPResponse, but the value is String
  --> main.jet:6:43
    |
  6 |     router.get("/a/:id", fn(id: Int) { Ok("joke {id}") })
    |                                           ^^^^^^^^^^^
 Why: The value here does not have the type required by this declaration
 Fix: Use a value of the required type or convert it explicitly
More: jet-lang.dev/e/Jet.BindingTypeMismatch

Error: Jet.ValueTypeMismatch: `print` doesn't know how to show `HTTPRouter`
  --> main.jet:7:11
    |
  7 |     print(router)
    |           ^^^^^^
 Why: Print shows values that have a display
 Fix: Print one of its parts instead
More: jet-lang.dev/e/Jet.ValueTypeMismatch

2 problems found
Run `jet explain Jet.BindingTypeMismatch` to learn more
```

The second error is the probe's own `print`; the first is the evidence.

### pPage: a page handler returning a String (F5)

```jet
use web: core.web

// A page handler whose value is not a WebPage.
home: fn() -> String { "hello" }

run: fn() {
    app: web.app().route("/", home)
    print(web.openapi(app) ?? "no document")
}
```

```text
Error: Jet.ValueTypeMismatch: `__page` wants `WebPage` for argument 1, but this is String (text)
  --> main.jet:7:31
    |
  7 |     app: web.app().route("/", home)
    |                               ^^^^
 Why: Every argument must match its parameter's type
 Fix: Use `WebPage` here
More: jet-lang.dev/e/Jet.ValueTypeMismatch

1 problem found
Run `jet explain Jet.ValueTypeMismatch` to learn more
```

### pFrom: a `From(WebPage)` reply and WebPage's Encode bound (`run`, F5)

```jet
use web: core.web

// A stand-in for the private `__AppReply`.
AppReply: enum {
    Page(WebPage),
    JSON(String),
}

impl AppReply.From(WebPage) {
    from: fn(page: WebPage) -> AppReply { .Page(page) }
}

// A data kind's `answer` takes any Encode value.
encoded: fn<R: Encode>(value: R) -> Int { 1 }

run: fn() {
    page: web.page("Home", "hello")
    reply: AppReply.from(page.copy())
    print(reply == .Page(_))
    print(encoded(page))
}
```

```text
true
1
```

### optB, optC, optD: the other options' samples, verbatim (F11)

optB:

```jet
use {
    server: core.http.server
    web: core.web
}

run: fn() {
    app: web.app()
    // PROPOSED: the handler takes the request and reads each input.
    app.get("/jokes/:id", fn(req: HTTPRequest) {
        id: req.input<Int>("id") ?? return Ok(req.rejected())
        Ok(server.response(200, "joke {id}"))
    })
    print(web.openapi(app) ?? "no document")
}
```

```text
Error: Jet.InvalidRoute: HTTP route `/jokes/:id` path field `id` does not have exactly one typed handler input
  --> main.jet:9:13
    |
  9 |     app.get("/jokes/:id", fn(req: HTTPRequest) {
    |             ^^^^^^^^^^^^
 Why: Path placeholders are bound by name before request decoding
 Fix: Declare one `id: T` handler input
More: jet-lang.dev/e/Jet.InvalidRoute

1 problem found
Run `jet explain Jet.InvalidRoute` to learn more
```

optC:

```jet
use {
    server: core.http.server
    web: core.web
}

run: fn() {
    app: web.app()
    // Core's #RouteTable marker replaces this call with code it writes.
    app.get("/jokes/:id", fn(id: Int) { Ok(server.response(200, "joke {id}")) })
    print(web.openapi(app) ?? "no document")
}
```

```text
OK: `main.jet` has no problems
```

optD:

```jet
use {
    server: core.http.server
    web: core.web
}

run: fn() {
    app: web.app()
    // The compiler adapts this, keyed by a role on `get`'s handler parameter.
    app.get("/jokes/:id", fn(id: Int) { Ok(server.response(200, "joke {id}")) })
    print(web.openapi(app) ?? "no document")
}
```

```text
OK: `main.jet` has no problems
```

### s0: the draft's surface sample for A, verbatim (F1)

```jet
// PROPOSED, core.http
impl HTTPRequest.HandlerSource {
    Reply: HTTPResponse,
    input: fn<T: Decode>(self, name: String, index: Int) -> T? { … }
    rejected: fn(self, name: String) -> HTTPResponse { … }
    …
}

// Unchanged.
app.get("/jokes/:id", fn(id: Int) { Ok(server.response(200, "joke {id}")) })
```

```text
Error: Jet.StrayCharacter: The character `…` doesn't mean anything here (yet)
  --> main.jet:4:66
    |
  4 |     input: fn<T: Decode>(self, name: String, index: Int) -> T? { … }
    |                                                                  ^
 Why: Unprintable characters are shown as `\0`, `\t`, or `U+XXXX` so you can identify them
 Fix: Remove it, or use supported syntax
More: jet-lang.dev/e/Jet.StrayCharacter

Error: Jet.StrayCharacter: The character `…` doesn't mean anything here (yet)
  --> main.jet:5:56
    |
  5 |     rejected: fn(self, name: String) -> HTTPResponse { … }
    |                                                        ^
 Why: Unprintable characters are shown as `\0`, `\t`, or `U+XXXX` so you can identify them
 Fix: Remove it, or use supported syntax
More: jet-lang.dev/e/Jet.StrayCharacter

Error: Jet.StrayCharacter: The character `…` doesn't mean anything here (yet)
  --> main.jet:6:5
    |
  6 |     …
    |     ^
 Why: Unprintable characters are shown as `\0`, `\t`, or `U+XXXX` so you can identify them
 Fix: Remove it, or use supported syntax
More: jet-lang.dev/e/Jet.StrayCharacter

3 problems found
Run `jet explain Jet.StrayCharacter` to learn more
```

The repaired copy's surface sample has the same impl as optA2.

### Handler scan (F12)

Run at de8a554abe1 from the repository root:
`git grep -n -E '\.(get|post|put|patch|delete|head|options|custom)\(.*fn\(' -- '*.jet' | awk -f count.awk`

```awk
# Classifies the first `fn(params)` after the first verb call on each line.
{
    if (match($0, /\.(get|post|put|patch|delete|head|options|custom)\(/) == 0) next
    rest = substr($0, RSTART)
    if (match(rest, /fn\([^)]*\)/) == 0) next
    params = substr(rest, RSTART + 3, RLENGTH - 4)
    gsub(/^ +| +$/, "", params)
    if (params == "") kind = "none"
    else if (params ~ /\^/) kind = "mutable-param (Store .patch)"
    else {
        n = split(params, ps, ",")
        req = 0; typed = 0; bare = 0
        for (i = 1; i <= n; i++) {
            p = ps[i]; gsub(/^ +| +$/, "", p)
            if (p ~ /: *HTTPRequest$/) req++
            else if (p ~ /:/) typed++
            else bare++
        }
        if (bare > 0 && req == 0 && typed == 0) kind = "untyped param"
        else if (req > 0 && typed == 0) kind = "request"
        else if (req > 0) kind = "request+typed"
        else kind = "typed"
    }
    count[kind]++
}
END { for (k in count) print "TOTAL", k, count[k] }
```

```text
TOTAL mutable-param (Store .patch) 9
TOTAL none 46
TOTAL request 112
TOTAL request+typed 10
TOTAL typed 32
TOTAL untyped param 4
```

Not route handlers, and excluded: "none" Compiler/JetCLI/Source/CLI/Host.jet:3666,
Compiler/JetSema/Source/Sema/Calls/ReactiveHandles.jet:122 and Tests/Guards/network_paths.jet:173
(text inside strings); "typed" Examples/Collections/MapSetMethods.jet:19 (`merge(conflict:)`);
"untyped param" Compiler/JetSema/Source/Sema/Calls/SharedOwner.jet:230,
Tools/Perf/AsmParity/AsmParity/run.jet:380 and Tools/Perf/Scaling/Gate/Source/Gate.jet:588. The
remaining untyped one is the route `mux.get("/", fn(req) { … })` in
Tests/Cases/LanguageFeatures/HTTPMuxUnannotatedHandlerLambdaIsOwnedInAOT/Input/run.jet:10.

## What the repaired copy changes

- Option A: a parsing sample; `Never!` on the reply members; the effects clause (F3); site 2's full
  kind list and E2810 rules; sites 1-3 each carry their unresolved reply change (B1-B3); site 4 is
  stated as not replaced; the generic-bound limit; corrected Encode rule, citation and failure text;
  the `fn(HTTPRequest) -> HTTPResponse` edge case; stack traces marked [INFERENCE].
- Option B: mounted methods can't move to taking the request; App handlers need a public call type;
  41 typed lambdas.
- Detail: tip de8a554abe1; corrected counts; the effects fact; the lambda FunctionInfo criterion gap;
  screens and mount's listing kept in the compiler under every option.
- Recommendation: losses gain B1-B3 as "Unresolved, blocks posting" with their best fixes, and the
  Encode loss gains the text change; why, gains and tradeoff name the unsettled reply side.
- `checkInstructions` and `reviewPasses` point at this audit's predecessor and record this pass.

## Validator

`jet Tools/Agent/Ballots/Validate.jet` on the repaired copy:

```text
note: Tools/Agent/Ballots/Validate.jet ends its own process and reads files (-A hides this)
ok …/D-HTTP-HANDLER-DECODE1.reviewed2.json (D-HTTP-HANDLER-DECODE1)
```
