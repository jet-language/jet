# #3077 — Browser action security on the real dispatch path

Date: 2026-09-29. Card #3077. Binary: `jet-debug-snapshot14`.

## Question

On the App action path (`web.app().action(name, handler)`), do CSRF, authentication, capability and input-limit checks run before the handler's mutation, for one native form action and one scripted server-function call?

## Blocker: no App program compiles on snapshot14

Probe: `~/.cache/jet-dev/scratch/Closer06/web/action_security.jet`. It defines two actions, `save(note: Note)` and `ping()`; each prints `MUTATION …` when invoked. It was run as a service with `JET_APP_PORT=47731` through `safe-jet.sh run`, and exited 1 with 14 errors, among them:

```
E0102 `<corelib>/Core/web::Core/web/web.jet::App` has no method `route`   (action_security.jet:24)
E0119 There's no type called `WebFormError`                                (Core/web/web.jet:81)
E0311/E0310 `len` on `String?`                                             (Core/web/forms.jet:225, :754)
E0202 write-access marker `&` needs a plain named binding (`&form.form`)   (Core/web/forms.jet:294, 308, 345, 346, 363, 377, 439, 450)
```

`safe-jet.sh check Examples/features/web/web_app.jet` fails the same way: `App has no method 'csr'`, plus the same Core/web errors. No request could be sent, so criteria 1, 2 and 4 have no runtime evidence.

## Source evidence (read, not executed)

Dispatch order in `crates/jet-codegen/src/Prelude/CoreLib/Top/WebServerFn.rs` `dispatch_checked` (:901-966) is:

method → endpoint → context → `check_live` → `check_csrf` (:920) → `authorize` (:921) → `normalize_input` (size and JSON validity, :922) → validator → middleware → handler (:947).

Every policy check therefore precedes the handler in the Rust layer.

Findings that the planned harness must confirm or refute:

| # | finding | locator | consequence |
|---|---|---|---|
| S1 | The App action path builds each action with `jet_web_server_fn_typed(...)` and **never** calls `with_capability` or `with_csrf` | `crates/jet-codegen/src/Prelude/App.rs:82-91`, `:115-124` | on App actions `spec.capability` is always `None`, so `authorize` is a no-op: no 401 or 403 is reachable. CSRF is the default `SameOrigin{expected_origin: None, token: None}` (`WebServerFn.rs:149-156`): only an Origin-vs-Host comparison, no token |
| S2 | The authentication subject is **any non-empty `Authorization` or `Cookie` header**; nothing validates it | `WebServerFn.rs:754-765` | "authenticated" means only that some credential header is present |
| S3 | Capabilities are read from the **client-supplied** `x-jet-capability` header | `WebServerFn.rs:766-777` | wherever a capability is configured, a client can grant it to itself. A security defect |
| S4 | An oversized body (> 4 MiB, `JET_WEB_SERVER_FN_MAX_BODY` :13) maps to `InvalidInput` → **400**, not 413 | `:837-841`, `:414` | the plan's expected 413 does not exist |
| S5 | Unknown and duplicate members differ between the UI (form) path and the wire (JSON) path. See the table below | `:1408-1427`, `:1463-1470` | inconsistent policy |
| S6 | The decode error text is `format!("{errors:?}")` of the decoder's field errors. CSRF and auth messages are fixed strings. The token, cookie and authorization values are never copied into the context (the subject becomes the literal `"authorization"` or `"cookie"`) | `App.rs:71-73`, `WebServerFn.rs:431-450`, `:754-765` | responses do not echo the CSRF token or session value by construction; unverified at runtime |
| S7 | No proxy trust at all: `Origin` and `Host` come from the raw request, and no `Forwarded`/`X-Forwarded-*` handling exists in WebServerFn.rs, App.rs, Core/web or Core/http | grep result | proxy trust is **unsupported**. Behind a TLS-terminating proxy the SameOrigin check compares against the proxy's `Host`. Route to #3305's owner gate |

## Criterion 3 — unknown and duplicate member policy (runtime evidence on the wire codec)

Probe: `~/.cache/jet-dev/scratch/Closer06/web/json_members.jet`, adapted from the retained CORE input `json-deny-unknown-initial.jet`. It decodes `{"count":2}`, `{"count":2,"extra":3}` and `{"count":2,"count":3}` into a `#[Codable, DenyUnknownFields]` struct and a plain `#Codable` struct. `jet run` and `jet run --interpret` gave identical output, rc=0:

```
strict valid: accepted 2
strict unknown: rejected
strict duplicate: rejected
lenient valid: accepted 2
lenient unknown: accepted 2
lenient duplicate: rejected
```

| input | wire (JSON body → typed decode) | UI form, untyped (`jet_web_server_fn_form_json`) | UI form, typed binding (`…_form_json_typed`) |
|---|---|---|---|
| unknown member | rejected under `DenyUnknownFields`, otherwise **ignored** (observed) | forwarded into the JSON object, then the same as wire (source) | **always rejected**: "form input contains unknown field" (source :1464-1468) |
| duplicate member | **rejected** in both modes (observed) | turned into a JSON **array** of values (source :1412-1423), then a type error or array acceptance depending on the field type | not visible in the lines read; **unknown** |

Verdict for criterion 3: the policy is **not consistent** across UI and wire (defect SEC-3).

## Defects

| id | repro | tiers | observed | expected |
|---|---|---|---|---|
| WEB-1 | `safe-jet.sh check Examples/features/web/web_app.jet` | all | App has no `csr`/`route`; `Core/web/web.jet:81` can't see `WebFormError`; `Core/web/forms.jet` fails E0202/E0310/E0311 (14 problems) | compiles and serves (the golden `web_app.harness.out`) |
| SEC-1 | App action with a declared capability | source | the App path cannot declare a capability or CSRF token (S1) | the declared policy reaches `JetWebServerFnSpec` |
| SEC-2 | any request with `x-jet-capability: <cap>` and any `Cookie` | source | the client grants itself the capability and counts as authenticated (S2, S3) | capabilities and subject derived from a verified session/principal |
| SEC-3 | `{"count":2,"count":3}` vs `count=2&count=3`; unknown field under a typed form | wire observed, form source | wire rejects duplicates while the form turns them into an array; the typed form rejects unknowns while wire JSON ignores them by default | one policy, as CorelibAstra requires |
| SEC-4 | a 5 MiB body | source | 400 `invalid_input` | 413 |

## Verdict

**BLOCKED** (WEB-1).

- Criteria 1, 2 and 4 need the running dispatch path. They are unmet, and the source findings S1–S3 show that 401/403 are unreachable on App actions even once the path compiles.
- Criterion 3 is investigated and **fails** on consistency (SEC-3).
- Criterion 5 needs a new `tests/web_app.rs` case built with cargo against a compiling App path. Not attempted, because WEB-1 makes it fail regardless.
