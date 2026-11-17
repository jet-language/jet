# Web framework parity audit (2026-11-15)

This audit compares Jet's web framework with the reference frameworks one
element at a time. Each element is judged against the framework that does it
best, not against one framework. For each element it records:

- the best framework, and what it does at its best;
- what Jet has today, cited by code path, executable example and test;
- the gap, and the card that owns it;
- each known downside of the best implementation, and the Jet design that
  removes it.

It answers #6552 criterion 1. The owner choices it raises are listed in
section 4, for the ballots of criterion 2. The gaps with no card are listed
in section 5, for the child cards of the card's method step 3.

## 1. How to read it

**Scope.** The reference set is the one #6552 names: TanStack (Start, Router,
Query, Form, Table, Virtual, Store, DB, devtools), Next.js, Remix and React
Router, SvelteKit, SolidStart, Qwik, Astro, Nuxt, Angular, Phoenix LiveView,
Rails (Hotwire), Laravel (Livewire), Django, Leptos and Dioxus, htmx, and
Elysia and Hono. Section 3 maps each framework's signature elements to the
rows of section 2, so every element of every named framework has a row.

**What "Jet today" means.** It is what runs on dev at 1b65b28007, cited by
file. A design that exists only in a proposal is not "today". The proposal
that holds most of the designs is
`Docs/proposals/2026-11-13-web-app-framework-shape.md` (the shape document
below). Its section 8 compares the proposed shape with the strongest systems;
this audit compares what is built.

**Verdict words.**

- **at parity:** Jet does what the best framework does, at about the same cost
  to the developer, and an executable example shows it;
- **partial:** Jet has the element, but the best framework does more;
- **behind:** Jet has a small part of the element or none of it;
- **not adopted:** the element conflicts with a ratified decision, so Jet
  removes the need for it instead of copying it. The row names the decision.

