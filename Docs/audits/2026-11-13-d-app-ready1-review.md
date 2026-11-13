# D-APP-READY1 adversarial design review (2026-11-13)

Card #3078 c10, first half: "Ballot a readiness hook versus explicit
pre-serve composition." The draft ballot D-APP-READY1 offers three options:
A, plain code before `app.serve()` or `app.bind()`; B, an
`app.before_serve(step)` hook; C, `app.settings<T>()` decoded by serve. It
recommends A, with no new API. This review attacks each strength the draft
claims. It compares them against the strongest existing readiness models
(Kubernetes probes, systemd notify and socket activation, Go `net/http`,
Axum, Phoenix, Spring Boot and ASP.NET Core) and checks them against the code
on the sweep2 stage-B base `cb2cf53397` and a probe. It ends with a verdict,
and every gap goes to a card, a criterion or a ballot.

## Measured facts

Code reads are against `cb2cf53397`. Probe P1 ran the seed compiler
`seed-b15` (`jet@a0.0.45#1b363853`) with `JET_TOOLCHAIN_ROOT` set to a
checkout of tree `65db32de3a` plus the Core files that seed needs, and
`NO_COLOR=1`. Outputs are quoted verbatim.

| # | Read or probe | Observed |
|---|---|---|
| R1 | Tower `D-VERDICT-3086-1` | Ratified A on 2026-10-04 (owner chat ruling of 2026-10-01, ask question s3086): "Framework checks before serve. The app declares one typed settings record. JetApp.serve decodes and checks it before it binds the port." Its rejected option B was "App calls env.decode itself ... An app that skips the step starts with missing settings", rejected because "B depends on every author remembering one step, and a forgotten step only shows up in production". Its detail says the work rides on #3078. |
| R2 | Tower `D-ONE-LIFECYCLE1` | Ratified 2026-09-13. Its detail scopes it to sharing the interpreter host registration across run, hot swap and restart: "The concrete consolidation is interpreter registration only." Its "no new type, public method, executor, resource category" line authorizes nothing new for that consolidation; it does not mention web.app, settings or serve. |
| R3 | `Core/Web/Web.jet:418,490-500,516-526` | `serve(port)` and `serve_on(port)` call `__app_listen`, which listens on `127.0.0.1:{port}` only, then prints `serving http://127.0.0.1:PORT`. |
| R4 | `Core/Web/Web.jet:472-477` | `__app_port(fallback)` returns `JET_APP_PORT` whenever it holds a valid port, and the argument only otherwise. An invalid `JET_APP_PORT` silently falls back. |
| R5 | `Core/Web/Web.jet:425-427`, `Core/Http/Server.jet:292,363,530` | `bind(address)` opens the listener and returns `HTTPServer`; `serve()` answers, and `shutdown(grace)` drains. Between `bind` and `serve` the kernel accepts connections into the backlog. |
| R6 | `Core/Sys/Sys.jet:2145-2158` and `config_entries` | The secret rule hides the reason when a field path segment, split on `.`, `[` and `]`, is exactly `secret`, `password`, `token` or `key`. Env names split into segments on `__`, so `APP_DB_PASSWORD` is the single segment `db_password`. |
| R7 | `Examples/Systems/CLI/AppConfig.jet:22-26,54`, `.out:5` | The example keeps its password in `#Redact password: crypto.Secret` under `APP_DB__PASSWORD` (a nested `db.password`). A failure prints as `failure: [at \`port\`: E2416: env var APP_PORT -> port: expected Int, found text "not-an-int"]`. |
| P1 | `redact.jet`, below, `jet run` | Every bad field is reported (four rows). `row: E2416: config field db_url: E2410: missing required field \`db_url\``; `row: E2416: env var APP_DB_PASSWORD -> db_password: expected Int, found text "hunter2"`; `row: E2416: env var APP_API_KEY -> api_key: expected Int, found text "sk-live-123"`; `row: E2416: env var APP_PASSWORD -> password: secret value rejected`. The `?? panic("not started: {err}")` line then stops with `Stop [E3001]: \`panic: not started: [at \`db_url\`: …]\``, whose Why reads "an unhandled entry error is a returned report, not E3001", and the process exits 70. |

P1:

```jet
use {
    env: core.sys
}

#[Debug, Decode]
Settings: struct {
    port: Int
    db_url: String
    db_password: Int
    api_key: Int
    password: Int
}

run: fn() {
    env.unset("APP_DB_URL") ?? false
    env.set("APP_PORT", "8080")
    env.set("APP_DB_PASSWORD", "hunter2")
    env.set("APP_API_KEY", "sk-live-123")
    env.set("APP_PASSWORD", "swordfish")
    names: ["APP_PORT", "APP_DB_URL", "APP_DB_PASSWORD", "APP_API_KEY", "APP_PASSWORD"]
    if env.decode<Settings>(prefix: "APP_", file: "", allow: names) == {
        .Ok(_) -> { print("accepted") }
        .Err(errors) -> { loop error in errors -> print("row: {error.reason}") }
    }
    settings: env.decode<Settings>(prefix: "APP_", file: "", allow: names) ?? panic("not started: {err}")
    print("unreachable {settings.port}")
}
```

