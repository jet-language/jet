# Adversarial review: D-RULE-REWRITE1, D-RULE-BODYFACTS1 and D-RULE-WRITTEN1, how the HTTP route checker leaves sema

Date: 2026-11-24
Reviewer: RewriteReview (Claude Opus, fresh agent)
Ballots reviewed:
- `$JET_SCRATCH_ROOT/tmp/orch/ballots/D-RULE-REWRITE1.json` (draft, author G1Generation-2; card #6851 c3 and c34, Tower card `c0e6znz4`). The author's draft held three questions in one ballot. Main ruled to split it (finding F1), so the reviewed text now lives in three files:
  - `D-RULE-REWRITE1.json`: Q1, the per-call value (piece 5), recommendation A;
  - `D-RULE-BODYFACTS1.json`: Q2, the handler-body facts (piece 6), recommendation A;
  - `D-RULE-WRITTEN1.json`: Q3, the literal as written (piece 2), recommendation A.
- The repaired three-question text before the split is kept as `$JET_SCRATCH_ROOT/tmp/orch/ballot/rewrite1/D-RULE-REWRITE1.three-question-r1.json`.

Compiler for probes: `$JET_SCRATCH_ROOT/tmp/orch/batch20-chain/s1/jet` (`jet a0.0.46-dev`, dev through the batch 20 landing).
Source read: dev worktree `Main-dev-collectall` at 6ab0b5a5c8.
Evidence: `$JET_SCRATCH_ROOT/tmp/orch/ballot/rewrite1/p*/main*.jet`, each with its `out*.txt`. Every probe ran alone on batch 20.

Verdict scale: **holds**, **holds within limits** (the limits are stated), **fails**.
F1-F19 are round 0 findings; R1-R5 are round 1 findings. Their state is in the last section.

## 0. Context the ballots depend on

- Card #6851 c3: "The Http.jet and HttpRouting.jet checkers in Sema/Calls move to Core declarations and are deleted; every diagnostic code they raised keeps its UI snapshot." Its proof is that `git ls-files` prints nothing for **both whole files**. So all of HTTP.jet (590 lines) is in scope, not only its `web.form` part.
- Card #6851 c34: a check rule tells an interpolated String literal from a run-time value "through a fact within D-CORE-CHECK1's CompilerCall surface … If no ratified surface can carry it, the shape goes on a ballot first." D-RULE-WRITTEN1 is that ballot.
- Card #6851 c22: rule dispatch at most 1 µs per heard call; 46.5 µs measured. c26: D-RULE-TYPEARGS1 rework. c30: `CompilerArg.function` / FunctionInfo for function arguments. c7: a user package gets the same checks.
- D-CORE-CHECK1=A: rules hear a `CompilerCall` (CoreRecordRows.jet:250-256: caller, callee, module, span, name, args, receiver) and refuse. `CompilerArg` (:259-263) is label, type, value, function, span. The text promises `function` as "a `FunctionInfo?` for a function or lambda argument". Its implementation decision M3 moves `#Pre` onto a check rule with a per-call self; not landed.
- D-META-LADDER1=A: "Jet has no macros: no call-site expansion is registered." D-META-RECONCILE1=A: written code means what its file says, and in standard code "no rewrite happens". D-JAI-CODE1, narrowed by D-META-COMPILER1=A, removed `#Expand` and `name!()` but keeps `c.nodes()`, "a typed, read-only tree with kinds, children, locations and, after checking, types" (syntax-decisions.md:4607-4623). Spellings.jet has no `c.nodes()` row, so it is not built.
- D-PRE-COMPILE1=A: a known argument is a literal, a constant or a comptime parameter. D-JAI-GENERIC1: `#Pre(#run …)` decides its predicate per call with the call's argument facts. D-SITE-CALLER1: `at: SourceLocation{#caller}` is computed per call and forwarded by a wrapper that declares the same default.
- D-CONC-CROSS1=A: sendability is a registered fact about a type, and one prover checks every crossing. HTTP handlers are one crossing kind (`SemaSendCrossing.HTTPHandler`, Crossings.jet:102).
- D-RULE-TYPEARGS1 (draft, reviewed): a **type's** arguments as TypeIds. It does not cover a **call's** explicit type arguments (`json<Order>()`).
- What sema does today:
  - HTTPRouting.jet (1,482 lines): router entry `sema_call_try_http_router` :1170-1414; literal check :466-475 with `sema_http_route_interpolated_source` :430-448; schema rendering :535-614; response walk :656-926 (constructor names :713-731, local type guesses :740-772); `json<T>()` search :928-1025; contract :1107-1168 (:1165 returns None only when no response is recognized); handler-shape E2805 :1247; handler wrap :1345 and `sema_http_mux_route_handler` :1452-1482; capture check :1348; E2805 contract :1387-1389; lowering :1391-1411; E2804 table :1416-1427.
  - HTTP.jet (590 lines): server JSON admission E2411 :74-93; middleware capture check :117-147; `app.middleware` lowering :149-176; `sema_call_http_mux_source_call` :185-213 (emits every lowered call and fills `#caller`); `web.form` lowering :274-437; typed form methods :439-590 (method renames to `typed_*` and the leading-dot selector `.email` lowered to the String "email").
  - Methods.jet: route registrations :1506; App `route`/`page`/`layout` and `middleware` :1954-2035.
  - Core/Web/Web.jet: App targets `__http_route`, `__http_route_dynamic`, `__http_route_custom` :496-512, each with `at: SourceLocation{#caller}`; `web.openapi`'s run-time contract check :4302.
- Probe results on batch 20:
  - p1: the rule hears `"/users/{id}"` as `value: None`; `"/plain"` as /plain; a constant as /cache; `TEAM: #run "/teams/{{name}}"` as /teams/{name}. `get(#run …)` written directly is `Jet.UnsupportedCompileTimeValue` ("`#run value` is used inside a larger value"). p1b: a local `#run` binding is heard.
  - p2: `.reflect()` on a function-typed parameter is `Jet.UnknownMethod`. p2b, p2c, p2g: `#Pre(#run …)` over a module constant or a forwarded parameter is `Jet.CompileTimeReadsRuntimeValue`, reported at the declaration, not the call.
  - p3: `#run` in a parameter default is `Jet.UnsupportedCompileTimeValue`. p3b: a plain default over an earlier parameter, and `#caller`, are computed per call.
  - p4: E2805 renders "Jet.InvalidRoute: Invalid HTTP route `/users/{id}`: brace interpolation is not allowed in route patterns" at 18:16. `print(web.openapi(router))` without `??` is `Jet.MissingErrorConversion`.
  - p5: responses returned through a helper and a body decoded in a helper are missing from the OpenAPI document; the build succeeds. Typed lambda and query parameters are present.
  - p6: a duplicate route is `Jet.AmbiguousRoute` (E2804). A handler with only helper paths gets E2805 "HTTP route handler has no complete checked endpoint contract", Fix "Return a typed `HTTPResponse` constructor from every handler path". A run-time router path gets "HTTPRouter route path must be a static String or Path".
  - p7: the same handler on an App compiles; `web.openapi` fails at run time (Web.jet:4302).
  - p8: `CompilerArg.function` is None for a named function and for a lambda.
  - p9: a non-bare hole renders as `/u/{…}`.
  - p10: a generic handler adapter needs one impl per input type (`impl Int.Input {…}`). p11: no overloading (`Jet.DuplicateDefinition`).
- Costs: 46.5 µs per heard call today (c22); a reflect host round trip is 43-229 µs (D-RULE-TYPEARGS1 review). So a handler with three nested types costs about 0.2-0.7 ms. At 500 handlers that is about 0.1-0.35 s of build time until c22 is met [INFERENCE, arithmetic from the measured numbers].
- Repo counts: at least 17 files with typed-first-parameter lambda handlers, 32 with `fn()` handlers and 10 with `web.form(` in Tests and Examples.

## 1. Strongest existing solutions, stated fairly

### Q1, a value computed while building, attached to each call

| Language | Mechanism | Best case | What it does that the draft omitted |
|---|---|---|---|
| C++20 | `std::source_location::current()` as a default argument is evaluated at each call site ([support.srcloc]) | A's shape: the callee declares, the call stays a call | Limited to the location; no reflection over other arguments. Source: ISO C++20 [support.srcloc.cons]. |
| Zig | `comptime` parameters and `anytype`; the callee's body runs at compile time per instantiation | The strongest "computed per call, no rewrite" form | Every instantiation is a new function; no stable ABI for the callee. Source: ziglang.org/documentation, "comptime". |
| Swift | SE-0382 expression macros, SE-0389 attached macros | B's shape, at its strongest: visible `#route(...)` spelling at the call | The call is visibly a macro; Jet's B rewrites a call that looks ordinary. Source: github.com/swiftlang/swift-evolution proposals 0382, 0389. |
| C# 12 | Interceptors: `[InterceptsLocation]` redirects one call site to another method | B exactly: an ordinary-looking call is replaced | Experimental and opt-in per namespace; a reader of the call cannot see the redirection [INFERENCE from the mechanism]. Source: github.com/dotnet/roslyn docs/features/interceptors.md. |
| Rust | Procedural macros over tokens | General rewriting | Needs `name!()` call syntax, which D-JAI-CODE1's narrowing removed from Jet. Source: doc.rust-lang.org/reference/procedural-macros.html. |
| Go | `encoding/json` special-cases `[]byte` inside the library | C's "bounded list the compiler owns" | A library-internal test, not a compiler privilege. Source: pkg.go.dev/encoding/json. |

The draft cited Swift and C# but not C++ default-argument evaluation or Zig, which are the precedents for A. It also did not say that C# interceptors are experimental.

### Q2, what a handler's body reads and returns

| Framework | Contract source | Source |
|---|---|---|
| FastAPI | Signature annotations, read at startup; the body is never read | fastapi.tiangolo.com, "Request Body", "Response Model" |
| Axum + utoipa | Extractor types in the signature; responses by return type or `#[utoipa::path(responses(...))]` | docs.rs/axum, docs.rs/utoipa |
| ASP.NET Core minimal APIs | `TypedResults` return types (`Results<Accepted<T>, BadRequest>`) give OpenAPI metadata; a plain `IResult` gives none | learn.microsoft.com, "Minimal APIs: responses" |
| Roslyn / Swift macros | Body syntax trees for analyzers and macros | Roslyn SyntaxTree API; swift-syntax |

No mainstream framework infers a contract from body data flow, as Jet does today. Signature contracts (B) are the industry norm; the draft's B was correct but understated that it is the only option where the contract shows at the declaration.

### Q3, the literal as written

| Language | Field | Source |
|---|---|---|
| Go vet printf | Checks a literal or constant format; since Go 1.24, `fmt.Printf(s)` with a non-constant `s` and no other arguments is reported | go.dev/doc/go1.24, "Vet" |
| Roslyn | `InterpolatedStringExpressionSyntax.Contents`: text parts and interpolations with spans | learn.microsoft.com, Microsoft.CodeAnalysis.CSharp.Syntax |
| swift-syntax | `StringLiteralExprSyntax.segments`: string and expression segments | github.com/swiftlang/swift-syntax |
| C# 10 | `[CallerArgumentExpression]`: another argument's source text as a String | learn.microsoft.com, "CallerArgumentExpression" |
| Rust | `stringify!` | doc.rust-lang.org/std/macro.stringify.html |

Roslyn and swift-syntax are A's shape. C# `CallerArgumentExpression` is the source-text option the draft lacked (E).

## 2. Claims and verdicts

### C1. Situation: "HTTPRouting.jet (about 1,500 lines) and the web-form part of HTTP.jet (702 lines)"

HTTPRouting.jet is 1,482 lines. HTTP.jet is 590 lines; its `web.form` lowering is :274-437, about 165. And c3 deletes **all** of HTTP.jet, which also holds middleware, the capture check, the mux emitter, the form methods and E2411 (R1).
**Verdict: fails.**

### C2. Situation: "the contract is built from the checked tree"

The walk reads the AST (`sema_http_route_find_json_*` :928-1025, `sema_http_route_response_*` :656-926) and guesses local types from syntax (:740-772).
**Verdict: fails.**

### C3. Detail: the inventory of sites

The draft named the lowering and the contract builder. It missed the handler wrap (:1345, :1452-1482), the schema rendering (:535-614), the E2804 cross-call table (:1416-1427, Methods.jet:1506), the App `route`/`page`/`layout` path (Methods.jet:1954-2035), the shared lookups, and in round 1 the HTTP.jet sites listed in §0 and the capture check (:1348).
**Verdict: fails** (F3, R1, R2).

### C4. Detail: "HTTPMount.jet has App variants of the router methods"

HTTPMount.jet mounts structs (`app.mount` with a struct, Methods.jet:1966); it has no router variants. It does call the shared capture check (HTTPMount.jet:1240), which lives in HTTP.jet.
**Verdict: fails.**

### C5. Precedent: "`#Pre(#run …)` already computes a value per call from the call's arguments"

It does so only for a literal argument. A module constant or a forwarded parameter is `CompileTimeReadsRuntimeValue` at the declaration (p2b, p2c, p2g), against D-PRE-COMPILE1=A, which counts a constant as known.
**Verdict: fails as stated; holds within limits** as "ratified to" (F6).

### C6. "A `#run` value reaches the rule through `value`"

Only when bound first (p1, p1b). Written directly as the argument, it is refused before any rule runs (p1).
**Verdict: holds within limits.**

### C7. Story: `print(web.openapi(router))`

`web.openapi` is fallible; the program fails with `MissingErrorConversion` (p4). The ballot program now uses `?? "openapi failed"`.
**Verdict: fails** (F10).

### C8. E2805 rendering of `/users/{id}`

Matches p4 and Tests/UI/http_route_invalid_literal.stderr exactly.
**Verdict: holds.**

### C9. "The rule hears `\"/users/{id}\"` as None, like a run-time value"

p1.
**Verdict: holds.**

### C10. "`CompilerArg.function` is None for a lambda"

p8 shows None for a named function too: c30 is not landed. D-CORE-CHECK1=A's text promises both.
**Verdict: holds** (the draft understated it; the ballot now says both).

### C11. Cost: "about 54 µs per heard call"

Card #6851 c22 records 46.5 µs. The ballot now cites c22 and the 43-229 µs reflect.
**Verdict: holds within limits.**

### C12. Q1 B: "a rule may replace its call"

B conflicts with D-META-LADDER1=A and D-META-RECONCILE1=A, which the draft did not say. B also cannot synthesize today's handler wrap.
**Verdict: fails as stated** (F5); the repaired B is at its strongest and names both limits.

### C13. Q1 A: "a per-call parameter removes the lowering"

A needs `#run` in a default (p3), `.reflect()` on a function parameter (p2), lambda FunctionInfo (p8, c30), nested TypeIds (c26), the body facts (Q2), the handler adaptation (p10, p11) and the capture check (R2). None is named in the draft except c26.
**Verdict: holds within limits** (six gates, all on #6851; F7, F8, R2).

### C14. Q1 C and D

C at its strongest was missing ("no comptime child", "Go's `[]byte`"). D's "errors move to run time" holds; p7 shows App routes already do that.
**Verdict: holds within limits.**

### C15. Q2: "a handler path that returns something else gives E2805"

E2805 fires only when **no** path is recognized (:1165, :1387; p6). Other paths are dropped silently (p5). The E2805 Fix text "from every handler path" overstates the rule (F13). An App route is never refused, and its `web.openapi` fails at run time (p7).
**Verdict: fails** (F12, F13, F14).

### C16. Q2 A: "a new `RecordedCall` record"

It duplicates CompilerCall (I8), and a call's explicit type arguments (`json<Order>`) reach no field today. Repaired: A reuses CompilerCall and adds `CompilerCall.type_args: [TypeId]`.
**Verdict: fails as drafted; holds within limits as repaired** (F15).

### C17. Q2 D: "the BodyTree layout becomes API"

D-JAI-CODE1 already ratifies `c.nodes()` as the tree view, and `#{f.body}` already names a body. D's cost is that the view is unbuilt, ships whole bodies to the comptime child and moves the walk into Core.
**Verdict: fails** (F17).

### C18. Q2: callees are matched correctly

The walk matches the leaf name (:713-731), so a user function named `json` counts.
**Verdict: fails** (F16, a defect today; A's matching by symbol fixes it).

### C19. Q3 D: "D-CORE-CHECK1 says rules hear values, not syntax"

D-CORE-CHECK1's text lists CompilerArg's fields; it states no such rule.
**Verdict: fails** (F18).

### C20. Q3 options

The draft lacked a source-text option (C# `CallerArgumentExpression`, Rust `stringify!`), and its A filled a field for every literal, which also exposes "plain literal versus constant".
**Verdict: fails** (F19). Repaired: E added; A fills only interpolated literals.

### C21. Format

Three questions in one ballot. Owner law (store.mjs:2779) and `Validate.jet` require one question per ballot with top-level `options`, `rec`, `recommendation` and an object `surface`; the situation was 1,958 characters (max 1,200).
**Verdict: fails** (F1).

### Checklist (AGENTS.md "Adversarial design review" and ballot-review.md)

| Item | State in the draft | State after round 1 |
|---|---|---|
| Every site of the mechanism named, one source of truth | Fails (C3, C4; F3) | Holds (R1, R2 fixed) |
| Siblings in the same store (App, mux, middleware, forms, E2804) | Fails (F3) | Holds; E2804, form selectors and the capture check named as kept or gated |
| Second store kept with numbers and reasons | Fails | Holds (E2804 table, handler adaptation, ~370-line walk under Q2 C) |
| I8 one mechanism | Fails (Q2 `RecordedCall`, F15) | Holds (CompilerCall reused) |
| I9 one meaning | Holds within limits (`#run` keeps its meaning in a default) | Holds within limits |
| Strongest prior art, sourced | Fails (no C++ default args, Zig, CallerArgumentExpression; interceptors not marked experimental) | Holds |
| Each option at its strongest | Fails (Q1 C, Q2 D, Q3 D; F17, F18) | Holds |
| Code in options compiles, or its prerequisite is named | Fails (story print, F10; A's gates, F7) | Holds within limits (gates named) |
| Ratified text quoted correctly | Fails (C12, C19) | Holds |
| Compile speed | Not stated | Holds within limits (0.2-0.7 ms per handler until c22) |
| Beginner path | Holds (calls unchanged under every A) | Holds |
| Expert path (package router) | Partly (forwarding unstated) | Holds (forwarding rule, `#Recorded` on own callees) |
| Tooling, diagnostics, docs | Fails (E2805 Fix text, stale `#Expand` rows; F13, F20) | Named on #6851 |
| Format passes the validator | Fails (F1) | Holds; only "draft:false" remains |

## 3. Findings (round 0)

- **F1 Format.** One ballot holds three questions; it cannot post (store.mjs:2779) or validate. Split into D-RULE-REWRITE1, D-RULE-BODYFACTS1 and D-RULE-WRITTEN1, each cross-referencing the others in `situation` and `hybrid`. Done on Main's ruling.
- **F2 Line counts.** 1,482 and 590 lines; the web-form lowering is :274-437.
- **F3 Inventory.** Name the handler wrap, schema rendering, E2804 table, App `route`/`page`/`layout`, shared lookups, and their fate under each option.
- **F4 Form selector lowering.** The typed form's leading-dot selector `.email` is lowered to "email" (HTTP.jet:566-590). Under every option it needs a per-model field type, which no ballot settles. (Found in round 1 with R1; numbered here because it is a gap, not a text fix.)
- **F5 B's ratified limits.** State that D-META-LADDER1=A and D-META-RECONCILE1=A rule out a call-site rewrite in standard code, and that B cannot synthesize the handler wrap.
- **F6 `#Pre(#run …)` precedent.** It works only for literals; a constant or a forwarded parameter fails at the declaration (p2b, p2c, p2g), against D-PRE-COMPILE1=A. The fix (constants known, errors at the call) is M3's.
- **F7 A's gates.** `#run` in a parameter default (p3), `.reflect()` on a function parameter (p2), lambda and qualified-name FunctionInfo (p8, c30), nested TypeIds (c26), body facts (Q2). Name each as a gate on #6851.
- **F8 Handler adaptation.** Jet has no overloading (p11), and a generic adapter needs an impl per input type (p10). So A does not remove `sema_http_mux_route_handler` until D-RULE-TYPEARGS1's typed handler params land.
- **F9 E2804 across calls.** The duplicate-route table needs state across calls, which no rule has. It stays in sema under every option, so c3's deletion needs a cross-call mechanism or a narrowed criterion.
- **F10 Story program.** `print(web.openapi(router))` does not compile (p4).
- **F11 "Checked tree".** The contract is built from the AST.
- **F12 Q2 E2805 claim.** E2805 fires only when no path is recognized; other paths are silently dropped (p5, p6).
- **F13 E2805 Fix text.** "Return a typed `HTTPResponse` constructor from every handler path" overstates a rule that needs one.
- **F14 Silent incomplete contracts.** Router contracts silently drop helper paths (p5); App routes are never refused and `web.openapi` fails at run time (p7, Web.jet:4302).
- **F15 Q2 A and I8.** Reuse CompilerCall; add `CompilerCall.type_args: [TypeId]`, which D-RULE-TYPEARGS1 does not cover.
- **F16 Name-based callee matching.** A user function named `json`, `response` or `empty_response` counts as Core's (:713-731, :928-1025).
- **F17 Q2 D at its strongest.** D is D-JAI-CODE1's ratified `c.nodes()` over `body: Code`, not a new BodyTree API.
- **F18 Q3 D's claim.** D-CORE-CHECK1 does not say "values, not syntax".
- **F19 Q3 options.** Add E (`text: String`, after C# `CallerArgumentExpression`); narrow A to interpolated literals only, one enum, no wrapper struct; A's fill site is `sema_rule_call_arg` (CheckRules.jet:268), which must take the expression.

Not a ballot finding, sent to #6851 with F20:
- **F20 Stale `#Expand` text.** Spellings.jet:48 (`#caller` "or inside an #Expand macro") and CoreSurface.jet:153-154 still name `#Expand`, which D-JAI-CODE1's narrowing removed.

## 4. Re-review log

- 2026-11-24, round 0: F1-F20 found. Main ruled the split for F1. I repaired the three-question text (kept as `D-RULE-REWRITE1.three-question-r1.json`) and then wrote the three ballots from it, per the owner ruling of 2026-11-24 that removable downsides are engineered away in the ballot before posting:
  - Q1 A keeps calls unchanged, completes the path half of the contract at registration so run-time App paths work, refuses a written contract except by forwarding (no forgery), caches per handler, and names its gates.
  - Q2 A reuses CompilerCall, matches callees by symbol, and keeps today's refusal rule.
  - Q3 A fills one enum only for interpolated literals, so plain literals and constants expose nothing and cost nothing.
- 2026-11-24, round 1 (my second pass over the three written ballots). New:
  - **R1 HTTP.jet scope.** c3's proof deletes all of HTTP.jet. Fixed: Q1's situation, C's costs (about 2,000 lines) and two new detail paragraphs give the middleware lowering, the mux emitter, the form methods and E2411 with their fates.
  - **R2 Capture check.** Every route handler gets the HTTP-handler capture check (HTTPRouting.jet:1348; HTTP.jet:117-147; also HTTPMount.jet:1240). Under A, B or D the handler becomes an ordinary Core parameter, and no Core declaration can name that crossing (D-CONC-CROSS1=A). Fixed: the sixth gate in Q1 A, its loss, the surface and the reviewPasses line.
  - **R3 Q2 gain overclaim.** "The body walk leaves sema" was false: the alias and return-path search stays as a generic digest. Fixed: "The HTTP-only walk leaves sema; a generic digest replaces it", and a technical paragraph saying so.
  - **R4 Go vet claim.** Go 1.24 reports a non-constant format only when it is the call's only argument. Fixed in Q3's comparison note and code.
  - **R5 Q2 marking scope.** Who may mark a callee was unstated. Fixed: only the module that declares a callee can mark it.
  - Re-attacks that held: Q1 A's forwarding through `kit.router` (the forwarded default reads the same handler); I9 (`#run` keeps one meaning, the default is one more place it is decided per call); Q3 A when the literal moves into a local (`written: None`; a run-time router path is refused anyway, p6, and an App path behaves as today); compile speed (0.2-0.7 ms per distinct handler, stated with c22).
- `Validate.jet` on batch 20 reports one gap for each of the three written ballots: "posting payload must set draft:false". Each `reviewPasses.adversarial` is byte-identical to §5.

## 5. Overall verdict

Round 0: **fails** (F1-F20). The direction is right in all three questions. But the draft could not post, misstated two ratified decisions, missed half the sites, overstated E2805, and hid A's gates.

Round 1 (final): **holds within limits**. Every claim holds or holds within these named limits:
- D-RULE-REWRITE1, A: six gates on #6851 (`#run` in a default and `.reflect()` on a function parameter; lambda FunctionInfo, c30; nested TypeIds, c26; D-RULE-BODYFACTS1; the handler adaptation, which waits on D-RULE-TYPEARGS1; a declared handler crossing). The E2804 table and the form selector lowering stay in sema under every option (F4, F9), so c3 cannot be met by these three ballots alone.
- D-RULE-BODYFACTS1, A: c30 for lambda handlers and c26 for nested type arguments. Helper-made responses stay invisible, as today (p5).
- D-RULE-WRITTEN1, A: no gate beyond the new field and enum; it keeps E2805's snapshot exactly.

The recommendations hold: A, A and A.

| Claim | Round 0 | Round 1 |
|---|---|---|
| C1 line counts and scope | fails | holds |
| C2 checked tree | fails | holds |
| C3 inventory | fails | holds |
| C4 HTTPMount | fails | holds |
| C5 `#Pre(#run)` precedent | fails | holds within limits (M3) |
| C6 `#run` through `value` | holds within limits | holds within limits |
| C7 story print | fails | holds |
| C8 E2805 rendering | holds | holds |
| C9 interpolated is None | holds | holds |
| C10 function None | holds | holds |
| C11 cost | holds within limits | holds within limits (c22) |
| C12 Q1 B | fails | holds |
| C13 Q1 A | holds within limits | holds within limits (six gates) |
| C14 Q1 C, D | holds within limits | holds |
| C15 Q2 E2805 | fails | holds |
| C16 Q2 A record | fails | holds within limits (c30, c26) |
| C17 Q2 D | fails | holds |
| C18 Q2 callee matching | fails | holds (A fixes it; F16 on #6851 today) |
| C19 Q3 D | fails | holds |
| C20 Q3 options | fails | holds |
| C21 format | fails | holds (draft:false left) |

Proposed `reviewPasses.adversarial` lines, which pass `Validate.jet` with only the draft gap left:

> D-RULE-REWRITE1: Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: RewriteReview. The adversarial review attacked the recommendation. Probes on batch 20 showed B conflicts with D-META-LADDER1 and D-META-RECONCILE1, and A holds within six named gates, including the handler adaptation and capture check the draft omitted.

> D-RULE-BODYFACTS1: Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: RewriteReview. The adversarial review attacked the recommendation. Probes on batch 20 showed E2805 fires only when no path is recognized, and A now reuses CompilerCall, adds `type_args` and holds within c30 and D-RULE-TYPEARGS1.

> D-RULE-WRITTEN1: Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: RewriteReview. The adversarial review attacked the recommendation. Probes on batch 20 confirmed only A and C keep E2805's snapshot, and A now fills one enum only for interpolated literals, beside a new source-text option E.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F2 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F3 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F4 | card | #6851 |
| F5 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F6 | card | #6851 |
| F7 | card | #6851 |
| F8 | card | #6851 |
| F9 | card | #6851 |
| F10 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F11 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F12 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F13 | card | #6851 |
| F14 | card | #6851 |
| F15 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F16 | card | #6851 |
| F17 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F18 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F19 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| F20 | card | #6851 |
| R1 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R2 | card | #6851 |
| R3 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R4 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
| R5 | no-action | archived: fixed in the ballot text before posting (re-review rounds 0-1) |
<!-- /audit-dispositions -->
