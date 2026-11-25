# D-HTTP-HANDLER-DECODE1: adversarial review

Reviewer: HandlerDecodeReview (Claude Opus), fresh agent. Author: G1Checkers (Claude Opus).
Draft: `$JET_SCRATCH_ROOT/tmp/g1gen2/g1c/D-HTTP-HANDLER-DECODE1.json` (validator: ok).
Revised copy: `$JET_SCRATCH_ROOT/tmp/orch/land/D-HTTP-HANDLER-DECODE1.reviewed.json`.
Code read at dev tip 35cc4235d3b. `git diff 08d2eae9774 HEAD` over Compiler/JetSema/Source/Sema/Calls,
Core/HTTP, Core/Web, Docs/Spec/spec.md and Compiler/JetFoundation/Data/Syntax is empty, so the
draft's batch-48 line numbers still hold.

Verdict: **back to the author.** The draft's recommendation fails. The revised copy narrows the
ballot to the one undecided piece, changes `rec`, and its new option A needs its own fresh
adversarial pass before posting.

## Findings

F1 (fatal). Option A re-ballots a choice ratified the day before. D-RULE-REWRITE1 (ratified
2026-11-24, outcome A) asked how a Core declaration attaches a per-call value to a route call. Its
option B, "a rule may replace its call", lost: whyNot B says it is "a call-site rewrite in standard
code, which D-META-LADDER1=A and D-META-RECONCILE1=A rule out". The draft's `expand_call` is a wider
form of that B (it returns arbitrary `#code`, not a target and arguments). The draft cites neither
decision.

F2 (fatal). `FunctionInfo.calls` with `returned: Bool` re-ballots D-RULE-BODYFACTS1 (ratified
2026-11-24, A): a `#Recorded` Prelude marker, `FunctionInfo.recorded` and `FunctionInfo.returns`,
and `CompilerCall.type_args`. The draft lists every call (A exposes only marked calls), adds
`CompilerCall.type` instead of the ratified `type_args`, and its option C is BODYFACTS1's losing D.
The OpenAPI body walk is therefore out of scope for this ballot.

F3 (fatal, I7/I8). The draft's A reintroduces call-site macros. syntax-decisions.md:4580-4581
(D-META-LADDER1=A): "Jet has no macros: no call-site expansion is registered." :4540
(D-META-RECONCILE1=A): "Written code means what its file says, and no rewrite happens." :4651-4658
(D-JAI-CODE1 narrowed by D-META-COMPILER1=A): `#Expand` and `name!()` removed. D-CORE-CHECK1=A's
rule only hears and refuses (spec.md:167-173). A second code-placement mechanism beside marker
`expand`, `meta.add` and `meta.rewrite_body` fails I8; the draft names none of these rulings.

F4 (I7). The draft's A syntax is not registered. Spellings.jet:49 (`#code`): it holds items, or a
block of statements for `meta.rewrite_body`; there is no one-expression form. Spellings.jet:41 and
:49: `#{name}` pastes a String build local's name or type. `#{call.args[i]}` (argument as written),
`#{call.receiver}`, `#{p.type}` from a TypeId and `expand_call` itself are new. The draft
presents them as reuse of `meta.add`'s paste.

F5 (inventory). Missed site: Calls/AppRoutes.jet (984 lines). `sema_call_app_screen` (:168) and
`sema_app_screen_opener` (:513-537) build the screen opener lambda; `sema_app_state_made`
(:543-554) decodes the state from address fields, the same decode-a-typed-input job. Also missed:
`app.mount` sends each marked method through the router path (HTTPMount.jet:752-755), so mount uses
the same handler wrap. "About 3,000 lines" is not reproducible: HTTP.jet 408 + HTTPRouting.jet 1,406
+ HTTPMount.jet 1,397 + AppJobs.jet 300 + Live.jet 508 = 4,019 whole-file lines before Methods.jet
and AppRoutes.jet; c3 deletes only the first two (1,814 lines).

F6 (I1/safety gap). The draft never mentions the capture check every handler gets today
(HTTPRouting.jet:1284-1287 via `sema_call_http_check_middleware_callback`, HTTP.jet:113-143).
Probe p2 (below) shows it: `Jet.ConcurrentCaptureNotOwned` at the handler lambda, "`hits` cannot
cross the `HTTP handler` boundary". Deleting the router path without a declared crossing
(D-CONC-CROSS1=A; D-RULE-REWRITE1's sixth gate) would admit a data race. The revised copy states it
as a gate shared by every option.

F7 (I9). The draft's A "doesn't run inside `#run`". Compile-time code is an I9 tier (AGENTS.md I9),
and check rules run "with one meaning ... at every tier" (spec.md:175-176). A route registered in
compile-time code would mean something else.

F8 (authority, I1-adjacent). Under the draft's A any dependency's marker could replace its users'
calls. D-META-LADDER1=A puts rewriting behind the root-only `Compiler` value and a locked
dependency's `allow_rewrite` consent (syntax-decisions.md:4571-4577). The draft gives packages
more than the root gets.

F9 (feasibility). `expand_call` can't reproduce today's adapter. HTTPRouting.jet:1388-1395 takes the
lambda's parameters off and splices its body into a new `fn(HTTPRequest)`; `#{call.args[i]}` pastes
the whole lambda, so the expansion must call it from a new closure. Once the web package leaves
Core, its template can't paste `__route_input`: a pasted `__` name is E0067 (card #6851, met
criterion on user marker templates). Neither is stated.

