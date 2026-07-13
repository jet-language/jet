# Framework lessons — transplants, not surveys (2026-07-11, v2 deep pass)

Owner ask: mine the best software — languages, tools, services, systems,
libraries — for what Jet should absorb. Each row is a concrete transplant
into ratified Jet machinery with worked code, or an explicit
"already have it" / "rejected because". Every transplant carries a
**placement** verdict: core language (new checked semantics), tooling
(a jet verb / dev-loop surface), or core lib (a `core.*` module). Big
ideas are ballots; everything else is recorded so nobody re-derives it.

## Placement law (how we decide where a feature lands)

- **Core language** only when the compiler must *prove* something new
  (an effect, a pattern form, a transaction boundary). Highest bar.
- **Core lib** when the feature is types + functions the checker already
  handles; magic comes from safe defaults, not new grammar.
- **Tooling** when the value is workflow, not semantics.
One mechanism per job (I8): a transplant that duplicates a ratified
mechanism is folded into it or rejected.

## Convex — the big one

Convex's magic is one loop: server functions are **deterministic**
queries whose **read set is tracked**, so the platform knows exactly
when any client's query result changed and pushes the update. Mutations
are transactions retried on conflict. No cache invalidation, no
websocket plumbing, no stale UI — by construction.

Jet already owns every ingredient Convex had to build a platform for:
effect rows prove a query touches only `Db.Read` (D-EFF1/D-EFFTREE1),
`@Pure` proves determinism modulo tracked reads (S60, D-DET1),
`#Transact` is the transaction story (D-TXN1–4), `core.reactive` is the
signal graph (D-REACT1), the app graph is sema-known (D-WEBAPP1), and
`core.ws` is the push channel (D-WS1). Nobody has shipped this as a
*language* feature. Transplant:

```jet
// server: an ordinary fn — #(Db.Read) + purity make it a live query
#(Db.Read)
@Pure fn open_tickets(team: Id<Team>) -> [Ticket] {
    return db.query("SELECT * FROM tickets WHERE team = {team} AND open")?
}

fn close(t: Id<Ticket>) {
    #Transact(tx) { db.exec("UPDATE tickets SET open = false WHERE id = {t}")? }
}

// client: subscribing yields a Signal that re-renders the view (D-REACT1)
tickets :: app.live(open_tickets, team)     // Signal<[Ticket]>
```

The compiler records each live query's read footprint from its effect
row + bound parameters; a committed `#Transact` whose write set
intersects a footprint invalidates exactly those subscriptions over
`core.ws`. **Ballot D-LIVEQUERY1.** Follow-on (not balloted yet):
typed optimistic updates derived from the mutation's write set —
Linear-class UI latency for free.

## Scheduling as code (Convex crons, Temporal)

Convex lets code declare its own schedule next to the function. Jet has
runtime timers (`tasks.interval`, D-TASKRUNTIME1) and jetos service
timers, but no way for an app to *declare* "run this every night".
Transplant — one marker on the already-ratified task surface
(D-JPK-TASKRUN1), consumed by `jet dev`, services (D-SERVICE1), and
jetos timers from one declaration:

```jet
#Task #Every("03:00")           // or #Every(5min) with D-UNITLIT1 literals
fn prune_sessions() { db.exec("DELETE FROM sessions WHERE expired")? }
```

**Ballot D-SCHEDULE1.**

## "We are not their parents" — expert override with enterprise audit

Owner principle (2026-07-11): experts may work dangerously — ignore
warnings, effects, determinism — without babying; enterprises still need
enforcement and audit. Jet's escape hatches already exist but each was
minted ad-hoc: `#Unsafe("reason")`, `--try-anyway`, `--allow-impure`,
`--allow-<root>`, `assume_deterministic { }`, `#[allow(lint)]`. What is
missing is the **law** that keeps future gates consistent and gives
teams the enforcement knob:

1. Warnings and lints never block a build by default — the tool advises,
   the human decides.
2. Every bypass is spelled in source or on the command line, never in
   hidden config, and every bypass is a recorded fact (audit output,
   `jet inspect dossier`, effect budget provenance).
3. A team that wants walls opts in via the one policy surface
   (`policy:` — D-JPK-POLICYSURFACE1): `policy.lints: { deny: [...] }`,
   effect budgets (D-EFFBUDGET1), trust grants. Host/org policy can
   narrow, never widen (already law).

```jet
// pkg.jet — the team's choice, not the compiler's default
policy: {
    lints: { deny: [float_money, unused_result] },
    effects: { deny: [Exec] },
}
```

```shell
$ jet build --allow-impure          # solo expert: fine, recorded
$ jet build                          # CI with the policy above:
error[E13xx]: lint `float_money` is denied by policy (pkg.jet:12)
```

**Ballot D-LINTPOLICY1** (codifies 1–3 as one law; unifies future gate
design the way D-MARKER-FAMILY1 unified sigils).

