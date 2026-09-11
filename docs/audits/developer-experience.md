# Developer Experience — one program, observed once, rendered everywhere
- The owner's ask is simple: every quality-of-life feature that makes a strong ecosystem smooth must exist natively in Jet, first for Jet itself and then at equal depth for web, games, backend, live work, systems, mobile, data, and CLI/TUI work.
- The unifying idea is one program model, observed once, rendered everywhere: semantics stay in Jet and the protocol, panels, and hosts only project checked facts.
- This epoch ships one entry resolver, discoverable jobs, `jet.devtools.v1`, a typed panel API, five hosts, state-preserving edits, derived web rendering, and the `core.web` suite.
- The eight owner decisions are D-DX-ENTRY1, D-DX-JOBS-UX1, D-DX-DEVTOOLS-UX1, D-DX-PLUGIN1, D-DX-LIVE1, D-DX-WEBARCH1, D-DX-SUITE1, and D-DX-PROD1.
- The five hosts are the browser surface, the `jet dev` workbench, the terminal, the editor, and a native in-app overlay.
- Web rendering becomes a compiler fact per route, and the same route, query, form, table, store, trace, and error facts reach every host.
- A compatible edit keeps state on every applicable tier; an incompatible edit restarts the smallest safe unit and names the reason.
- The epoch is `e14`, with milestones e14-m01-entry-and-commands, e14-m02-devtools-core, e14-m03-live-loop, e14-m04-web-suite, e14-m05-games, e14-m06-backend, e14-m07-live-timetravel, e14-m08-systems-toolchain, e14-m09-mobile, e14-m10-data-notebooks, e14-m11-cli-tui, and e14-m12-proof.

## The bar

The census uses `already-implemented` as shipped, `ratified-in-progress` as ratified, and `real-gap` as gap; other states stay visible in the notes. `P0` is the total number of P0 rows, not only P0 gaps.

| domain | best loop (tool) | the mechanism that makes it feel great | Jet today (shipped / ratified / gap counts from the census) | the beat vector Jet already holds |
|---|---|---|---|---|
| web-tooling | Scaffold, edit, inspect, test, and build with Vite, Bun, Deno, and Playwright | One command reaches a runnable app, native watch gives fast feedback, plugins and reports use typed records, and failed work keeps its last good result | 72 rows: 29 / 0 / 37; 6 need measurement; 21 P0 | One checked web graph and one `jet dev` loop already join scaffold, effects, diagnostics, and last-good recovery |
| web-frameworks | TanStack Router, Query, Form, Table, Virtual, Store, and Start | Typed route/data/write flow keeps params, cache state, form state, visible rows, and pending or error states in one model | 53 rows: 7 / 0 / 45; 1 owner gate; 31 P0 | One typed graph carries route, action, mount, rendering, policy, adapter, and effect facts |
| live | Redux DevTools, Replay, Pharo, CIDER, Erlang, and rr | A running process or recorded session stays inspectable, state survives compatible edits, and history can be replayed at a chosen point | 57 rows: 12 / 6 / 39; 26 P0 | Shared TIR parity, typed persistence, module-aware swap, atomic last-good recovery, and source-first frames |
| games | Unreal, Unity, and Godot editor loops, with Bevy's composable runtime | One surface runs, pauses, advances a frame, inspects a live object, keeps or discards changes, profiles, and packages | 91 rows: 10 / 7 / 70; 3 owner gates; 1 rejected conflict; 46 P0 | A typed `Scene`, deterministic headless replay, source-backed Canvas, and machine-readable scene performance identity |
| backend | Phoenix, Laravel, Rails, Spring, Aspire, Django, and Erlang | A local control plane links request, query, job, service, error, trace, migration, and deployment state | 89 rows: 6 / 0 / 83; 42 P0 | Typed HTTP authority, named jobs, bounded traces, explicit effects, last-good swap, and payload-free observation |
| systems | Cargo, rustc, rust-analyzer, Bacon, Zig, Go, and Criterion | One project graph owns quiet defaults, exact diagnostics, focused tests, measured profiles, and named tasks | 68 rows: 34 / 0 / 15; 19 need measurement; 32 P0 | Typed What/Why/Fix diagnostics, proof and cost records, semantic inspection, target authority, and last-good execution |
| mobile | Flutter, Expo, SwiftUI/Xcode, Android Studio/Compose, and Kotlin Multiplatform | Scaffold, target selection, preview, device run, inspector, profile, and release controls share one visible target identity | 71 rows: 8 / 6 / 32; 4 need measurement; 21 owner gates; 39 P0 | One retained `JetUiNode` tree, reactive rendering, Canvas source authority, DAP, traces, and exact effect denial |
| data | Jupyter, marimo, Observable, Pluto, Polars, DuckDB, Streamlit, and Quarto | One kernel or dependency graph reruns the right work, exposes tables and plots, and publishes with explicit state and loss | 49 rows: 10 / 6 / 27; 1 needs measurement; 3 owner gates; 2 rejected conflicts; 26 P0 | One executable meaning across tiers, stale output as a correctness state, deterministic plots, bounded streams, and fail-closed bridges |
| cli | Bubble Tea, Lip Gloss, Bubbles, Huh, Ratatui, Rich, and Textual | A model/update/view loop, capability-aware layout, guided input, adaptive output, and headless frame tests make terminal work composable | 67 rows: 16 / 0 / 45; 5 need measurement; 1 rejected conflict; 36 P0 | One checked CLI schema drives help, completion, man, JSON, diagnostics, effects, and cross-tier behavior |

The nine census reports and their `census.json` files supply the domain evidence in this proposal; the web-tooling report also records the measured browser reload gap.

## Where the loops diverge today