F10 (misstated facts). "`CompilerArg.function` is filled only for a declared function, never for a
lambda" is true of the build (spec.md:144-146) but D-CORE-CHECK1=A already promises "a
`FunctionInfo?` for a function or lambda argument". Lambda FunctionInfo is an unbuilt part of a
ratified decision, cardable on #6851, not one of A's "two reflection additions". "FunctionInfo has no
view of a body" is wrong: `reaches_panic` and `arithmetic` are body digests
(CoreRecordRows.jet:479-480).

F11 (unfair alternative). The draft's B, `get: fn<In: Decode, Out: Encode>(…, handler: fn(In) -> Out
HTTPError!, …)`, can't be built as written. "A handler that wants the raw request takes
`HTTPRequest`, which decodes as itself": Core/HTTP has no Decode impl for HTTPRequest (grep `impl
… Decode` in Core/HTTP: none; HTTP.jet:252 is a plain struct). Jet has no overloading (D-RULE-REWRITE1
probe p11), and the 43 `fn()` handlers match no `fn(In)`. The fair no-mechanism option is
request-only handlers with a public, `#Recorded` `req.input<T>(name)`, which reuses
D-RULE-BODYFACTS1's mechanism.

F12 (missing option). No option gave typed handler parameters without call rewriting, the
direction D-RULE-REWRITE1=A named ("until D-RULE-TYPEARGS1's typed handler params replace it") and
D-RULE-BODYFACTS1's option B text repeats. D-RULE-TYPEARGS1's ratified JSON has no mention of
handlers (`jq '[.. | strings | select(test("handler";"i"))]'` is empty), so that pointer dangles and
the adaptation has no owning vote. The revised A (a declared `HandlerSource` trait and one typing
rule, following the declared `From` conversion of spec.md:1992-2012) fills that gap.

F13 (cost). No number in the draft was measured. Per-request: the draft's figures for today match
Core/HTTP/HTTP.jet:317-336; A's "unchanged" and B's cost are inference. Build-time: the draft omits
the measured rule dispatch cost, about 19.8 µs per heard call and 28-31 ms for a module's first
(D-RULE-DISPATCH-BUDGET1, ratified 2026-11-25), against D-CORE-CHECK1's 1 µs, plus the recheck of
every expansion.

F14 (counts). "43 / 25 / 112 / 11" has no command. A one-line awk scan over `git ls-files '*.jet'`
of `.(get|post|put|patch|delete|head|options|custom)(… fn(params)` gives 43 no inputs, 112 request
only, 40 typed only (8 of them Store `.patch` calls, not routes, so 32), 10 request plus typed.
Multi-line lambdas and named handlers are not counted. Two of the request-plus-typed handlers are in
the compiler itself (Compiler/JetCanvas/Source/Host/CanvasHost.jet:204-205,
Compiler/JetDevServer/Source/WebHost03.jet:29-30).

F15 (scope). D-WEB-RUNTIME-API1=A keeps `live`, `subscribe`, `push`, `invalidate` in Core's app
module; AppJobs, Live and Remote calls encode a call's arguments at the caller (the opposite
direction) and are outside card #6851 c3. The revised copy keeps them separate with these reasons.

## Claims checked and held

- The rewrite: `sema_call_try_http_router` (HTTPRouting.jet:1123) calls `sema_http_mux_route_handler`
  (:1282, :1376-1406), then lowers to `__route`, `__http_route`, `__http_route_dynamic` or
  `__http_route_custom` (:1342-1348; Core/Web/Web.jet:531-544).
- E2805 binding checks: :1185, :1216-1277, :1313, :1325.
- Body walk: `sema_http_route_response_expr` (:767), `_statements` (:812), `find_json_body` (:973),
  `sema_http_route_contract` (:1060).
- App adapter: `sema_app_handler_adapter` (Methods.jet:1369-1433, called at :1888) over
  `__AppCall.__input/__body/__data` (Core/Web/Web.jet:90-107).
- Per-request decode: `__route_input` (Core/HTTP/HTTP.jet:317-329), `__route_rejected` (:331-336).
- `RouteTable` is a marker with no `check` (Core/Web/Web.jet:287-291); `Form` has a `check` (:297-310).

## Probes (batch48-chain s1 compiler; scratch deleted after the review)

- p1, `$JET_SCRATCH_ROOT/tmp/HandlerDecodeReview/p1/main.jet`: the draft's explainer program.
  `jet run` printed the OpenAPI document with `/jokes/{id}` (path parameter `id`, integer) and
  `/jokes` (request body NewJoke, response 201). The explainer's "it works" holds.
- p2, `.../p2/main.jet`: a typed handler that writes a captured `^hits`. `jet check` reports
  `Jet.ConcurrentCaptureNotOwned` at main.jet:9:27 ("cannot cross the `HTTP handler` boundary") and
  `Jet.UseWhileWriteActive` at 13:11.

## What the revised copy changes

- Scope: only the handler adaptation. Contract, body facts, E2804, `web.form`, routing home and the
  table are listed as fixed by their ratified votes.
- Inventory: four decode-side sites with file:symbol and lines, AppRoutes.jet added; encode-side and
  middleware kept separate with reasons and line counts.
- Options: A (new) declared request source; B request-only handlers with a recorded `input`; C the
  draft's call-expansion hook, with every ruling it reopens; D keep the adapter behind a role.
- `rec` A now means the declared request source. The draft's A is option C.
- Costs labeled measured or inferred; counts given with their method.