## The strongest existing models

- **Kubernetes** separates three probes: a startup probe, a readiness probe
  that takes a pod out of its Service's endpoints while it fails, and a
  liveness probe that restarts it. Readiness is a state that changes while
  the process runs: it starts false, turns true, and turns false again on
  overload or shutdown. A `tcpSocket` readiness probe treats an open port as
  ready, so an app that listens before it can answer is ready too early.
  Source: kubernetes.io/docs/concepts/configuration/liveness-readiness-startup-probes/.
- **systemd** `Type=notify`: the service says `READY=1` through
  `sd_notify` when it can serve and `STOPPING=1` when it drains. With socket
  activation systemd owns the listening socket and queues connections until
  the service takes them, so the port is open before the program runs, by
  design. Sources: freedesktop.org/software/systemd/man/latest/sd_notify.html,
  .../systemd.socket.html.
- **Go `net/http`**: settings and resources are plain code before
  `ListenAndServe`, the shape of A. `net.Listen` and `Server.Serve` split
  bind from serve, and `Server.Shutdown` drains. Readiness endpoints are
  hand-written. Source: pkg.go.dev/net/http.
- **Axum**: plain code builds the state, `TcpListener::bind` opens the
  port, and `axum::serve(listener, app).with_graceful_shutdown(..)` serves
  and drains. `Router::with_state(state)` hands typed state to the
  handlers, and a handler that asks for `State<T>` with the wrong `T` fails
  to compile. So typed app-held settings need no run-time check, unlike C as
  drafted. Sources: docs.rs/axum/latest/axum/fn.serve.html,
  docs.rs/axum/latest/axum/extract/struct.State.html.
- **Phoenix**: `config/runtime.exs` runs before the application starts, and
  `System.fetch_env!` raises on a missing variable. The supervision tree
  starts its children in order, with the Repo before the Endpoint, so the
  port opens only after the pool is up. On shutdown the Endpoint's drainer
  stops admission before the Repo closes. The framework owns the order and no
  author writes it. Sources: hexdocs.pm/phoenix/deployment.html,
  hexdocs.pm/elixir/Supervisor.html, hexdocs.pm/plug_cowboy/Plug.Cowboy.Drainer.html.
- **Spring Boot**: `@ConfigurationProperties` with `@Validated` fails startup
  with every bad field named. `ApplicationAvailability` publishes
  `ReadinessState.ACCEPTING_TRAFFIC` after the runners finish, and
  `REFUSING_TRAFFIC` when graceful shutdown starts. Actuator serves both as
  probe groups. Sources: docs.spring.io/spring-boot/reference/features/external-config.html,
  docs.spring.io/spring-boot/reference/features/spring-application.html.
- **ASP.NET Core**: `services.AddOptions<T>().ValidateDataAnnotations()
  .ValidateOnStart()` checks typed options before the host starts.
  Health checks tagged `ready` back the readiness endpoint. Source:
  learn.microsoft.com/en-us/dotnet/core/extensions/options.

What they do that A does not: Phoenix, Spring, ASP.NET and NestJS run the
settings check themselves, so an app cannot skip it; Axum hands typed state
to handlers with a compile-time check; Kubernetes, systemd and Spring treat
readiness as an observable state that flips back on shutdown. Go is the one
strong system that matches A, and it pays for that with a hand-written check
and readiness endpoint in every service.

## Claims and attacks

### 1. "No new surface is needed; code before serve is the readiness step"

Fails. The owner already decided this question. D-VERDICT-3086-1 (R1),
ratified 2026-10-04, chose "Framework checks before serve" and rejected
"App calls env.decode itself", which is the draft's option A in substance,
because "a forgotten step only shows up in production". Its detail puts the
work on #3078, the card c10 belongs to. The draft never names it. Under
AGENTS.md a ballot that re-proposes a rejected option without stating the
ruling and the new evidence that reopens it is missing context, and missing
context is a failed claim.