| domain | what the developer types and sees minute to minute in the best ecosystem | what the developer types and sees in Jet today |
|---|---|---|
| web-tooling | Run a scaffold command, start the dev server, edit a module, see a module update without losing app state, open a devtool or test UI, and inspect a trace or report. | Run `jet new`, `jet dev`, or `jet test`; see a checked graph, a status surface, and last-good recovery, but web edits still rebuild and reload the page and there is no unified panel or plugin loop. |
| web-frameworks | Define a route, loader, query, form, table, or store; get generated types; navigate or submit; see pending, fresh, stale, error, and mutation state in the same application tools. | Define `fn run() App` and a typed builder chain; inspect CSR or island labels and policy facts, but typed search params, loader state, query cache, forms, tables, virtual rows, and store history are mostly absent. |
| live | Edit a definition in a live process, trigger input, pause, inspect state or source, jump through history, evaluate a new question, and replay a failure without the original service. | Run `jet dev`, choose automatic rerun or resident swap, inspect live tasks and effects, and keep the last-good program; there is no shipped timeline, historical query, rich value inspector, or reverse step. |
| games | Create a project, press Play, pause or advance one frame, eject or edit an object, inspect the live tree, keep or discard the edit, profile the frame, and package with named profiles. | Run a typed `Scene` headless probe and see assets, input, components, replay, and frame rows; there is no live rendered editor process, remote entity tree, frame debugger, asset reload, or game package loop. |
| backend | Generate a schema or service slice, start one local server, open the route, inspect request and SQL timing, watch jobs and queues, reproduce in a console, and trace the same request through failure. | Start `jet dev`, use typed HTTP and named jobs, retain last-good code, and emit bounded traces; request, exception, query, queue, migration, service topology, and safe console views are not integrated. |
| systems | Run one project command, get quiet passing output or a source-local diagnostic, focus the failing test, rerun it in a persistent pane, inspect a graph, then compare a named profile or coverage artifact. | Use `jet run`, `jet test`, `jet check`, `jet perf`, `jet explain`, and `jet inspect`; diagnostics are strong, but `jet test --watch`, failed-first focus, richer capture controls, and measured editor/profile flows are incomplete. |
| mobile | Create a target project, run doctor, choose a simulator or device, preview a screen, edit and see a safe reload, inspect a widget, profile with an explicit mode, and build or sign the same target. | Use `jet new`, `jet dev`, `jet perf`, and the portable UI tree; the checked binary has no Android or iOS target, device identity, simulator, signing path, mobile preview, or mobile release loop. |
| data | Open one notebook or script, edit a cell, rerun only descendants, inspect a graph, show a table or plot, query a lazy plan, and publish the same source with provenance. | Run `jet notebook --headless`, edit JSONL cells, inspect cache and stale state, export with explicit loss lines, and use typed data functions; rich controls, automatic descendant rerun, lazy plan inspection, and script-to-app publication remain gaps. |
| cli | Declare a model or typed command, run it in one terminal process, edit style or behavior, see the next frame, use adaptive output, and assert a headless frame or tape. | Declare `fn run()` or `fn run(args: #CLI)`, get help, completion, man, JSON, exact errors, and typed jobs; a reusable native TUI, guided prompts, frame snapshots, and terminal recordings are not a complete loop. |

## The proposal

### One command runs the right thing

A bare command must feel obvious without hiding its choice. The resolver applies the same rule to `run`, `dev`, `build`, `test`, `check`, and `doc`.

1. The root role home `@<cmd>.jet` wins, as D-CMDOVERRIDE1 already rules.
2. Exactly one root-level `.jet` file with a top-level `fn <cmd>` wins next; this inference is new.
3. If the root has no match, the same single-file rule applies to `src/`.
4. The stock default follows: a declared `defaults` entry or one compatible `Output`, then canonical `run.jet` and `src/run.jet`.
5. If nothing matches, or two files match, Jet emits a registered diagnostic with every candidate and a `jet fix` action that pins one.

The resolver never searches deeper than `src/`. The ratified override order does not change for shipped packages: a package with both `@run.jet` and `defaults.run` keeps running `@run.jet`. `jet test` keeps the package `#Test` runner as its stock fallback. `--show-default` bypasses overrides and reports the stock entry.

Jobs stay ordinary functions. `#Job` supplies typed visibility and arguments, `jet jobs` lists name, scope, documentation, typed arguments, and schedule, and the ballot's recommended runner `jet jobs <name> [-- <args>]` runs one job of the resolved entry. `jet run <entry> -- <name>` stays the canonical ratified form, `.Internal` jobs never reach argv, and a name that matches no job reports E1294 with the entry's job list. Bare `jet run <name>` never guesses between a file and a job.

A worked package keeps its entry, sources, tests, and output fact visible:

```text
orders/
├── package.jet
├── @run.jet
├── src/
│   ├── Orders.jet
│   ├── Cart.jet
│   └── Payments.jet
└── tests/
    └── ShipOrder.jet
```

The project can pin an executable with existing syntax:

```jet
app :: Output.Executable{ name: "orders", entry: run }
defaults: { run: app };
```

The role home can contain the existing entry and job forms:

```jet
use core.web as web
#Target(Web)

fn run() App -> {
    return web.app()
        .csr()
        .route("/", home)
}

#Job(.Dev) fn Seed() { print("seeded") }
```

The proposed transcript makes the hidden choice visible:

```text
$ jet run --show-default
stock entry: package.jet -> orders

$ jet jobs
Seed  [Dev]  Load 1,000 sample orders
Lint  [Dev]  Run policy lints on the package

$ jet jobs Seed
run: job Seed [Dev]
seeded

$ jet run ./@run.jet -- Seed
run: job Seed [Dev]
seeded

$ jet jobs Sed
error E1294: No job named `Sed`
declared jobs: Seed, Lint
```

| rung | default path | what the rung teaches |
|---|---|---|
| beginner | `jet dev` or `jet run` | A package needs one command; Jet finds the written entry and reports the result. |
| intermediate | `jet dev ./@run.jet`, `jet jobs`, and `jet jobs Seed` | An explicit file, the job list, and a named job are all available without learning a second task language. |
| expert | `defaults: { run: app };`, `jet run ./@run.jet -- Seed`, and `--show-default` | The project can pin the stock entry, the invocation can name the entry and job, and the resolver can be audited. |

**Exits.** See the choice with the real command `jet run --show-default`; spell it with the real explicit path `jet run ./@run.jet` or the real manifest form `defaults: { run: app };`; refuse file inference for a project by removing role files and pinning that manifest default. See jobs with the real command `jet jobs`; spell a job with the real canonical form `jet run ./@run.jet -- Seed`; refuse the named runner by always using an explicit entry path and separator.

### One observation protocol, five hosts

`jet.devtools.v1` is one typed event stream and one query surface. Build events carry status, tier, diagnostics, and kept or reset state; Route events carry match, render mode, and reason; Query events carry fetching, fresh, stale, invalidated, and footprint state; Mutation events carry lifecycle and rollback; Form events carry field state and validation; Table events carry viewport, sort, and filter; Store events carry transactions, diffs, and time; Trace events carry request, render, job, reload, and frame spans; Cost events carry D-COSTLAW1 rows; Gate events carry the ledger; Structure, Test, Job, Frame, Request, Response, and Custom events complete the families.

The protocol lives in one Prelude part. AOT, JIT, interpreter, and web adapters marshal it but do not redefine its meaning. Existing `Observe.rs` snapshots, `LiveInspect`, `.jettrace`, WebHost status, and Canvas routes become producers or consumers of the same stream. Store and query state use a bounded development ring with monotonic timestamps, and every host can scrub one time cursor.

The default is dev-only, loopback-only, payload-free observation. A site must explicitly publish a value. Release builds remove the devtools path by default; the expert production choice is the authenticated read-only endpoint in D-DX-PROD1.

A panel is a Jet function that returns `core.ui` nodes from typed devtools state. The marker spelling remains the D-DX-PLUGIN1 naming menu; the following uses one proposed option:

```jet
use core.ui as ui

// proposed: `#Panel` is a marker option from D-DX-PLUGIN1; `DevtoolsFacts` is typed protocol state.
#Panel
fn payments_panel(facts: DevtoolsFacts) UiNode -> {
    return ui.box([
        ui.text("Payments"),
        ui.text("authorized={facts.authorized}"),
        ui.text("captured={facts.captured}")
    ])
}
```

The five hosts render that same `core.ui` tree:

| host | opens with | sees |
|---|---|---|
| browser | the in-app surface injected by `jet dev` | a calm status pill, a compact lens, and route, query, form, and trace details |
| workbench | the `jet dev` workbench URL for any program | the full panel tree, timeline, source links, and shared time cursor |
| terminal | the interactive `jet dev` TUI | the same panels in `TuiBackend`, with keyboard paths and `NO_COLOR` output |
| editor | an LSP custom request in VS Code or Zed | the workbench page in a loopback webview beside source and diagnostics |
| native overlay | the app's toggle key in a GUI or game | the same panels through the app's `JetBackend`, including Frame and Entity panels |

The in-app pill stays small and useful:

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ Orders                                      /orders/1042        ● ready  ⌄    │
└──────────────────────────────────────────────────────────────────────────────┘
                                                       click or press I
```