## Already have it (recorded so nobody re-proposes)

- **Elm/Rust error voice** → Jet's diagnostics ARE the product (I4).
- **Deno permission flags** → effect system + `--allow-<root>` (deeper:
  per-callee, not per-process).
- **Temporal durable workflows** → D-SERVICE-WORKFLOW1 (ratified).
- **Terraform plan/apply** → jetos plan/diff/proof (D-WD8), D-FE-CLI1
  plan-first mutations.
- **Tailscale zero-config identity** → D-SERVICE-IDENTITY1 signed
  directories.
- **Vite instant feedback** → `jet dev` interpreter/JIT tiers (<200ms
  budget, D-DEV3).
- **Stripe API versioning** → `@PublishedSchema` + migration verbs
  (D-MIGRATE1/2), decode-time migration transparency (D-MIGRATE3/4).
- **SQLite embedded-first reliability** → `core.db` SQLite default
  (D-DEP-DB1); test rigor lives in the golden/differential CI ethos.
- **Rails scaffolding** → `jet new --annotated`; richer templates are a
  tooling card, not a language change.
- **Unison content-addressed code** → considered and frozen (D-CADEFS1);
  do not re-open without owner.
- **Phoenix LiveView server-driven UI** → belongs to the full-stack web
  card (#438, D-WEBAPP1); D-LIVEQUERY1 supplies its data layer.
- **Jupyter/Observable notebooks** → D-NOTEBOOK-* (ratified 2026-07-10).

## v2 deep pass — new transplants (each a ballot on card #506)

### Erlang bit syntax → binary patterns — D-BINPAT1 (core language)

Erlang parses network protocols declaratively: `<<Version:4, IHL:4,
TOS:8, Len:16>>` destructures bits. Jet has the cursor surface
(`Reader.over`, D-SHIFT1) and the one pattern engine (D-PARSESTR1) but
no declarative binary shape. Transplant — binary patterns as the byte
sibling of interpolation patterns, same engine, same arms:

```jet
// pattern position, if-table arm or ==-test — holes are bit-typed
if packet == b"{version:U4}{ihl:U4}{tos:U8}{len:U16be}{rest:...}" {
    print("IPv{version}, {len} bytes")
}
// consume mode mirrors Cursor.take_pattern (D-SHIFT1):
r :: Reader.over(bytes)
(version, ihl) :: r.take_pattern(b"{version:U4}{ihl:U4}")?
```

Placement: language (pattern grammar + exhaustive checking). Erlang
proved this is THE ergonomic win for protocol/parser work — a
master-of-all systems language needs it.

### Haskell STM → composable atomic memory transactions — D-STM1 (core language)

`#Transact` rolls back locals on `?`-failure (single-task). `Shared<T>`
gives lock-scoped closures (single handle). Nothing composes an atomic
step across TWO shared handles — the classic bank transfer deadlocks or
races in every lock-based language; Haskell's STM solved it. Transplant:
`#Transact` gains the concurrency plane when its block touches
`Shared<T>` handles — reads/writes inside become one atomic commit,
retried on conflict (the D-SERVICE-DELIVERY retry discipline, in-memory):

```jet
#Transact(tx) {
    from.edit((b) => b.balance -= 100)   // both commit, or neither —
    to.edit((b) => b.balance += 100)     // no lock ordering to get wrong
}
```

One mechanism (I8): same marker, new proof (sema rejects irreversible
effects inside, exactly as it already does — E0746 covers it).

### Firebase/Clerk auth → `core.auth` batteries — D-AUTH1 (core lib)

Every web framework makes auth the user's problem; every batteries
platform (Firebase, Supabase, Clerk) made it magic and won beginners.
Jet's web stack (D-WEBAPP1, D-HTTPDEPTH1, core.crypto) has no auth
story. Transplant: `core.auth` — sessions (cookie, signed, rotating),
password login (argon2 via crypto suite), OAuth/OIDC client, email
magic-links, JWT/PASETO verification for APIs; safe defaults
(httponly/secure/samesite, constant-time compares), expert control over
every knob; integrates the effect/taint law (`.Credential` taint kind
exists).

```jet
auth :: app.auth(users: db)               // magic default: sessions + password
app.mount("/login", auth.routes())
fn dashboard(req: Request) -> Response {
    user :: auth.user(req) ?? return Response.redirect("/login")
    ...
}
```

### Figma/Linear sync → CRDT value types — D-SYNC1 (core lib)

Multiplayer/offline-first is table stakes for app platforms; everyone
hand-rolls CRDTs or buys them. Jet has live queries (D-LIVEQUERY1,
server push) but no conflict-free client merge. Transplant: `core.sync`
— CRDT value types (`SyncText`, `SyncMap<K,V>`, `SyncList<T>`,
`SyncCounter`) that are `@Codable`, merge deterministically, and ride
the live-query channel; structural-merge card #143 supplies the
semantic-merge substrate.

```jet
doc :: SyncText.new()
doc.edit(at: 120, insert: "flap angle")   // offline OK
app.sync(doc, over: session)               // merges conflict-free on reconnect
```

### Zod/Pydantic/Ecto → `core.validate` — D-VALIDATE1 (core lib)

Decode gives type-shape errors one at a time; refinements prove static
bounds. What forms need: validate a whole value, accumulate EVERY field
error with paths, return them as data (to render beside form fields).
Transplant: one validation engine over the existing machinery — reuses
`DecodeError { path, reason }`, `@Pre` conditions, and refinement
bounds; never a second schema language (the struct IS the schema, I8):

```jet
signup :: req.decode<Signup>().validate() ?? (errs) => {
    return Response.unprocessable(errs)    // [{path: "email", reason: "…"}, …]
}
```

### Supabase RLS → row policies — D-DBPOLICY1 (core lib, safety)

Enterprises need "users see only their rows" enforced below app code.
Transplant: typed row policies on `core.db` tables, checked against the
live-query/mutation paths — a query that could violate a declared policy
is a compile-time effect error, and the runtime filter is generated:

```jet
db.policy<Ticket>((user, row) => row.team == user.team)
```

### direnv → env auto-activation — D-ENVHOOK1 (tooling)

`jet env` requires an explicit enter. direnv proved cd-activation is the
magic default devs keep. Transplant: `jet env hook fish|bash|zsh` prints
a shell hook; entering a dir with `env.jet` activates (with the same
trust prompt law as everything else — D-JPK-GRANTCMD1), leaving
deactivates. Opt-in install, one line.

### Erlang observer → live runtime inspector — D-OBSERVE-LIVE1 (tooling)

Erlang ships a live view of every process, mailbox, and memory cell.
Jet's scheduler/tasks/channels are opaque at runtime. Transplant:
`jet inspect live <pid|--attach>` — live task tree, channel depths,
deadlines, effect activity, GC/arena stats; the dev-server variant feeds
the same facts to Canvas's proof rail. Rides `.jettrace`/observability
rails (D-PERFSESSION1, D-OBS1) — a viewer, not a new fact producer.

## v2 rejections (recorded, with the reason)

- **Racket language-towers / reader macros** — D-EXT1 Tier 3/4 rejected
  grammar mutation, even for experts. Firm.
- **Smalltalk image persistence** — opaque state fights source-truth law
  (jetos/Studio never own state outside source; D-WD7). The live *feel*
  is delivered by dev/hot-swap/REPL/Canvas instead.
- **APL/BQN symbol density** — readability is priority 2; fan-out,
  adapters, and compute already deliver the semantics.
- **Kotlin extension functions on foreign types** — UFCS declined
  (D-UFCS1); orphan rule keeps method origin knowable.
- **Haskell pervasive laziness** — explicit lazy adapters only
  (D-ITERTOOLS1); pervasive laziness wrecks predictable performance
  (priority 3).
- **Clojure persistent-by-default collections** — Jet's ownership makes
  copies explicit (`copy`); persistent structures become a library when
  a real workload demands (no ballot without evidence —
  D-STDLIBLEDGER1 spirit).
- **Plan 9 everything-is-a-file** — jetos chose typed option trees +
  proof artifacts; strictly stronger for the same goal.
- **Julia broadcasting dot-ops** — fan-out `f.[…]` (S75) already
  canonical; a second axis was already declined (D-FANOUT2).

## v2 already-have-it additions (so nobody re-proposes)

- Go `context` → `#Context` deadlines/cancellation (ratified).
- Zig comptime → S26/S57 layered comptime (stricter: never types).
- OCaml functors → generic modules (D-GENMOD1/2).
- Ada ranged types → `distinct Int(0..10)` (D-RANGETYPE1).
- Cloudflare Durable Objects → D-SERVICE-STATE1 snapshot/event-log
  actors.
- Kafka event sourcing → `.EventLog` state adapter + durable delivery.
- Bazel remote cache/exec → D-BUILDCACHE1/D-BUILDREMOTE1.
- jj operation-log undo → Tower undo, codemod undo, jetos generations.
- mise/asdf toolchain pins → U30 `jet:` pin.
- Redis TTL values → `core.time.expiring` (D-TTLVAL1) + `Lru`.
- Excel reactive cells → signals + computed fields (D-FIELDPOL1).
- Wolfram/Jupyter → D-NOTEBOOK-* family.
- OpenTelemetry → D-LOGTRACE1 typed events/spans, OTLP export.

## Explicitly not transplanted

- Convex's "everything lives in our cloud" — Jet stays local-first;
  the live-query engine must run in `jet dev` and any self-hosted
  server. No SaaS dependency enters the language.
- Firebase-style schemaless sync — fights the typed-data floor (D-WD9)
  and decode migration law; nothing to salvage that `DataTree` +
  migrations don't already give.
