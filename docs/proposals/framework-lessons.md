# Framework lessons — transplants, not surveys (2026-07-11)

Owner ask: what made products like Convex feel like magic, and what of it
can Jet absorb? Each row below is a concrete transplant into ratified Jet
machinery with worked code, or an explicit "already have it". Big ideas
are ballots on the framework-lessons card; the rest is recorded so we
never re-derive it.

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

## Explicitly not transplanted

- Convex's "everything lives in our cloud" — Jet stays local-first;
  the live-query engine must run in `jet dev` and any self-hosted
  server. No SaaS dependency enters the language.
- Firebase-style schemaless sync — fights the typed-data floor (D-WD9)
  and decode migration law; nothing to salvage that `DataTree` +
  migrations don't already give.