The lens answers the next question without taking over the page:

```text
┌─ Orders · live lens ──────────────────────────────────────────────────────────┐
│ Build 214   ready   380 ms build   120 ms reload   kept 4   reset 0            │
│ Route       /orders/:id   server + island   loader orders.byId                 │
│ Query       orders.list   fresh   41 ms   1 observer   1,000 rows              │
│ Form        ShipOrder     dirty   invalid   carrier: choose a carrier         │
│ [Open workbench]  [Inspect source]  [Copy event]                               │
└──────────────────────────────────────────────────────────────────────────────┘
```

The workbench gives the full program three stable panes:

```text
┌─ Orders · workbench ─────────┬─ Timeline ─────────────────┬─ Selected fact ───────┐
│ Build                         │ 12:05:11 reload           │ Route /orders/:id      │
│ Routes                        │ 12:05:11 Store Cart       │ mode server + island  │
│ Queries                       │ 12:05:09 mutation ship    │ reason: loader plus   │
│ Mutations                     │ 12:05:03 query orders     │ interactive ShipOrder │
│ Forms                         │ 12:04:03 request 200      │ source: Orders.jet    │
│ Tables                        │                            │ [go to source]        │
│ Store                         │ [scrub time] [pause]      │ [inspect values]      │
│ Traces  Cost  Gates  Tests    │                            │                        │
└──────────────────────────────┴────────────────────────────┴────────────────────────┘
```

The terminal uses the same facts rather than a second data model:

```text
Orders  build=ready  tier=jet dev (JIT)  port=43127  reload=120ms

BUILD       kept 4  reset 0  last build 380ms
ROUTES      / static       /orders server+stream       /orders/:id server+island
QUERIES     orders.list fresh 41ms   orders.byId fresh 18ms   me stale   carriers fetching
FORMS       ShipOrder dirty invalid carrier: choose a carrier
TABLE       OrdersTable 1,000 rows 24 visible 32 rendered sort=created desc
TRACE       request 41ms  query 38ms  render 6ms  mutation 220ms  reload 120ms

keys: tab panel  enter inspect  t time  r rerun  q quit
```

| rung | interaction | result |
|---|---|---|
| beginner | Run `jet dev` and click the pill. | The lens shows build, route, query, form, and reload state without setup. |
| intermediate | Use the lens, the terminal panel keys, or the editor request. | Selection and time stay in sync across browser, workbench, terminal, and editor. |
| expert | Write a proposed panel marker and publish a typed Custom event at its site. | A package adds a panel without a runtime registration call, and the host applies the same privacy and release rules. |

**Exits.** See the live state with the real `jet dev` surface or `jet inspect live <pid>`; spell an extension with the proposed typed panel form above and a typed `Custom` event; refuse observation with the real environment switch `JET_OBSERVE=0 jet dev` or the proposed project switch `devtools: { observe: false }`. See payload-free output by leaving values unpublished; spell a value publication at the proposed site marker; refuse payloads by omitting that marker.

### Edits keep state on every tier

When you change code while it runs, Jet swaps in the new code and keeps everything it can prove still fits. If something no longer fits, it restarts that part and tells you exactly why.

The reload unit is a module. A type-stable edit swaps in place. A layout change uses `#Persist` migration rules when available or performs a clean announced restart. The Build panel and terminal list kept and reset values after every reload. Web keeps signals, stores, query cache, form drafts, scroll, and focus while rebinding server functions; native servers keep listeners and open connections; games keep a `#Persist` world.

The web baseline today rebuilds and reloads the browser:

```text
$ jet dev examples/features/web/web_app.jet
ready  web  port=43127  reload=full-page

edit: fn home
build: passed
reload: browser page
state: reset signals, store, query cache, form draft, scroll, focus
```

The proposed web loop keeps state inside the DOM runtime:

```text
$ jet dev orders
ready  web  port=43127  reload=module

edit: src/Orders.jet  fn pick_carrier
build: passed  changed module Orders
reload: swapped module in 120ms
kept: signals 3, Cart store, query cache 4/4, ShipOrder draft, scroll /orders, focus carrier
reset: none
```

The native server uses existing HTTP and task forms, then applies the same reload law:

```jet
use core.http.server as server
use core.net as net
use core.tasks as tasks

fn handle(req: HTTPRequest) HTTPResponse !HTTPError -> {
    return Ok(server.response(200, "orders"))
}
```

```text
$ jet dev src/Server.jet
ready  native  listener=127.0.0.1:43127  mode=resident
connection 17 open

edit: src/Server.jet  fn handle
build: passed
reload: swapped module
kept: listener, connection 17, in-flight request
reset: none
```

| rung | what the developer learns | proof of the rule |
|---|---|---|
| beginner | Save a function body and keep the visible page or server session. | The host shows `kept` and `reset` lists after the edit. |
| intermediate | Change a type or layout and read the announced restart reason. | `jet dev` keeps the last-good session and names the affected values. |
| expert | Choose `--restart`, `--swap`, or `--watch=off`; use `#Persist` for a named native or game value. | The explicit flag wins over auto-detection, and the persisted value has a typed identity. |

**Exits.** See the chosen mode with the real `jet dev` status and `jet inspect live <pid>`; spell a native retained value with the real `#Persist counter := 0` form and spell the reload policy with the real `--restart`, `--swap`, or `--watch=off` flags; refuse state retention with `--restart` or a project policy that selects restart for the dev command. The proposed `jet explain --reload` view adds the reason as a queryable fact.

### Rendering is a compiler fact

The developer writes one route graph. The compiler reads data dependencies and interactivity, then derives a route mode and records the reason. A route with no runtime data is static; a route with loader data is server-rendered and can stream; an interactive subtree is an island; an explicitly client-only route stays client. Islands resume from serialized state instead of re-running setup code, and a non-serializable capture is a diagnostic.

The four-route Orders app uses the existing web builder surface:

```jet
use core.web as web
#Target(Web)

fn home() WebPage -> web.page("Orders", "Track every order")
fn orders_page() WebPage -> web.page("Orders", "Pending orders")
fn order_detail() WebPage -> web.page("Order", "Ship order")
fn settings() WebPage -> web.page("Settings", "Preferences")

fn run() App -> {
    return web.app()
        .csr()
        .route("/", home)
        .route("/orders", orders_page)
        .route("/orders/:id", order_detail)
        .route("/settings", settings)
        .security("csp-default")
        .cache("revalidate")
        .a11y("wcag-aa")
        .adapter("node")
        .hydration_dev()
}
```

Today, the checked example reports explicit builder labels, not derived route facts:

```text
Command: scripts/agent/jet-env jet explain --web-graph examples/features/web/web_app.jet
Output:
web application graph — web_app.jet
hydration: dev-overlay (shared TIR: true)
  / [csr] -> home (builder)
  /widget [island] -> home (builder)
  action save (action) -> save (builder)
  mount /api -> api_mount effects=Net security=csrf (builder)
  security: csp-default
  assets: examples/features/web/public
  split: widget
  cache: revalidate
  a11y: wcag-aa
  adapters: node
```

The proposed output makes the four route decisions and reasons first-class:

```text
// proposed output
web application graph — Orders
hydration: dev-overlay (shared TIR: true)
  /            [static]          reason: no runtime data
  /orders      [server + stream] reason: loader orders.list
  /orders/:id  [server + island] reason: loader orders.byId; ShipOrder is interactive
  /settings    [client]          reason: explicit .render(.Client)
serialization:
  /orders/:id  ShipOrder state resumable
cache:
  orders.list  effect footprint: orders
  orders.byId  effect footprint: orders:1042
```

The design keeps `.render(...)` as the expert override; the line above is proposed until the per-route spelling is implemented. Existing verified builder methods remain `.csr()`, `.ssr()`, `.ssg()`, `.stream()`, and `.island()`.

| route archetype | what Jet derives or records | trap removed | expert exit |
|---|---|---|---|
| SPA | Client mode when the route needs client execution or the author chooses it | The whole app does not need to pay for client execution when its route has no client fact | `.csr()` is a real explicit builder choice; a proposed `.render(.Client)` pins one route |
| SSR | Server mode when loader data or server effects exist | The developer does not hand-maintain a server/client file split for ordinary route data | `.ssr()` is a real explicit builder choice |
| SSG | Static mode when the route has no runtime data | Static output does not require a separate page language or duplicated route graph | `.ssg()` is a real explicit builder choice |
| streaming | Server plus stream when loader data can arrive in parts | Pending boundaries stay tied to the route data graph instead of ad hoc loading flags | `.stream()` is a real explicit builder choice |
| islands | Server shell plus an interactive subtree | The developer does not mark every component as client code or rebuild the entire page | `.island()` is a real explicit builder choice |
| resumability | An island serializes state and listeners and resumes at the event | Hydration does not replay all setup code, and unsupported captures fail at check time | The proposed route option can refuse a capture with a serialization diagnostic |
| RSC-style split | Server facts and client facts remain in one checked route graph | No transitive `use client` boundary or second cache identity is hidden from the graph | The proposed `.render(...)` override and `jet explain --web-graph` show the written exception |

**Exits.** See the derived decision with the real command `jet explain --web-graph examples/features/web/web_app.jet`; spell a mode with the real `.csr()`, `.ssr()`, `.ssg()`, `.stream()`, or `.island()` builder method, and use the proposed `.render(...)` only for a per-route exception; refuse derivation by writing the explicit mode on the route or by the proposed project switch `rendering: .Explicit`.

### The first-party suite

`core.web` groups Router, Query, Forms, Table and Virtual, and Store over `core.reactive`. The suite keeps the beginner path inferred from the `App.route` graph and struct fields, while experts can supply configuration objects. Each module emits its own panel facts.

The current live-query floor is real and effect-tracked:

```jet
use app

fn run() {
    q :: app.live("orders", "{status:pending,page:1}")
    print(app.live_show(q))
    hit :: app.invalidate("orders")
    print("hit:{hit}")
}
```

The suite's proposed Orders API keeps invalidation tied to a read footprint rather than a hand-maintained key list:

```jet
use core.web as web
use app

// proposed: Query derives its cache identity and footprint from the checked callback.
orders :: web.query("orders.list", () -> app.live("orders", "{status:pending,page:1}"))

// proposed: Forms derives fields and validation from the Order shipment input struct.
ship_order :: web.form("ShipOrder", Order)

// proposed: Table and Virtual keep the typed row model while rendering only the viewport.
orders_table :: web.table("OrdersTable", [Order]).virtual()

// proposed: Store records typed transactions for the Store panel and time cursor.
cart :: web.store("Cart")
```

A `DB.Read` callback records the footprint. A committed `#Transact` write intersects that footprint, invalidates only the affected query, reruns it, and pushes the result to a client `Signal<T>`. For a source outside the tracker, the expert floor remains the real `app.subscribe` and `app.invalidate` pair.

| rung | suite path | result |
|---|---|---|
| beginner | Build the Orders `App` and pass an `Order` struct to the form. | Router inputs and form fields derive from checked source facts. |
| intermediate | Use the Query, Table, Virtual, and Store panels while editing `/orders`. | Pending, fresh, stale, mutation, viewport, and history state stay visible beside the feature. |
| expert | Use the real `app.subscribe` and `app.invalidate` escape hatch for an untracked source, or supply the proposed explicit configuration objects. | The developer can opt out of inference without creating a second reactive primitive or cache model. |

**Exits.** See suite state in the proposed Query, Form, Table, or Store panels; spell an outside-source dependency with the real `app.subscribe` and `app.invalidate` calls; refuse the inferred defaults with explicit proposed configuration objects or a proposed project switch `web: { infer: false }`.

### Every domain at the same depth

Every domain receives the same six pieces: a useful loop, panels, probes, a scaffold, a benchmark peer, and cards that close named census findings. Domain syntax stays out of this proposal until its plan card authors the required ballot.