No row says Jet is better. A "better" claim needs a passing adversarial review
in `Docs/audits` (#6552 criterion 6, AGENTS.md "Adversarial design review").
Where a Jet design removes a downside, the row says "design removes it" and
names where the design lives and whether it is built.

**Sources.** Section 6 lists every outside source. Each link answered
HTTP 200 on 2026-11-15. A bracketed key such as [TSR] points there.

## 2. Elements

### 2.1 Routing: typed paths, search params and links

- **Best:** TanStack Router. Path params, search params and links are typed
  end to end; a link to an unknown route or with a wrong param type fails the
  TypeScript check; search params are validated and serialized by a schema
  [TSR] [TSR-SEARCH].
- **Jet today:**
  - Server routes on `web.app()` check their path fields at compile time. A
    handler input must exist for each path field (`tests/ui/http_route_missing_parameter.jet`),
    and two routes that match the same requests are E2804 with both spans
    (`tests/ui/http_route_ambiguous_overlap.jet`). A path value that does
    not parse as the declared type is 400 before the handler
    (`Examples/Apps/Web/JokesAPI.jet`, `tests/run/Net/RouteOverlap.jet`).
  - The client router `core.web.router` (`Core/Web/Router.jet`) declares path
    and search fields with value types, and decodes them apart
    (`tests/cases/web_app_graph/router_path_and_search_plus_decoding_are_distinct.case`).
    Its fields are runtime values (`WebRouterField`), and `link` takes a
    route name and `[String:String]` maps, so a wrong link is a
    `WebRouterError` at run time, not a compile error.
- **Verdict:** partial. Server routes are checked; client links are not.
- **Gap:** compile-time checked links and navigation with typed params
  (#6864 c1).
- **Downsides of the best, and the Jet design:**
  - *The route tree is generated code that must be regenerated, and its
    types slow the editor on large apps.* Design: routes are declarations the
    compiler reads directly; there is no generated tree (shape document 4.4).
    Not built.
  - *Search-param schemas are a second validation layer beside the server's.*
    Design: one struct with its `validate` block decodes the search params,
    the form and the JSON body (D-VALIDATE1=A; shape document 4.3). Built for
    JSON and forms; not yet for client search params.
  - *Browser routes and server routes are two tables.* Design: one route
    table (D-ROUTE-ONE1=A). Built on the server (`Core/Web/Web.jet`, the
    `App` builder); the client router is still a second table.

### 2.2 Data loading: loaders, waterfalls, preloading

- **Best:** React Router and Remix loaders run in parallel for nested routes
  [RR-LOAD]; TanStack Router preloads on intent and caches loader data
  [TSR-LOAD]; SvelteKit `load` runs on server or client [SK-LOAD].
- **Jet today:** a route takes a `.loader(...)`, an `.error(...)` boundary,
  and the app a `.not_found(...)` page (`Examples/Apps/Web/JetWeb/run.jet`).
  `__app_page_response` (`Core/Web/Web.jet:824`) runs the loader, then the
  handler with its data, and renders the error boundary on failure. The client
  router has `preload`, `abort`, `stale`, `invalidate` and `collect`
  (`Core/Web/Router.jet:107-141`).
- **Verdict:** partial. One loader per route, server side; no nested
  parallel loaders; pending boundaries are not rendered.
- **Gap:** pending, error and not-found boundaries on every tier, and a
  canceled preload on aborted navigation (#6864 c2).
- **Downsides of the best, and the Jet design:**
  - *Waterfalls between nested loaders when a child needs the parent's
    data.* Design: a screen's reads are known to the compiler, so the server
    starts them together (shape document 4.6). Not built.
  - *Loader data is untyped JSON across the wire.* Jet's loader data is the
    route's typed value encoded by the type's codec; the typed client is
    #6865.

### 2.3 Caching and invalidation

- **Best:** TanStack Query: keyed cache, stale time, background refetch,
  retries, offline pause, and invalidation by key prefix [TSQ]. Convex
  replaces manual invalidation with reactive queries [CVX-RT].
- **Jet today:**
  - `core.web.query` (`Core/Web/Query.jet`) has keyed queries with stale
    state, `refresh`, `cancel`, `retry`, an offline `queue`, network modes,
    and `mutate_with_invalidations`
    (`Examples/Apps/Web/QueryLifecycle.jet`). The data is JSON text
    (`get` returns `String`), and keys are strings.
  - `app.live` binds a source to its argument list and re-runs it on
    `app.invalidate`; `app.push` publishes a value
    (`Core/App/App.jet:436-462`, `Examples/Tooling/DevLoop/AppLive.jet`).
    A live result past `LIVE_MAX_PAYLOAD` is not stored.
- **Verdict:** partial. The lifecycle is there; types and automatic
  invalidation are not.
- **Gap:** one owning module for live data (#3709 c8); live queries from
  real write deltas, shared results and caps (#6868); route data freshness
  (#6864 c3); a server response cache (#6814 c3-c5).
- **Downsides of the best, and the Jet design:**
  - *Cache invalidation by hand: a forgotten key is a stale screen.* Design:
    the compiler derives the read set of a query and the write set of a
    method, and a write marks stale the reads it meets (D-INVALIDATE1=A,
    #6554 paused). Not built.
  - *Untyped keys collide.* Design: the key is the endpoint and its typed
    arguments (`live_key` in `Core/App/App.jet` already keys by source and
    encoded arguments).
  - *Falsy values read as misses in some caches.* The #6814 c3 case fixes
    the rule for Jet's cache before it is built.

### 2.4 Mutations and optimistic updates

- **Best:** TanStack Query `onMutate` with rollback [TSQ-OPT]; React
  `useOptimistic` [REACT-OPT]; Zero and Replicache run the mutator on the
  client and confirm on the server [ZERO] [REP].
- **Jet today:** `core.web.query` `mutate` tracks mutation state;
  `core.web.store` has `optimistic`, `patch`, `patch_commit` and
  `patch_rollback` (`Core/Web/Store.jet:135-160`,
  `Examples/Apps/Web/StoreHistory.jet`).
- **Verdict:** partial. Prediction is written by hand, as in TanStack Query.
- **Gap:** prediction from the server method (#6555, owner-paused); the
  offline write queue with request keys and row versions (#6867).
- **Downsides of the best, and the Jet design:**
  - *The optimistic update is a second copy of the server logic, and the
    two drift.* Design: the client runs the server method's own body minus
    what needs the server (D-OPTIMISTIC1; shape document 4.6, claim CL4).
    Paused with #6555.
  - *A retried write runs twice.* Design: every queued write carries a
    request key the server remembers (#6867 c2).

### 2.5 Forms and validation

- **Best:** TanStack Form for typed field state, async validation and
  submission state [TSF]; SvelteKit form actions work without script
  [SK-ACT]; Laravel and Rails validate on the server with field errors
  [LAR-VAL] [RAILS-VAL]; Angular typed reactive forms [NG-FORMS].
- **Jet today:** `core.web.forms` (`Core/Web/Forms.jet`) has typed forms
  over a struct, validation on change, blur or submit, async validators,
  no-script HTML, a submission lifecycle and action field errors
  (`Examples/Apps/Web/FormsTable.jet`, `Examples/Apps/Web/AppFieldErrors.jet`).
  A struct's `validate` block runs on decode (`Examples/Data/Validate.jet`).
  A server field error names a checked field path
  (`tests/cases/web_app/field_error_path_is_checked.case`).
- **Verdict:** partial.
- **Gap:** `kit.Form<T>` with one checked field per member (#6821 c1); one
  rule set for decode, `T.validate` and submit with the same paths
  (#6821 c3); the lifecycle with goldens on every tier (#6821 c4);
  database-aware field errors (#6831).
- **Downsides of the best, and the Jet design:**
  - *Validation is written twice, once for the client and once for the
    server.* Design: one `validate` block on the type is the only rule set
    (D-VALIDATE1=A). Built for decode; the form half is #6821 c3.
  - *Field names are strings, so a renamed field breaks silently.* Design:
    fields are members of `T`, checked by the compiler (#6821 c1).
  - *No-script forms and script forms behave differently.* Design: one
    submission lifecycle for both (#6821 c4).

### 2.6 Tables

- **Best:** TanStack Table: headless, typed columns, sorting, filtering,
  pagination, selection, server-side data [TST].
- **Jet today:** `core.web.table` (`Core/Web/Table.jet`) is headless with
  sorting, filtering, paging, selection, focus, row edits and server pages.
  A column literal is checked against the row type's fields by the compiler
  (the marker described at the top of `Core/Web/Table.jet`)
  (`Examples/Apps/Web/FormsTable.jet`).
- **Verdict:** partial.
- **Gap:** goldens on O0, O1, O2 and web, one state for local and server data
  (#6821 c5).
- **Downsides of the best, and the Jet design:**
  - *Column accessors are typed but column IDs are strings in state, filters
    and sorting.* Jet checks the column name against the row type at compile
    time. Built.
  - *A large feature surface pulled into every bundle.* Design: the web build
    links only what is reached (`tests/cases/web_build/web_wasm_int_export_reachability_golden.case`).

### 2.7 Virtualization

- **Best:** TanStack Virtual: windowing with measured, dynamic sizes and
  scroll-to [TSV].
- **Jet today:** `core.web.virtual` (`Core/Web/Virtual.jet`) plans a bounded
  window over fixed or measured sizes, with overscan, `plan_scroll_to` and
  resize; a 100,000-row list yields only its visible range.
- **Verdict:** partial.
- **Gap:** the virtual list as a view with goldens on every tier
  (#6821 c5).
- **Downsides of the best, and the Jet design:**
  - *Overflow in size arithmetic on very long lists.* Jet's plan uses
    saturating arithmetic (`sat_add`, `sat_mul` in `Core/Web/Virtual.jet`).
    Built.

### 2.8 State stores

- **Best:** TanStack Store and the Redux family: a typed store with
  selectors, derived values and devtools time travel [TSS].
- **Jet today:** `core.web.store` (`Core/Web/Store.jet`) has transactions,
  batches, patches with rollback, history with back, forward, jump and
  scrub, selector subscriptions, derived values and `inspect`
  (`Examples/Apps/Web/StoreHistory.jet`, `Examples/Apps/Web/StateSurface.jet`).
- **Verdict:** at parity on the store's own features.
- **Gap:** none on the store. Its devtools panel is 2.24.
- **Downsides of the best, and the Jet design:**
  - *A store beside the framework's reactive state makes two sources of
    truth.* Design: plain data updates the screen (D-REACT-REF1, #6553
    paused). Not built.

### 2.9 Reactivity

- **Best:** fine-grained signals: SolidJS [SOLID-SIG], Angular signals
  [NG-SIG], Leptos [LEP], Svelte runes.
- **Jet today:** `core.reactive` (`Core/Reactive/Reactive.jet`) has signals,
  computed values and effects with versions; `ui.reactive_render` repaints on
  change (`Examples/Apps/UI/Reactive.jet`, `Examples/Apps/Web/UIWebReactive.jet`,
  `tests/cases/web_build/web_reactive_render_replaces_stale_dependencies.case`).
- **Verdict:** partial.
- **Gap:** batched, glitch-free updates with disposal and no version-cap
  loss (#6552 c37).
- **Downsides of the best, and the Jet design:**
  - *Signals are a second way to hold data, and reading one outside a
    tracking scope silently loses updates.* Design: D-REACT-REF1 (#6553,
    paused).

### 2.10 Server rendering

- **Best:** Next.js and React Server Components [REACT-RSC]; SvelteKit
  [SK-LOAD]; Angular SSR [NG-SSR].
- **Jet today:** every page is rendered on the server per request
  (`Core/Web/Web.jet:824`); `jet run` serves pages, actions and assets
  (`tests/cases/web_app/web_app_run_serves_pages_actions_and_assets.case`).
- **Verdict:** partial. HTML pages render; there is no component model that
  renders on the server and continues on the client.
- **Gap:** server-rendered screens of the screen graph (#6445, shape
  document 3.3).
- **Downsides of the best, and the Jet design:**
  - *Server and client components have separate rules that are easy to
    break ("use client" boundaries).* Design: one screen language; the
    compiler places code by what it uses (shape document 1, "What runs
    where"). Not built.
  - *Secrets leak through server component props.* Design: secrets never
    reach a client path (D-SECRETEDGE1, #6557 paused; claim CL5).

### 2.11 Static generation and incremental regeneration

- **Best:** Next.js ISR [NEXT-ISR]; Astro static output; SvelteKit
  prerendering.
- **Jet today:** `.ssg()` records a mode in the app graph
  (`Core/Web/Web.jet:452`, `__app_mode`); no build writes static pages.
- **Verdict:** behind.
- **Gap:** no card. Section 5, row G1.
- **Downsides of the best, and the Jet design:**
  - *ISR serves stale pages with time-based revalidation tuned by hand.*
    No Jet design yet; the owner choice is section 4, row O1.

### 2.12 Streaming

- **Best:** React streaming with Suspense [REACT-STREAM] [REACT-SUSPENSE];
  Leptos SSR modes [LEP-SSR].
- **Jet today:** `.stream()` records a mode (`Core/Web/Web.jet:454`). No
  response streams regions.
- **Verdict:** behind.
- **Gap:** #6812 (waits on D-STREAM-REGION1).
- **Downsides of the best, and the Jet design:**
  - *A Suspense boundary is placed by hand, and a missing one blocks the
    whole page.* Design: each screen's `Loading` branch is the region
    (shape document 5.2.2). Not built.

### 2.13 Islands and server islands

- **Best:** Astro islands and server islands [ASTRO-ISL] [ASTRO-SISL].
- **Jet today:** `.island()` records a mode and the graph keeps it
  (`tests/cases/web_app_graph/render_modes_mount_and_island.case`); a
  wasm-built UI mounts into a server page
  (`Examples/Apps/Web/UIWebMount.jet`).
- **Verdict:** partial: mounting is built, partial hydration is not.
- **Gap:** no card. Section 5, row G2; the owner choice is O2.
- **Downsides of the best, and the Jet design:**
  - *Islands cannot share state without a separate store library.* Jet's
    `core.web.store` and `core.reactive` are one runtime for every mounted
    tree. Built for wasm-mounted trees.

### 2.14 Resumability and hydration

- **Best:** Qwik resumes from serialized state with no hydration [QWIK];
  Angular incremental hydration [NG-HYD].
- **Jet today:** `.hydration_dev()` and `.hydration_release()` record a
  setting (`Core/Web/Web.jet:460-470`). No hydration or resume runs.
- **Verdict:** behind.
- **Gap:** D-RESUME1 (#6556, owner-paused).
- **Downsides of the best, and the Jet design:**
  - *Hydration cost and mismatch bugs.* Design: resume from server state
    (D-RESUME1). Paused; #6556 must measure client bytes and time to
    interactive against Qwik and LiveView before any claim.
  - *Qwik's lazy closures fetch code on first interaction.* Same measure.

### 2.15 Client-side apps on WebAssembly

- **Best:** Leptos and Dioxus compile Rust to wasm with fine-grained
  updates [LEP] [DIOXUS-DOCS]; every JavaScript framework for plain CSR.
- **Jet today:** `jet build --target web` writes `rt.wasm`, `app.wasm`,
  `app.js` and `jet_dom_runtime.js`; a UI tree mounts into `#jet-app` and
  handles clicks (`Examples/Apps/Web/UIWebClick.jet`,
  `tests/cases/web_build/web_click_counter_dom_roundtrip.case`); 109 cases
  under `tests/cases/web_build`.
- **Verdict:** partial.
- **Gap:** the known-failing web rows (#5201 c1), tasks on the web target
  (#5201 c7), generated rather than hand-written runtime JavaScript
  (#5201 c5). Two findings from this audit: `core.browser.host.on` has no
  implementation on the wasm build (`Core/Browser/Host.jet`), and
  `core.browser.storage` is not an origin store on the web
  (`Core/Browser/Storage/Local.jet`).
- **Downsides of the best, and the Jet design:**
  - *Large wasm binaries.* The web build links only what is reached. The
    client byte budget is #6556's measure.

### 2.16 Server functions and actions

- **Best:** TanStack Start server functions [TSS-SF]; Next.js `"use server"`
  [NEXT-USE-SERVER]; SolidStart `"use server"` [SOLID-SERVER]; SvelteKit
  remote functions [SK-RF]; Leptos server functions [LEP-SF]; tRPC [TRPC].
- **Jet today:** `.action(name, fn)` registers `POST /actions/<name>` with
  a checked wire contract
  (`tests/cases/web_app_graph/server_function_action_exposes_checked_wire_contract.case`).
  `app.remote(service)` gives a client that has only the marked methods
  (`Core/App/App.jet:236`,
  `tests/cases/web_app/remote_has_only_marked_methods.case`); markers
  `#App.SignedIn` and `#App.Public` decide exposure, and a method's rate
  limit answers 429 (`tests/cases/web_app/method_rate_limit_answers_429.case`).
- **Verdict:** partial.
- **Gap:** a screen calls a marked method like a function and gets a typed
  error (#6552 c36); typed endpoints on every tier (#6865 c1-c3).
- **Downsides of the best, and the Jet design:**
  - *Every exported server function is a public endpoint, often by
    accident.* Design: no method is reachable unless marked (claim CL1).
    Built for `app.remote`.
  - *Errors cross as untyped exceptions.* Design: `app.CallError` cases are
    typed (claim CL7, `Core/App/App.jet`). Built as a type; the screen-side
    call is #6552 c36.

### 2.17 HTTP server, middleware and API routes

- **Best:** Hono and Elysia: small, fast routers with middleware, validated
  inputs and an end-to-end typed client [HONO] [HONO-RPC] [ELYSIA-VAL]
  [ELYSIA-EDEN].
- **Jet today:** `core.http.server` (`Core/Http/Server.jet`) has
  middleware layers, CORS, request IDs, limits, trailers and ETag on files
  (`Examples/Network/HTTP/HTTPServerMiddleware.jet`,
  `Examples/Network/HTTP/HTTPServerLimits.jet`). Route markers on methods
  mount with one call (`Examples/Apps/Web/JokesAPI.jet`). `web.openapi`
  derives an OpenAPI document from the route table, including a request
  body read under a decode fallback
  (`tests/cases/http/openapi_request_body_survives_a_decode_fallback.case`).
- **Verdict:** partial.
- **Gap:** auth requirements, failures and statuses in the OpenAPI document
  (#6865 c4); action security policy before any mutation (#6865 c6).
- **Downsides of the best, and the Jet design:**
  - *The typed client depends on exporting the server's type to the client
    bundle.* Design: the typed client is generated from the checked route
    contract (#6865 c2).
  - *Validation is a plugin, so a route can skip it.* Every decoder runs the
    type's `validate` block (claim CL2). Built.

### 2.18 Real-time and live views

- **Best:** Phoenix LiveView for server-driven views with diffs over a
  socket, and Presence [PHX-LV] [PHX-PRES]; Laravel Livewire [LW]; Convex
  reactive queries [CVX-RT]; Turbo Streams [TURBO-STREAMS].
- **Jet today:** WebSockets in `core.net.ws` (`Core/Net/Ws.jet`,
  `Examples/Network/WSEcho.jet`); `app.live` with push and invalidate
  (2.3); replica-aware CRDTs in `core.sync` (`Core/Sync/Sync.jet`,
  `Examples/Concurrency/SyncCRDT.jet`, `tests/cases/sync_crdt`).
- **Verdict:** behind.
- **Gap:** live queries over real deltas and across servers (#6868); the
  sync engine (#3709); presence (#6868 c7).
- **Downsides of the best, and the Jet design:**
  - *LiveView and Livewire keep a server process per open page and stop
    working offline.* Not adopted: D-WEBAPP-SHAPE1=A runs screens on the
    client, with live queries for server data and an offline queue
    (shape document 4.8, 4.9).
  - *Convex invalidates per table by default.* Design: SQL-narrowed read
    sets with a table-level fallback, shown by `jet inspect` (#6868 c5).

### 2.19 Auth, sessions and abuse limits

- **Best:** Laravel Fortify and Rails' secure defaults for accounts,
  sessions and CSRF [FORTIFY] [RAILS-SEC]; Django auth [DJ-AUTH].
- **Jet today:** `core.auth` (`Core/Auth/Auth.jet`) has strict JWT (HS256)
  and PASETO v4.public verification, password login, sessions, magic links
  and OAuth begin and finish (`Examples/Crypto/AuthSessions.jet`,
  `Examples/Crypto/PasswordAuth.jet`). Method rate limits answer 429 (2.16).
  `web.auth` in `Core/Web/Web.jet:2569` is a descriptor that only names
  routes.
- **Verdict:** partial.
- **Gap:** the account lifecycle and mail (#6826); native sessions with
  keychain refresh tokens (#6818); auth in the route contract (#6865 c4).
- **Downsides of the best, and the Jet design:**
  - *Generated auth code (`phx.gen.auth`, Fortify views) is copied into the
    app and drifts from upstream fixes.* Design: the account flows are part
    of the framework and join the route set (#6826 c2).
  - *CSRF protection can be switched off per route.* Design: an action's
    security policy runs before any mutation (#6865 c6).

### 2.20 Internationalization

- **Best:** Angular i18n with extraction [NG-I18N]; Django translation
  [DJ-I18N]; FormatJS ICU messages [FORMATJS].
- **Jet today:** none beyond English helpers such as `plural` in
  `Core/Text/Fmt.jet`.
- **Verdict:** behind.
- **Gap:** catalogs, plurals, locale formats and right-to-left (#6808);
  translatable view text (#6886).
- **Downsides of the best, and the Jet design:**
  - *Message IDs are strings, so a missing translation shows at run time.*
    Design: view literals are the messages, checked at compile time
    (D-I18N1=A; #6886 c1, c2).

### 2.21 Images and assets

- **Best:** Next.js `Image` [NEXT-IMG]; Astro images [ASTRO-IMG]; Vite's
  hashed assets [VITE].
- **Jet today:** `web.image` turns a source into a `<picture>` of hashed
  variants with the original as the fallback; every image URL answers with
  `public, max-age=31536000, immutable`
  (the Images section of `Core/Web/Web.jet` from line 883, `Examples/Apps/Web/AppImages.jet`,
  `tests/cases/web_app/image_pages_serve_hashed_immutable_variants.case`).
  Other static files are served from `static_files` with ETag
  revalidation, not hashed names (`Core/Web/Web.jet:425`,
  `Core/Http/Server.jet:1973-2004`).
- **Verdict:** at parity for PNG and JPEG images; behind for other assets.
- **Gap:** Jet WebP (#4695 c5, c6); build reuse of variants (#4695 c3);
  hashed non-image assets (#6816, #6814 c2).
- **Downsides of the best, and the Jet design:**
  - *Image optimization depends on a native library (sharp, libvips).*
    Design: a memory-safe Jet encoder (D-IMAGE-ENCODE1=A, #4695 c5).
  - *A changed encoder serves old bytes under the same URL.* Jet's variant
    key includes the encoder version (`C_IMAGE_ENCODER`). Built.

### 2.22 Styling

- **Best:** Tailwind for utility styling [TW]; vanilla-extract and StyleX
  for typed, zero-runtime styles [VE] [STYLEX]; Next.js CSS options
  [NEXT-CSS].
- **Jet today:** typed styles on `core.ui` views
  (`Examples/Apps/UI/UITypedStyle.jet`, `tests/cases/ui_lint_style`); web
  pages use plain CSS files (`Examples/Apps/Web/Public/App.css`).
- **Verdict:** behind.
- **Gap:** the styling system (#6817).
- **Downsides of the best, and the Jet design:**
  - *Utility class strings are unchecked; a typo is a silent no-op.*
    Design: typed styles with dead-rule and contrast checks (#6817 c1, c2).
  - *Dark mode values can be left out.* Design: a token without a dark
    value does not compile (claim CL9).

### 2.23 Testing

- **Best:** Playwright for browser tests with UI mode and traces
  [PW-UI] [PW-TRACE]; Phoenix LiveViewTest in process [PHX-TEST].
- **Jet today:** `core.testing` (`Core/Testing/Testing.jet`) has goldens,
  snapshots, fake clocks and history tests; `jet test` runs corpora and
  `#Test`. `core.browser` drives Chromium and Firefox through WebDriver BiDi
  and CDP (`Core/Browser/Browser.jet`, `Examples/Network/Browser/BrowserAgent.jet`,
  `tests/cases/browser_bidi`).
- **Verdict:** partial.
- **Gap:** the browser test scaffold, UI mode, retry traces and cross-browser
  runs (#6866).
- **Downsides of the best, and the Jet design:**
  - *Browser tests are a separate tool with its own config, and failures
    lose source positions.* Design: browser tests run in `jet test` and keep
    Jet's diagnostics (#6866 c4).

### 2.24 Devtools

- **Best:** TanStack Query and Router devtools [TSQ-DEV] [TSR-DEV]; React
  DevTools.
- **Jet today:** `core.devtools` publishes typed inspector events
  (`Core/Devtools/Devtools.jet`); `core.web.store` `inspect` and
  `core.web.query` `facts` expose state; the web graph is explained by
  `jet expand` and `jet inspect choices`
  (`tests/cases/web_app_graph/expand_web_lens_and_explain_web_graph_json.case`,
  `tests/cases/web_app_graph/explain_routes_lists_every_row.case`,
  `tests/cases/web_app/inspect_choices_lists_app_endpoint_markers.case`).
- **Verdict:** behind: the facts exist, no panel shows them.
- **Gap:** no card. Section 5, row G3; owner choice O4.
- **Downsides of the best, and the Jet design:**
  - *Devtools are a dependency shipped and then stripped by the bundler.*
    The facts are compiler output, so a release build carries none.

### 2.25 Error boundaries

- **Best:** Next.js `error.js` and React Router `ErrorBoundary`
  [NEXT-ERR] [RR-ERR]; SvelteKit errors [SK-ERR].
- **Jet today:** a route's `.error(...)` boundary and the app's
  `.not_found(...)` page (`Core/Web/Web.jet`, `__app_boundary`;
  `Examples/Apps/Web/JetWeb/run.jet`); `Loadable` carries a typed failure
  (`Core/Reactive/Loadable.jet`, `Examples/Apps/UI/Loadable.jet`).
- **Verdict:** partial.
- **Gap:** boundaries on every tier (#6864 c2).
- **Downsides of the best, and the Jet design:**
  - *An error boundary catches untyped exceptions, so the boundary cannot
    tell a network failure from a bug.* Design: network failures are typed
    cases the code must handle (claim CL7).

### 2.26 Deployment and edge targets

- **Best:** SvelteKit adapters [SK-ADAPT]; Next.js deployment targets
  [NEXT-DEPLOY]; Hono runs on every edge runtime [HONO].
- **Jet today:** the server is a native program (`jet build`); `.adapter()`
  records a name (`Core/Web/Web.jet:508`) and selects nothing; the web
  build's edge target is a build fact
  (`tests/cases/web_build/web_edge_target_is_build_fact_not_runtime_probe.case`).
  A WASI server build is not built yet (`tests/known-failing.tsv`, the
  `wasip2` rows).
- **Verdict:** behind.
- **Gap:** WASI and edge server outputs (#5201 c1); no card for deploy
  adapters. Section 5, row G4; owner choice O5.
- **Downsides of the best, and the Jet design:**
  - *Framework lock-in to one host.* Design: one native binary per server
    and a static web output; no host-specific runtime is required.

### 2.27 The dev loop

- **Best:** Vite's HMR keeps component state across edits [VITE-HMR].
- **Jet today:** `jet dev` serves the app with live reload: each run
  answers `/__jet/reload` with its boot id and the page reloads on a new id
  (`Core/Web/Web.jet:775-812`); `jet run` prints a ready line and refuses a
  busy port with E2809
  (`tests/cases/web_app/web_app_ready_line_and_busy_port_on_run_and_dev.case`);
  30 cases under `tests/cases/web_dev`.
- **Verdict:** partial: full reloads lose page state.
- **Gap:** the jet dev browser host (G5, #5931); timing against Vite, Bun,
  Next.js, Deno, Astro, Rsbuild and Parcel (G11, #2858; #6552 c31).
- **Downsides of the best, and the Jet design:**
  - *HMR boundaries are invisible and a broken boundary silently does a
    full reload.* Owner choice O6.

### 2.28 Data layer and collections

- **Best:** TanStack DB for client collections with live queries and
  optimistic writes [TSDB]; Django models and Rails Active Record on the
  server [DJ-MODELS] [RAILS-AR].
- **Jet today:** `core.db` with its own SQL engine (`Core/Db/Db.jet`,
  `Core/Db/Sql.jet`, `Examples/Data/Database`); typed queries over data
  frames (`Examples/Data/Analysis/TypedQuery.jet`).
- **Verdict:** behind.
- **Gap:** typed tables (#6822, waits on D-DB-TABLE1); search and relation
  loading (#6827); row policies (#6869).
- **Downsides of the best, and the Jet design:**
  - *ORM lazy loading hides N+1 queries.* Design: related rows load in one
    query (#6827 c3).
  - *Access rules live in app code and one path forgets them.* Design: a
    typed row policy judges every read and write (#6869 c3).

### 2.29 Background jobs

- **Best:** Laravel queues [LAR-QUEUE]; Oban [OBAN].
- **Jet today:** `core.jobs` is a durable queue with leases, heartbeats,
  retries and dead letters (`Core/Jobs/Jobs.jet`,
  `Examples/Concurrency/Services/JobQueue.jet`).
- **Verdict:** partial.
- **Gap:** app jobs, the after-commit outbox and schedules (#6825).
- **Downsides of the best, and the Jet design:**
  - *A job enqueued in a rolled-back transaction still runs.* Design: the
    outbox commits with the transaction (#6825 c1).

### 2.30 File conventions, scaffolding and admin

- **Best:** Next.js, Nuxt and SvelteKit file routing; Nuxt auto-imports
  [NUXT-AUTO]; Django admin [DJ-ADMIN]; Rails generators.
- **Jet today:** `routes(from:)` expands convention files into the one
  route table (`Core/Web/Web.jet:442`,
  `tests/cases/web_app_graph/routes_from_expands_convention_files.case`,
  `tests/cases/web_app_graph/convention_page_and_explicit_route_share_one_table.case`);
  `jet new` scaffolds a web app that runs in a browser
  (`tests/cases/web_scaffold/jet_new_web_scaffold_runs_from_new_to_browser.case`).
  No admin.
- **Verdict:** partial.
- **Gap:** generators with preview and undo (#6863). No card for an admin;
  section 5, row G5.
- **Downsides of the best, and the Jet design:**
  - *Opaque magic in file conventions and auto-imports.* Jet's convention
    routes land in the same explained table as explicit ones, and
    `jet expand` shows them (2.24). Built.

### 2.31 Partial page updates without client code

- **Best:** htmx swaps and out-of-band updates [HTMX] [HTMX-OOB]; Turbo
  Frames and Streams [TURBO-FRAMES] [TURBO-STREAMS].
- **Jet today:** no-script forms (`core.web.forms` `no_script`); no
  fragment swaps.
- **Verdict:** not adopted, pending the owner choice O3: D-WEBAPP-SHAPE1=A
  runs screens on the client, which covers the same need with typed data.
- **Downsides of the best, and the Jet design:**
  - *Hypermedia attributes are strings the server and page must agree on,
    with no check.* Jet's screens and routes are checked declarations.

### 2.32 Content collections

- **Best:** Astro content collections with schemas [ASTRO-CC].
- **Jet today:** none.
- **Verdict:** behind.
- **Gap:** no card. Section 5, row G6.
- **Downsides of the best, and the Jet design:**
  - *Front matter is validated at build time by a runtime schema library.*
    A Jet design would decode front matter with the type's codec and
    `validate` block, as every other input is. Not designed.

### 2.33 Dependency injection and modules

- **Best:** Angular DI [NG-DI]; Nuxt modules.
- **Jet today:** packages and modules are explicit `use` imports; services
  are values (`Core/Service/Service.jet`); effects and authority are checked
  per package.
- **Verdict:** not adopted: Jet passes services as values and checks
  effects, so a container is not needed.
- **Downsides of the best, and the Jet design:**
  - *Injection tokens fail at run time when a provider is missing.* In Jet
    a missing service is a compile error, because it is an argument.

## 3. Framework index

Each named framework, its signature elements, and the rows that judge them.

| Framework | Signature elements | Rows |
|---|---|---|
| TanStack Start | server functions, SSR, streaming | 2.16, 2.10, 2.12 |
| TanStack Router | typed routes and search params, loaders, preloading | 2.1, 2.2 |
| TanStack Query | cache, invalidation, mutations, offline | 2.3, 2.4 |
| TanStack Form | typed fields, async validation | 2.5 |
| TanStack Table | headless tables | 2.6 |
| TanStack Virtual | windowing | 2.7 |
| TanStack Store | store, selectors | 2.8 |
| TanStack DB | client collections, live queries | 2.28, 2.3 |
| TanStack devtools | Query and Router devtools | 2.24 |
| Next.js | RSC, ISR, streaming, server actions, images, metadata | 2.10, 2.11, 2.12, 2.16, 2.21 |
| Remix and React Router | loaders, actions, error boundaries | 2.2, 2.16, 2.25 |
| SvelteKit | load, form actions, adapters, remote functions | 2.2, 2.5, 2.26, 2.16 |
| SolidStart | signals, `"use server"` | 2.9, 2.16 |
| Qwik | resumability | 2.14 |
| Astro | islands, server islands, content collections, images | 2.13, 2.32, 2.21 |
| Nuxt | auto-imports, file routing, data fetching | 2.30, 2.2 |
| Angular | signals, DI, typed forms, hydration, i18n | 2.9, 2.33, 2.5, 2.14, 2.20 |
| Phoenix LiveView | server-driven views, presence, LiveViewTest | 2.18, 2.23 |
| Rails (Hotwire) | Turbo Frames and Streams, secure defaults, Active Record | 2.31, 2.19, 2.28 |
| Laravel (Livewire) | Livewire, Fortify, queues, validation | 2.18, 2.19, 2.29, 2.5 |
| Django | admin, ORM, auth, translation, forms | 2.30, 2.28, 2.19, 2.20, 2.5 |
| Leptos and Dioxus | wasm CSR, server functions, SSR modes, native targets | 2.15, 2.16, 2.12 |
| htmx | swaps, out-of-band updates | 2.31 |
| Elysia and Hono | middleware, validation, typed client, edge runtimes | 2.17, 2.26 |

## 4. Owner choices this audit raises

Each needs a ballot with same-program alternatives (#6552 c2). None is
decided here.

| ID | Choice | Row |
|---|---|---|
| O1 | Whether Jet writes static pages and regenerates them, and in what form (build output, or a cache in front of server rendering) | 2.11 |
| O2 | What an island means in the screen graph of D-WEBAPP-SHAPE1=A: a mounted wasm tree only, or partial hydration | 2.13 |
| O3 | Whether hypermedia fragment swaps are offered beside client screens, or left out | 2.31 |
| O4 | The devtools surface: an in-page panel, a `jet dev` page, or `jet inspect` only | 2.24 |
| O5 | Which deploy outputs ship beyond the native server and the static web build | 2.26 |
| O6 | Whether `jet dev` keeps screen state across edits, and how a change that cannot keep it is shown | 2.27 |
| O7 | Whether `core.web.query` and `core.web.router` are cut over into `app.live` and the one route table, or kept as typed client libraries | 2.1, 2.3 |

## 5. Gaps with no card

Each needs a child card under #6552 that closes with an executable example
and goldens on every applicable tier (method step 3).

| ID | Gap | Row |
|---|---|---|
| G1 | Static generation and regeneration, after O1 | 2.11 |
| G2 | Islands beyond mounting, after O2 | 2.13 |
| G3 | A devtools view of store, query and route facts, after O4 | 2.24 |
| G4 | Deploy outputs, after O5 | 2.26 |
| G5 | A generated admin over typed tables, after #6822 | 2.30 |
| G6 | Content collections decoded by the type's codec | 2.32 |

## 6. Sources

| Key | Source |
|---|---|
| ASTRO-CC | [Astro: content collections](https://docs.astro.build/en/guides/content-collections/) |
| ASTRO-IMG | [Astro: images](https://docs.astro.build/en/guides/images/) |
| ASTRO-ISL | [Astro: islands](https://docs.astro.build/en/concepts/islands/) |
| ASTRO-SISL | [Astro: server islands](https://docs.astro.build/en/guides/server-islands/) |
| CVX-RT | [Convex: realtime](https://docs.convex.dev/realtime) |
| DIOXUS-DOCS | [Dioxus 0.7](https://dioxuslabs.com/learn/0.7/) |
| DJ-ADMIN | [Django admin](https://docs.djangoproject.com/en/stable/ref/contrib/admin/) |
| DJ-AUTH | [Django authentication](https://docs.djangoproject.com/en/stable/topics/auth/) |
| DJ-I18N | [Django translation](https://docs.djangoproject.com/en/stable/topics/i18n/translation/) |
| DJ-MODELS | [Django models](https://docs.djangoproject.com/en/stable/topics/db/models/) |
| ELYSIA-EDEN | [Elysia: Eden](https://elysiajs.com/eden/overview.html) |
| ELYSIA-VAL | [Elysia: validation](https://elysiajs.com/essential/validation.html) |
| FORMATJS | [FormatJS: React Intl](https://formatjs.github.io/docs/react-intl/) |
| FORTIFY | [Laravel Fortify](https://laravel.com/docs/fortify) |
| HONO | [Hono: basics](https://hono.dev/docs/getting-started/basic) |
| HONO-RPC | [Hono: RPC](https://hono.dev/docs/guides/rpc) |
| HTMX | [htmx documentation](https://htmx.org/docs/) |
| HTMX-OOB | [htmx: hx-swap-oob](https://htmx.org/attributes/hx-swap-oob/) |
| LAR-QUEUE | [Laravel: queues](https://laravel.com/docs/queues) |
| LAR-VAL | [Laravel: validation](https://laravel.com/docs/validation) |
| LEP | [Leptos book](https://book.leptos.dev/) |
| LEP-SF | [Leptos: server functions](https://book.leptos.dev/server/25_server_functions.html) |
| LEP-SSR | [Leptos: SSR modes](https://book.leptos.dev/ssr/23_ssr_modes.html) |
| LW | [Livewire: components](https://livewire.laravel.com/docs/components) |
| NEXT-CSS | [Next.js: CSS](https://nextjs.org/docs/app/getting-started/css) |
| NEXT-DEPLOY | [Next.js: deploying](https://nextjs.org/docs/app/building-your-application/deploying) |
| NEXT-ERR | [Next.js: error.js](https://nextjs.org/docs/app/api-reference/file-conventions/error) |
| NEXT-IMG | [Next.js: Image](https://nextjs.org/docs/app/api-reference/components/image) |
| NEXT-ISR | [Next.js: incremental static regeneration](https://nextjs.org/docs/app/guides/incremental-static-regeneration) |
| NEXT-USE-SERVER | [Next.js: use server](https://nextjs.org/docs/app/api-reference/directives/use-server) |
| NG-DI | [Angular: dependency injection](https://angular.dev/guide/di) |
| NG-FORMS | [Angular: typed forms](https://angular.dev/guide/forms/typed-forms) |
| NG-HYD | [Angular: hydration](https://angular.dev/guide/hydration) |
| NG-I18N | [Angular: i18n](https://angular.dev/guide/i18n) |
| NG-SIG | [Angular: signals](https://angular.dev/guide/signals) |
| NG-SSR | [Angular: SSR](https://angular.dev/guide/ssr) |
| NUXT-AUTO | [Nuxt: auto-imports](https://nuxt.com/docs/guide/concepts/auto-imports) |
| OBAN | [Oban](https://hexdocs.pm/oban/Oban.html) |
| PHX-LV | [Phoenix.LiveView](https://hexdocs.pm/phoenix_live_view/Phoenix.LiveView.html) |
| PHX-PRES | [Phoenix.Presence](https://hexdocs.pm/phoenix/Phoenix.Presence.html) |
| PHX-TEST | [Phoenix.LiveViewTest](https://hexdocs.pm/phoenix_live_view/Phoenix.LiveViewTest.html) |
| PW-TRACE | [Playwright: trace viewer](https://playwright.dev/docs/trace-viewer) |
| PW-UI | [Playwright: UI mode](https://playwright.dev/docs/test-ui-mode) |
| QWIK | [Qwik: resumability](https://qwik.dev/docs/concepts/resumable/) |
| RAILS-AR | [Rails: Active Record basics](https://guides.rubyonrails.org/active_record_basics.html) |
| RAILS-SEC | [Rails: security guide](https://guides.rubyonrails.org/security.html) |
| RAILS-VAL | [Rails: validations](https://guides.rubyonrails.org/active_record_validations.html) |
| REACT-OPT | [React: useOptimistic](https://react.dev/reference/react/useOptimistic) |
| REACT-RSC | [React: Server Components](https://react.dev/reference/rsc/server-components) |
| REACT-STREAM | [React: renderToPipeableStream](https://react.dev/reference/react-dom/server/renderToPipeableStream) |
| REACT-SUSPENSE | [React: Suspense](https://react.dev/reference/react/Suspense) |
| REP | [Replicache: how it works](https://doc.replicache.dev/concepts/how-it-works) |
| RR-ERR | [React Router: error boundaries](https://reactrouter.com/how-to/error-boundary) |
| RR-LOAD | [React Router: data loading](https://reactrouter.com/start/framework/data-loading) |
| SK-ACT | [SvelteKit: form actions](https://svelte.dev/docs/kit/form-actions) |
| SK-ADAPT | [SvelteKit: adapters](https://svelte.dev/docs/kit/adapters) |
| SK-ERR | [SvelteKit: errors](https://svelte.dev/docs/kit/errors) |
| SK-LOAD | [SvelteKit: loading data](https://svelte.dev/docs/kit/load) |
| SK-RF | [SvelteKit: remote functions](https://svelte.dev/docs/kit/remote-functions) |
| SOLID-SERVER | [SolidStart: use server](https://docs.solidjs.com/solid-start/reference/server/use-server) |
| SOLID-SIG | [Solid: signals](https://docs.solidjs.com/concepts/signals) |
| STYLEX | [StyleX](https://stylexjs.com/) |
| TRPC | [tRPC](https://trpc.io/docs) |
| TSDB | [TanStack DB](https://tanstack.com/db/latest/docs/overview) |
| TSF | [TanStack Form](https://tanstack.com/form/latest/docs/overview) |
| TSQ | [TanStack Query](https://tanstack.com/query/latest/docs/framework/react/overview) |
| TSQ-DEV | [TanStack Query devtools](https://tanstack.com/query/latest/docs/framework/react/devtools) |
| TSQ-OPT | [TanStack Query: optimistic updates](https://tanstack.com/query/latest/docs/framework/react/guides/optimistic-updates) |
| TSR | [TanStack Router: type safety](https://tanstack.com/router/latest/docs/framework/react/guide/type-safety) |
| TSR-DEV | [TanStack Router devtools](https://tanstack.com/router/latest/docs/framework/react/devtools) |
| TSR-LOAD | [TanStack Router: data loading](https://tanstack.com/router/latest/docs/framework/react/guide/data-loading) |
| TSR-SEARCH | [TanStack Router: search params](https://tanstack.com/router/latest/docs/framework/react/guide/search-params) |
| TSS | [TanStack Store](https://tanstack.com/store/latest/docs/overview) |
| TSS-SF | [TanStack Start: server functions](https://tanstack.com/start/latest/docs/framework/react/guide/server-functions) |
| TST | [TanStack Table](https://tanstack.com/table/latest/docs/overview) |
| TSV | [TanStack Virtual](https://tanstack.com/virtual/latest/docs/introduction) |
| TURBO-FRAMES | [Turbo Frames](https://turbo.hotwired.dev/handbook/frames) |
| TURBO-STREAMS | [Turbo Streams](https://turbo.hotwired.dev/handbook/streams) |
| TW | [Tailwind CSS](https://tailwindcss.com/docs) |
| VE | [vanilla-extract](https://vanilla-extract.style/) |
| VITE | [Vite: features](https://vite.dev/guide/features) |
| VITE-HMR | [Vite: HMR API](https://vite.dev/guide/api-hmr) |
| ZERO | [Zero: mutators](https://zero.rocicorp.dev/docs/mutators) |