The draft's own story shows the gap the owner closed: it follows Ana, who
writes the decode line and forgets one variable. The 3086 story follows Sam,
who never writes the step. Under A, Sam's app starts and fails on its first
request, which is the case c1 ("Not ready before required resources are
usable") forbids. The draft's losses list "each app writes its own decode
line" but not "an app that writes none starts anyway".

### 2. "A stays inside D-ONE-LIFECYCLE1=A; B and C each need this vote"

Fails. D-ONE-LIFECYCLE1 (R2) is the interpreter-registration consolidation,
ratified three weeks before D-VERDICT-3086-1. Its "no new public method" line
limits that consolidation and says nothing about web.app. The later ruling
(R1) explicitly adds a settings parameter to serve. Read together, the
framework hook is already authorized, and A is the option that departs from
a ratified ruling.

### 3. "app.serve() opens the port only when called, so a program that serves with it cannot hit the gap"

Holds only within stated limits. `serve` listens on 127.0.0.1 only (R3), so
it cannot serve a container, a pod or a load balancer. Every deployed app
goes through `bind(address)`, where the decode-before-bind order is guide
text only (R5). The mitigation covers the local path and leaves the
production one uncovered.

### 4. "The port can come from the settings"

Holds only within stated limits. `serve(settings.port)` is ignored whenever
`JET_APP_PORT` holds a valid port, and an invalid `JET_APP_PORT` is silently
dropped (R4). An app under A therefore has two port sources with an unstated
precedence, and its typed `port` field is not validated as the port the app
uses.

### 5. "A bad setting stops with E2416, the field named and any secret hidden"

Fails as drafted. The secret rule matches whole path segments (R6), and env
names split on `__`, so the draft's golden plan (APP_DB_PASSWORD whose reason
"reads secret value rejected") does not hold on this code: `db_password` is
one segment. `api_key` is not hidden either. P1 confirms it: the values
`hunter2` and `sk-live-123` print in full, and only the field named exactly
`password` reads "secret value rejected". The draft's story fails too: a
missing APP_DB_URL prints `config field db_url: E2410: missing required
field`, which does not name the variable the draft says it names. The repo's own
example (R7) keeps its secret under a nested `db.password` with the
`crypto.Secret` carrier, which is a type-driven rule; the name rule is a
heuristic. This is shared by all three options, since each calls env.decode,
but the draft's golden is specified on it.

### 6. "Under A the run stops on the decode line, prints the E2416 line and never opens a port"

Holds only within stated limits. The draft's line is
`?? panic("not started: {err}")`. A panic is the bug channel, and a missing
setting is an expected operator error. P1: the line stops with E3001, the
program-side stop, exit 70, and E3001's own Why says an unhandled entry
error should be a returned report instead (exit 1). Supervisors classify
by exit status (systemd `RestartPreventExitStatus=`, the sysexits
`EX_CONFIG` 78 convention, Kubernetes `CrashLoopBackOff`), and every app
picks its own text under A. Under the ratified framework check, serve stops
once, through a coded stop with one registered code, the same in every app.

### 7. "Readiness"

Holds only within stated limits. The draft decides start-up ordering and calls
it readiness. Card #3078 asks for readiness and drain: c1 not ready before
resources are usable, c2 shutdown stops admission before resources close. In
every model above, readiness is an observable state that a probe or a
supervisor reads and that turns false on shutdown. Jet's only signal is the
stdout ready line (R3). None of A, B or C gives a probe or a supervisor a
readiness state, and none flips it on shutdown. The ballot may leave that out
of scope, but it must say so and point at c1 and c2, not answer "readiness"
with ordering alone.

### 8. "Tests build the app with fakes by calling an ordinary function"

Holds. This is the Go and Axum pattern. It is not exclusive to A: a framework
check with typed settings composes the same way when the test passes the
settings value directly.

### 9. The rejected options

B and C are drafted at their weakest. B's "shared cell" and "fixed effect
row" come from choosing a hook that runs after the routes are built. C's
"run-time type check through Dyn" comes from storing settings untyped; Axum's
`with_state` shows typed app-held state checked at compile time. The ratified
shape, a typed settings record that serve decodes before it binds (R1), is
not on the ballot at all. Neither is the hybrid the strongest systems use:
the framework validates the record, and the program still receives it as a
plain typed value for its pool and its routes, as Spring's
`@ConfigurationProperties` bean or ASP.NET's `IOptions<T>` do.

## Verdict

**Fails.** Claims 1, 2 and 5 fail; claims 3, 4, 6 and 7 hold only within
stated limits; claim 8 holds. The draft must not go to the owner as written.

Gaps and where they go:

- Claim 1 and claim 2: the question is already ratified (D-VERDICT-3086-1=A,
  framework checks before serve). #3078 c10's ballot half should cite that
  ruling, and the work should build it: serve and bind decode one typed
  settings record before they open the port, and hand it to the program as a
  typed value. A new ballot is needed only to reopen it, stating the ruling
  and the new evidence. Owner of the draft: G12Docs-5.
- Claim 3: `serve` is local-only, so the production path is `bind`. Whatever
  check is built must run in `bind` too, before the listener opens. Card
  #3078 c10.
- Claim 4: one documented precedence between `JET_APP_PORT`, the serve
  argument and a settings `port` field, with an invalid `JET_APP_PORT`
  reported instead of dropped. Card #3078 c10.
- Claim 5: redaction keyed on exact segment names misses `db_password` and
  `api_key`. Either the rule matches a suffix segment of a snake-case name
  or it is driven by `#Redact` and `crypto.Secret` alone. Needs a card on the
  env.decode owner; the `web/app_config_readiness` golden must use a field the
  rule actually hides. A missing field should name the variable it reads
  (`env var APP_DB_URL`), as a bad one does.
- Claim 6: a settings failure stops through one registered coded stop with a
  distinct exit status, not a panic. Card #3078 c10.
- Claim 7: an observable readiness state that turns false on shutdown before
  drain. Card #3078 c1 and c2.