| domain | the loop we ship | the panels | the probes | the scaffold | the benchmark peer | the cards by title from OUTLINE.md | the decisions the plan lane will author |
|---|---|---|---|---|---|---|---|
| web-tooling | `jet new` to `jet dev`, inspect, test, and build with last-good recovery | Build, Routes, Queries, Mutations, Forms, Table, Store, Traces, Cost, Gates, Structure, Tests, Jobs, UI tree | Browser and terminal event stream, source identity, test UI, trace viewer | `jet new --target=web` | Vite, Bun, Deno, TanStack Start, Playwright | First-party devtools panels; Browser host; Terminal host; Editor host; Native overlay host; Devtools built in Jet; Browser tests; Prior-art registry | D-DX-BROWSERTEST1 for the test contract and CLI surface |
| web-frameworks | Typed route, loader, query, form, table, virtual, store, and progressive action loop | Router, Query, Form, Table, Store, Routes, Traces | Route graph, query invalidation, form validation, 100,000-row viewport, browser matrix | `jet new --target=web` | TanStack Start and Astro | Router; Query; Forms; Table and Virtual; Store; Server functions and progressive actions; Streaming and pending boundaries; Reference app | D-DX-ROUTER1, D-DX-QUERY1, D-DX-FORM1, D-DX-TABLE1, D-DX-STORE1, D-DX-SERVERFN1 |
| live | Record, pause, inspect, scrub, evaluate, replay, and continue with identity-bound artifacts | Build, Store, Trace, Structure, Gates, Tests | `jet prove`, replay, paused evaluator, reverse-step and divergence cases | A dev-session example beside the normal package scaffold | Redux DevTools, Replay, Pharo, CIDER, rr | Record and replay a dev session; Inspect and evaluate in a paused session; Reverse step and fix-and-continue; Named previews and playground; Web module swap; Native swap hardening | D-DX-DEBUGBACK1 and D-DX-PREVIEW1 |
| games | Run, pause, frame, inspect, edit, profile, reload, and package a source-backed scene | Frame, Entity, Asset, Trace, Build, Cost, Structure | `JET_SCENE_PROBE`, headless replay, remote tree, draw-event identity | `jet new game` with a runnable scene and dev profile | Unreal, Unity, Godot, Bevy | Game play loop; Game overlay; Live world inspector; Frame profiler and debugger; Asset pipeline; Game hot swap; Game packaging; `jet new game` | D-DX-GAMELOOP1 and D-DX-GAMEPKG1 |
| backend | Generate, serve, inspect request and query state, run jobs, migrate, trace, and recover from errors | Request, Response, Exception, Query, Job, Service, Metrics, Logs, Traces, Database | HTTP server, request identity, `.jettrace`, queue lifecycle, migration preview, safe EXPLAIN | `jet new service` with route, job, migration, and test | Phoenix, Laravel, Rails, Spring, Aspire | Request, exception, and query panels; Jobs and queues panels; Metrics, logs, and traces; Service topology and process; Actionable local error page; Migrations; Durable job queue; Backend generators; Capability-gated console; Database stats panel | D-DX-MIGRATE1, D-DX-QUEUE1, and D-DX-CONSOLE1 |
| systems | Scaffold, run, check, test, focus a failure, inspect the graph, explain a diagnostic, and compare proof | Build, Test, Job, Structure, Cost, Gate, Trace | `jet test`, `jet perf`, `jet explain`, job graph, diagnostics matrix | `jet new` with named role files | Cargo, rustc, Bacon, Zig, Go, Criterion | Test loop; Diagnostics rendering parity; Job graph; Explicit source generation loop | D-DX-TESTUX1 and D-DX-JOBGRAPH1 |
| mobile | Doctor, preview, choose a device, reload, inspect, profile, and release with one target identity | Build, Structure, Frame, Trace, Request, Gates | Existing cache-only smoke and authority probe; future device, preview, and profile probes | Mobile scaffold and device-loop design after e13-m03-mobile | Flutter, Expo, SwiftUI, Compose, Kotlin Multiplatform | Mobile dev loop design; Mobile scaffold, build profiles, signing, and destinations | D-DX-MOBILELOOP1 and D-DX-MOBILEBUILD1 |
| data | Edit a cell or script, rerun dependencies, inspect table and plan, plot, publish, and preserve loss records | Query, Table, Store, Trace, Structure, Cost, Gates | `jet notebook --headless`, DataStats, stale cache, lazy plan, deterministic plot | Notebook and typed function app scaffold | Jupyter, marimo, Polars, DuckDB, Observable | Reactive notebook; Run a notebook or typed function as a local app; Lazy table plans; Data loaders; Plot grammar; Local SQL console | D-DX-CELLS1, D-DX-APPFROMSCRIPT1, D-DX-PLOT1, and D-DX-SQLSHELL1 |
| cli | Declare typed commands or a TUI, run, edit, rerender, guide input, record, and test frames | Build, Test, Job, Structure, Trace, Custom, UI tree | Completion, PTY, headless frame snapshot, interaction driver, tape | `jet new` with `#CLI` and a TUI profile | Bubble Tea, Textual, Ratatui, clap, Typer | First-party TUI kit; Guided input; Human output primitives; TUI tests and recordings; Dynamic completions; Style live reload | D-DX-TUIKIT1 and D-DX-PROMPT1 |

### Proof

The proof milestone measures the loop, not only implementation presence. Every domain uses the same warm and cold protocol and keeps peer identity, machine, profile, source revision, and artifact identity beside the result.

| measure | what counts | peer set |
|---|---|---|
| time to first run | scaffold to a visible, successful program with no hand-edited config | Vite/Bun/TanStack Start; Flutter/Expo; Cargo/Go/Zig; Unity/Godot/Bevy; Phoenix/Rails/Aspire; marimo; Textual |
| edit to see | save a representative body or style edit and record the first visible correct result | the same peer set, with clean and warm paths separated |
| error to fix | create one typed or runtime failure, follow the first actionable link, and reach a passing result | Jet diagnostics against rustc/miette, Next overlay, backend local error pages, and domain peers |
| test loop | first test, focused failure, rerun, captured output, and final report | Jet, Bun, Deno, Go, Cargo/nextest, Playwright, Textual |
| build and package | build or package the reference program with an identity-bound receipt | web, native, game, backend, mobile, data, and CLI peers |
| observation depth | reach the same fact from browser, workbench, terminal, editor, and native hosts | one recorded `jet.devtools.v1` stream and one visual snapshot per host |

The fresh-agent run reuses #2324: give a new agent only the package tree, one task sentence, the available command list, and the project constraints; record the time to first run, first edit, first repair, and first useful panel. The agent must not receive this proposal or a prepared command transcript.

Owner visual acceptance uses real programs in the six states: ready, building, error, reconnecting, stale, and recovered. Good means the pill is calm, the lens answers the next question, the workbench is dense but readable, terminal output works without color, source links are exact, payloads stay absent by default, and every restart names what changed.

## Final vision

This is the complete Orders program at the end of e14. Existing lines use the verified Jet builder and UI forms. Proposed suite and derived-rendering lines carry an explicit comment at first use.

```jet
use core.web as web
use core.ui as ui
use core.reactive as reactive
#Target(Web)

struct Order {
    id: Int
    customer: String
    total: Int
    status: String
    created: String
}

fn home() WebPage -> web.page("Orders", "Track every order")
fn orders_page() WebPage -> web.page("Orders", "Pending orders")
fn order_detail() WebPage -> web.page("Order", "Ship order")
fn settings() WebPage -> web.page("Settings", "Preferences")
fn ship() { print("shipped") }

fn run() App -> {
    // proposed: the compiler derives each route mode from data and interactivity facts.
    return web.app()
        .csr()
        .route("/", home)
        .route("/orders", orders_page)
        .route("/orders/:id", order_detail)
        .route("/settings", settings)
        .action("ship", ship)
        .security("csp-default")
        .cache("revalidate")
        .a11y("wcag-aa")
        .adapter("node")
        .hydration_dev()
}
```

The application-level suite is proposed code over that existing graph:

```jet
use core.web as web
use app

// proposed: Query derives `orders.list` identity, DB.Read footprint, and cache state.
orders :: web.query("orders.list", () -> app.live("orders", "{status:pending,page:1}"))

// proposed: Form derives fields and validation from a checked shipment struct.
ship_order :: web.form("ShipOrder", Order)

// proposed: Table and Virtual preserve typed rows and render only the viewport.
orders_table :: web.table("OrdersTable", [Order]).virtual()

// proposed: Store records typed Cart transactions and exposes history to the panel.
cart :: web.store("Cart")
```

A full `jet dev` session reads like one story:

```text
$ jet dev
Orders  entry=@run.jet  tier=jet dev (JIT)  port=43127
build: ready 380ms  reload: 120ms  observe: loopback payload-free
routes: / static, /orders server+stream, /orders/:id server+island, /settings client
queries: orders.list fresh 41ms, orders.byId fresh 18ms, me stale, carriers fetching
forms: ShipOrder dirty invalid carrier: choose a carrier
tests: 12 passed 1 failed 0 skipped 640ms

$ edit src/Orders.jet
change: fn pick_carrier
build: passed
reload: module swap 120ms
kept: ShipOrder draft, Cart (3 items), scroll /orders, query cache 4/4, focus carrier
reset: none

$ curl http://127.0.0.1:43127/orders/1042
200  Order 1042  €184.00  pending

$ click Ship
runtime error: Index out of range
message: index 3 is past the end of `carriers` (length 3)
source: src/Orders.jet:91  fn pick_carrier
callers: ShipOrder.submit src/Orders.jet:64; run @run.jet:8
fix: Check `i < carriers.len()` or use `carriers.get(i)` which returns an Option.
last-good: session kept; form draft kept; query cache kept

$ jet test
12 passed  1 failed  0 skipped  640ms
FAIL ShipOrder rejects empty carrier
  tests/ShipOrder.jet:22
  expected error `choose a carrier`, got none
```

The workbench keeps the same session identity while changing the selected fact:

```text
┌─ Orders · workbench ─────────┬─ selected time ─────────────┬─ source and action ──────┐
│ Build   ready                 │ 12:05:11 reload             │ src/Orders.jet:91        │
│ Routes  4                    │ 12:05:09 mutation ship      │ pick_carrier             │
│ Queries fresh/stale/fetching │ 12:05:03 orders.list        │ Index out of range       │
│ Forms   ShipOrder dirty      │ 12:04:03 request 200         │ [go to source]           │
│ Tables  1,000 rows           │ [scrub] [pause] [resume]    │ [inspect callers]        │
│ Store   Cart history         │                             │ [copy diagnostic]        │
│ Traces Cost Gates Tests      │                             │                          │
└──────────────────────────────┴─────────────────────────────┴──────────────────────────┘
```

The terminal shows the same selected fact without requiring color:

```text
Orders  ready  JIT  43127

BUILD   passed 380ms  reload 120ms  kept=4 reset=0
ROUTES  / static | /orders server+stream | /orders/:id server+island | /settings client
QUERY   orders.list fresh 41ms | orders.byId fresh 18ms | me stale | carriers fetching
FORM    ShipOrder dirty invalid carrier: choose a carrier
TABLE   OrdersTable rows=1000 visible=24 rendered=32
TRACE   request=41ms query=38ms render=6ms mutation=220ms reload=120ms
ERROR   Index out of range  src/Orders.jet:91  pick_carrier
TEST    12 passed 1 failed 0 skipped

keys: tab panel  enter inspect  t time  r rerun  q quit
```

Today versus the proposed loop:

| today | proposed |
|---|---|
| `fn run() App` builds a checked graph with explicit CSR, island, action, mount, security, cache, accessibility, and adapter facts. | The same graph derives a mode and reason for every route, with one explicit expert override and a serializable island boundary. |
| `jet dev` uses a dependency-aware watcher, last-good artifact, status pill, and browser reload. | `jet dev` keeps compatible web state inside the module swap and shows kept or reset values in every host. |
| `jet inspect live <pid>` gives payload-free snapshots and `app.live` gives effect-tracked query invalidation. | `jet.devtools.v1` joins Build, Route, Query, Form, Table, Store, Trace, Cost, Gate, Test, Job, and runtime facts in one stream. |
| `jet jobs` lists typed jobs and canonical invocation uses `jet run <entry> -- <name>`. | `jet jobs <name>` runs one job of the resolved entry with completions and a Jobs panel, while the canonical form remains explicit and unchanged. |
| Native UI uses one `UiNode` tree with `JetBackend`, `TuiBackend`, DOM, GTK, and Null consumers. | The same public panel definition renders in browser, workbench, terminal, editor, and native overlay. |
| Web, native, game, backend, live, mobile, data, and CLI loops have different missing pieces and separate inspection habits. | Each domain has the same loop depth, panel contract, live-state law, scaffold, probe, benchmark, and proof receipt. |

## Ballots

| id | card | choice | options (one line) | recommendation | prototype path where relevant |
|---|---|---|---|---|---|
| D-DX-ENTRY1 | #2421 | Bare command entry precedence | A role file then one root function then `src/` then error; B keep canonical `run.jet` and Outputs; C the ratified override order (`@run.jet`, then one root `fn run` file, then `src/`), then the stock default (`defaults`, a sole Output, `run.jet`), then a candidate-listing diagnostic with a `jet fix` pin | C | — |
| D-DX-JOBS-UX1 | #2422 | Job invocation and discovery | A status quo plus completions; B `jet <job>`; C explicit verb from a naming menu; D `jet run <name>` after file resolution (namespace merge, typo and `.Internal` risks stated); E `jet jobs <name> [-- <args>]` as the explicit runner beside the ratified `jet run <entry> -- <name>` | E (the adversarial pass moved the recommendation off D) | — |
| D-DX-DEVTOOLS-UX1 | #2423 | Devtools UX archetype | A Dock; B Lens; C Workbench; D Pill to Lens to Workbench, with F12 (proposed) as the native overlay toggle | D | `docs/proposals/prototypes/devtools-ux/A-dock.html`, `B-lens.html`, `C-workbench.html`, `D-pill-lens-workbench.html` |
| D-DX-PLUGIN1 | #2424 | Panel and plugin registration spelling | A marker on panel function; B runtime registration call; C manifest field; D marker plus typed fact publishing with a written value form at the site, and a panel-specific release rule | D, with marker naming menu | — |
| D-DX-PROD1 | #2425 | Devtools in release builds | A always compiled out; B compiled out by default, a proposed `build.release.inspect` profile fact plus deploy-time `JET_INSPECT=1` and `JET_INSPECT_TOKEN` enable a read-only endpoint; C always present behind auth | B | — |
| D-DX-LIVE1 | #2426 | Live loop law across tiers (carries the ELI5) | A keep compatible state on every tier with recursive live-payload checks, symbol rebinding, and announced restarts; B web HMR only; C reload-only | A | — |
| D-DX-WEBARCH1 | #2427 | Web rendering model | A explicit modes; B always server-render and hydrate; C islands only; D derived rendering with resumable islands, a request-context fact that keeps personalized routes off the static path, and explicit override; E component-granularity boundaries | D | — |
| D-DX-SUITE1 | #2428 | First-party suite shape | A grow `App`; B separate installable packages sharing `core.reactive`; C `core.web` suite on one reactive primitive with built-in panels and selective imports | C | — |

All eight are full ballots: base, breadth, hybrid, and cooperative passes by the drafting agent, a fresh-agent beginner pass (`rli5`), and an adversarial pass from a rival model family. The adversarial passes changed real content: entry precedence keeps the ratified override order, jobs get an explicit runner instead of bare-name resolution, release devtools use a profile fact plus a deploy switch, and derived rendering gained the request-context fact.

## Cards

Cards use the phase and dependency rules in OUTLINE.md. Core cards retain their Tower numbers. Every card number is live on Tower under epoch e14.

### e14-m01-entry-and-commands

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2421 | Bare `jet run`, `dev`, `build`, and `test` pick one entry by one ratified rule | deciding | D-DX-ENTRY1 | Bare command precedence, candidate diagnostics, explicit defaults, and one `fn <cmd>` per project |
| #2422 | Jobs runnable by name with discovery, completions, and a devtools panel | deciding | D-DX-JOBS-UX1 | Typed job discovery, shell completion, job metadata, and a Jobs panel |

### e14-m02-devtools-core

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2423 | Devtools UX archetype: how developers see their running program on every host | deciding | D-DX-DEVTOOLS-UX1 | Floating host, in-app surface, lens, workbench, terminal view, shared selection, shared time cursor |
| #2424 | Public typed devtools panel API: how a package registers a panel and publishes facts | deciding | D-DX-PLUGIN1 | Typed panel registration, bounded panes, typed Custom events, release stripping, explicit value publication |
| #2425 | Devtools presence in release builds: compiled out, opt-in protected endpoint, or always on | deciding | D-DX-PROD1 | Production no-op, explicit endpoint, authentication, allowlist, and payload privacy |
| #2429 | `jet.devtools.v1`: one observation protocol in the Prelude feeding every devtools host | ready | — | Typed event stream, query surface, time cursor, bounded history, loopback privacy, and tier parity |
| #2492 | First-party devtools panels: Build, Routes, Queries, Mutations, Forms, Table, Store, Traces, Cost, Gates, Structure, Tests, Jobs, UI tree | ready | #2423, #2424, #2429 | First-party panels for all listed event families and source identity |
| #2493 | Browser host: in-app surface and workbench page served by `jet dev` | ready | #2423 | Browser surface, workbench route, shared selection, time cursor, six states, source maps, and element-to-source handoff |
| #2494 | Terminal host: `jet dev` renders the same panels with `TuiBackend` | ready | #2423 | Terminal panels, keyboard paths, `NO_COLOR`, ordered errors, and job switching |
| #2495 | Editor host: open the workbench from VS Code and Zed through the language server | ready | #2423 | LSP workbench request, loopback webview, and diagnostic/test/build handoff |
| #2496 | Native overlay host: GUI and game windows render the panels with their own `JetBackend` | ready | #2423 | Native overlay, toggle key, and release exclusion |
| #2497 | Devtools built in Jet: dogfood anchor 3 friction ledger | ready | #2423 | Devtools authoring friction, package extension, and dogfood evidence |

### e14-m03-live-loop

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2426 | Live loop law: an edit keeps every value whose type is unchanged, on every tier | deciding | D-DX-LIVE1 | Cross-tier compatible edit preservation, reset reasons, kept/reset lists, and state identity |
| #2465 | Web module swap: preserve signals, stores, query cache, form drafts, scroll, and focus | ready | #2426 | Browser module HMR and preserved application state |
| #2466 | Native swap hardening: servers keep listeners and connections; `jet explain --reload` | ready | #2426 | Resident server state, open connections, layout restart reasons, and reload explanation |

### e14-m04-web-suite

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2427 | Rendering is a compiler fact: derived per-route rendering with resumable islands | deciding | D-DX-WEBARCH1 | Derived static, server, stream, island, client, serialization, and route reason facts |
| #2428 | First-party web suite: Router, Query, Forms, Table and Virtual, Store on one reactive primitive | deciding | D-DX-SUITE1 | Typed params and search, loaders, queries, mutations, forms, validation, tables, virtualization, and store history |
| #2472 | Router: typed params, typed search params, loaders with preload and abort, pending and error boundaries | planning | #2428 | Inferred typed route tree, typed URL state, loaders, preload, abort, pending, and error boundaries |
| #2473 | Query: live queries by default, explicit keys, mutation lifecycle with optimistic rollback, targeted invalidation, cache states | planning | #2428 | Query cache, effect-tracked invalidation, explicit keys, optimistic rollback, and freshness states |
| #2474 | Forms: fields derived from struct types, sync and async validation, progressive enhancement through `action` | planning | #2428 | Typed fields, blur and submit validation, async checks, and script-free action submission |
| #2475 | Table and Virtual: headless typed table, server pagination, viewport virtualization | planning | #2428 | Typed table state, pagination, sorting, filtering, and visible-row rendering |
| #2476 | Store: signals plus transactions with history | planning | #2428 | Signal-backed store, transaction diffs, time-travel history, and reload retention |
| #2477 | Server functions and progressive actions: typed RPC boundary with serializability checks and CSRF policy | planning | #2428 | Typed server functions, serialization diagnostics, auth, CSRF, and progressive actions |
| #2478 | Streaming and pending boundaries, islands with explicit hydration timing | ready | #2427 | Stream pending UI, island boundaries, hydration timing, and resumable state |
| #2479 | Reference app: rebuild the TanStack Start example in Jet with measured lines of code and edit loop | ready | #2428 | Full route/data/write reference loop and benchmark evidence |

| #2498 | Browser tests: scaffold, UI mode, and trace viewer on the native Browser automation | planning | D-DX-BROWSERTEST1 | Browser test scaffold, cross-browser contexts, UI tree, filters, timeline, DOM snapshots, retries, reports, traces, and codegen |

### e14-m05-games

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2484 | Game play loop in `jet dev`: run, pause, frame advance, eject and keep changes | planning | — | Play, pause, frame advance, eject, keep or discard, and live process control |
| #2485 | Game overlay: in-game console, stat overlays, output categories with error pause | ready | #2423 | Native game overlay, console, categorized output, and error pause |
| #2486 | Live world inspector: remote scene tree, typed property editing with transient versus authored status, evaluator against a paused frame | ready | #2429 | Remote scene tree, property editing, authored-state boundary, paused evaluator |
| #2487 | Frame profiler and frame debugger: frame timing to responsible function, draw-event stepping, trace server on `.jettrace` | ready | #2429 | Frame timing, draw-event stepping, source identity, and trace artifact |
| #2488 | Asset pipeline: watch declared asset roots, reimport with dependency status, hot asset reload | ready | — | Asset roots, dependency status, reimport, and asset hot reload |
| #2489 | Game hot swap: world kept through `#Persist`, scripts synchronized, link mode for fast iteration | ready | #2426 | World persistence, script synchronization, link mode, and unsupported-edit reasons |
| #2490 | Game packaging: build, cook, stage, package, export presets, build profiles with receipts | planning | — | Cook, stage, package, export presets, profiles, receipts, and dev stripping |
| #2491 | `jet new game`: runnable scene, dev profile, diagnostics registry, remote protocol | ready | — | Game scaffold, runnable scene, profile, diagnostics, and remote protocol |

### e14-m06-backend

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2451 | Request, exception, and query panels: timeline, route inputs, response, SQL timing, source frames | ready | #2423, #2429 | Request timeline, route inputs, responses, SQL timing, exceptions, and source frames |
| #2452 | Jobs and queues panels: enqueue, start, retry, failure, duration, throughput, workers | ready | #2429 | Job lifecycle, queue state, retry, failure, throughput, and workers |
| #2453 | Metrics, logs, and traces panels: named instruments, structured log query, distributed trace with linked logs | ready | #2429 | Named metrics, structured logs, distributed traces, linked request identity |
| #2454 | Service topology and process panel: supervision tree, task states, readiness, endpoints | ready | #2429 | Service topology, process states, readiness, endpoints, and supervision tree |
| #2455 | Actionable local error page: HTML and JSON projections with request id, source frame, and safe context | ready | — | Local HTML/JSON errors, request id, source frame, safe context, and runtime-failure product |
| #2456 | Migrations: generate, preview SQL, status, apply, rollback with target and lock semantics | planning | — | Migration generation, SQL preview, status, apply, rollback, target, and locks |
| #2457 | Durable job queue: database-backed default store, delayed and bulk enqueue, retries, status | planning | — | Durable queue store, delayed and bulk enqueue, retries, and status |
| #2458 | Backend generators: `jet new service`, route, job, migration | planning | — | Typed service, route, job, migration generators and vertical slices |
| #2459 | Capability-gated console with in-process requests and reversible data sandbox | planning | — | Safe console, in-process request replay, reversible data sandbox, and authority gate |
| #2460 | Database stats panel with opt-in credentials and safe EXPLAIN | ready | #2429 | Database stats, opt-in credentials, safe EXPLAIN, and bounded sampling |

### e14-m07-live-timetravel

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2461 | Record a dev session and replay it with time travel | ready | #2429 | Session recording, replay, scrub, jump, skip, divergence, and build identity |
| #2462 | Inspect and evaluate in a paused session with authority checks | ready | #2429 | Paused inspection, typed evaluation, large values, authority, and retention rules |
| #2463 | Reverse step, watchpoints, and fix-and-continue in `jet debug` | planning | — | Reverse execution, watchpoints, source repair, and continue policy |
| #2464 | Named previews and a playground: source-defined live scenarios beside the editor | planning | — | Named previews, sample data, playground evaluation, and source identity |

### e14-m08-systems-toolchain

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2439 | Test loop: watch mode with failed-first rerun, output capture control, cache bypass, filter expressions, doctests in the normal loop | planning | — | Persistent test watch, failure focus, output capture, cache bypass, filters, and doctests |
| #2440 | Diagnostics rendering parity measured against rustc, miette, Zig, and Go | ready | — | Labels, notes, help, JSON, applicability, links, and cross-tool diagnostic measurements |
| #2441 | Job graph: dependencies, freshness, bounded parallel runs on `#Job` metadata | planning | #2422 | Task dependencies, freshness, bounded parallelism, and job metadata |
| #2442 | Explicit source generation loop with audit trail | planning | — | Explicit generators, source audit trail, authority, and reproducibility |

### e14-m09-mobile

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2443 | Mobile dev loop design: doctor, device inventory, one-command run, reload semantics, inspector, structured logs | planning | #480 | Doctor, device inventory, run, reload, inspector, structured logs, and mobile session identity |
| #2444 | Mobile scaffold, build profiles, signing, and destinations | planning | #480 | Mobile scaffold, target, signing, generated native output, profiles, and destinations |

### e14-m10-data-notebooks

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2445 | Reactive notebook: dependent-cell rerun, typed controls, rich outputs, delete-cell scrubbing | planning | — | Cell dependency rerun, typed controls, rich outputs, deletion, and stale descendants |
| #2446 | Run a notebook or typed function as a local app | planning | — | Notebook-to-app launch, per-client session, local server, and app publication |
| #2447 | Lazy table plans on every tier with plan and streaming inspector | ready | — | Lazy table execution, plan explanation, streaming, and tier parity |
| #2448 | Data loaders with deterministic snapshots and invalidation | planning | — | Data loaders, snapshots, invalidation, and reproducibility |
| #2449 | Plot grammar over typed tables | planning | — | Typed plot marks, scales, facets, layers, and deterministic output |
| #2450 | Local SQL console over typed tables and files | planning | — | Local SQL, table and file access, query output, and safe authority |

### e14-m11-cli-tui

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2467 | First-party TUI kit on `core.ui` and `TuiBackend`: model, update, view loop, widgets, constraint layout, capability-aware styles | planning | — | Model/update/view, widgets, geometry, styles, capabilities, and reusable TUI state |
| #2468 | Guided input: typed prompts and forms with validation and accessible non-TTY mode | planning | — | Typed prompts, validation, accessible mode, pipe mode, and non-TTY output |
| #2469 | Human output primitives: capability detection, adaptive tables, progress, and a strict machine-output separation | ready | — | Adaptive tables, progress, terminal detection, JSON separation, and `NO_COLOR` |
| #2470 | TUI tests and recordings: headless frame snapshots, interaction driver, in-process command tests, tape recordings | ready | — | Headless frames, interaction driver, command tests, recordings, and deterministic artifacts |
| #2471 | Dynamic completions for typed values and paths | ready | #2422 | Typed value completion, path completion, and safe job completion |
| #2480 | Style live reload for terminal apps | ready | #2426 | Terminal style reload, state retention, and explicit restart fallback |

### e14-m12-proof

| number | card title | phase | blocked by | the census features it closes |
|---|---|---|---|---|
| #2481 | DX benchmark matrix: time to first run, edit to see, error to fix, test loop, build, against each ecosystem's best loop | ready | — | Cross-domain benchmark matrix, peer identity, warm/cold paths, and receipts |
| #2482 | Fresh-agent cold-start run per domain | ready | — | Cold-agent first run, first edit, first repair, and first useful panel evidence |
| #2483 | Owner visual acceptance of devtools and dev loop on real programs | ready (owner visual) | #2423, #2492, #2493, #2494, #2495, #2496 | Pill, lens, workbench, terminal, editor, native overlay, six states, and visual quality |
| #2499 | Prior-art registry: record the nine DX census sources | ready | — | Durable provenance for all nine manifests, source IDs, URLs, and census claims |

## Sources

The nine census sources are registered by the prior-art card and listed here with their reports and census files.

- `~/.cache/jet-luna/dx/web-tooling/report.md`; `~/.cache/jet-luna/dx/web-tooling/census.json`; `~/.cache/jet-luna/dx/web-tooling/manifest.json`
- `~/.cache/jet-luna/dx/web-frameworks/report.md`; `~/.cache/jet-luna/dx/web-frameworks/census.json`
- `~/.cache/jet-luna/dx/live/report.md`; `~/.cache/jet-luna/dx/live/census.json`
- `~/.cache/jet-luna/dx/games/report.md`; `~/.cache/jet-luna/dx/games/census.json`
- `~/.cache/jet-luna/dx/backend/report.md`; `~/.cache/jet-luna/dx/backend/census.json`
- `~/.cache/jet-luna/dx/systems/report.md`; `~/.cache/jet-luna/dx/systems/census.json`
- `~/.cache/jet-luna/dx/mobile/report.md`; `~/.cache/jet-luna/dx/mobile/census.json`
- `~/.cache/jet-luna/dx/data/report.md`; `~/.cache/jet-luna/dx/data/census.json`
- `~/.cache/jet-luna/dx/cli/report.md`; `~/.cache/jet-luna/dx/cli/census.json`
- `docs/proposals/prototypes/devtools-ux/A-dock.html`
- `docs/proposals/prototypes/devtools-ux/B-lens.html`
- `docs/proposals/prototypes/devtools-ux/C-workbench.html`
- `docs/proposals/prototypes/devtools-ux/D-pill-lens-workbench.html`
- `~/.cache/jet-luna/dx/proto/sample-data.json`
