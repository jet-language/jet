# Jet observation guide

Use one query when a program is slow:

```text
jet ? why is my program slow
```

This page is the terminal guide for package authors and toolchain maintainers.
It covers live runtime facts, GC evidence, wall-clock traces, browser rows, and
recorded runs. Executable truth is in `crates/jet-foundation/src/Devtools.rs`,
`crates/jet-devserver/src/LiveInspect.rs`, `Source/CmdGc.rs`, and
`Source/CmdPerf.rs`. The observation terms follow `docs/spec/vocabulary.md`;
an observation event is not a private payload channel.

The table is the single vocabulary for observation. Choose a surface by the
question it answers.

| Surface | Question answered | Start here | Flag or recorder | Artifact envelope and convergence |
|---|---|---|---|---|
| Live scheduler | What runs, waits, or queues now? | `jet run app.jet --observe`, then `jet inspect structure --live <pid>` or another named inspect plane | `--observe` publishes bounded `jet.devtools.v1` facts. Use `--json` for a machine projection. | The live snapshot is `jet-observe-<pid>.json` in the host temporary directory; it carries freshness and process identity and exposes no unpublished values. |
| GC promotions | Which allocations enter automatic memory management, and what ownership rewrite helps? | `jet run app.jet --gc-trace`, then `jet gc report` | `--gc-trace` records promotion evidence. | `.jet/gc/trace-v1.json` uses `jet.gc.trace` v1. The separate `jet.gc.report` v1 projection requires complete promotion and identity evidence and rejects dropped rows. |
| Wall-clock session | Which symbols consume wall time, CPU, task, lock, or I/O time? | `jet perf run app.jet`, then `jet perf view <trace.jettrace>` | `jet perf run`, `jet perf test`, and `jet perf attach <pid> --source app.jet` record sessions. The ratified user-verb recorder is `--record=<name>` on `run`, `dev`, and `test`. | The default artifact is `.jet/perf/<stamp>-<short-id>.jettrace`, with schema `jet.trace` v1. |
| Browser rows | Which browser, WebAssembly, or DOM rows consume time? | `jet dev --target=web`, then `jet perf attach <pid>` | The dev server sends bounded, payload-free relay rows; `jet perf attach` requests collection. | The request transport uses `jet.browser.request.v1`; `jet perf attach` merges rows into the canonical `.jettrace` rather than creating a second trace format. |

## Exporting a wall-clock trace

`jet perf export <trace.jettrace> --chrome` writes Chrome Trace Event JSON.
Open it in Perfetto UI or `chrome://tracing`. Captured wall/CPU, allocation,
browser, task-span, I/O, lock, and native rows become complete (`X`) events;
process, domain, and task lanes use metadata (`M`) events. Unavailable states,
async flows, counters, screenshots, and source-map events remain outside this
projection. `--pprof` and `--otel` remain Jet JSON projections, not wire-format
exports.

The canonical trace wrapper is `jet.trace` version 1, verified by
`crates/jet-foundation/src/JetTrace.rs`. A trace's identity, toolchain, and
hardware facts stay bound to that artifact; `jet perf view`, `compare`,
`attach`, and `export` use the same verification seam.

## Canonical devtools protocol

`jet.devtools.v1` is the single observation envelope for development hosts.
Every execution tier publishes the same typed event record; a host projects it
instead of reconstructing meaning from transport text. The first-party event
families are `build`, `route`, `query`, `mutation`, `form`, `table`, `store`,
`trace`, `cost`, `gate`, `structure`, `test`, `job`, `frame`,
`game-frame-sample`, `game-draw-event`, `game-launch-profile`, `game-swap`,
`request`, and `response`. Package-specific data may use the typed payload seam,
but a host must not invent a family or reinterpret a payload.

The protocol version is 1. Its development policy is loopback-only and
payload-free by default, with a maximum of 256 retained events and 256 history
entries, 16 KiB text fields, and 1 MiB envelopes. Values are private unless a
producer publishes them through the typed boundary. Live-value decisions carry
a slot identity, type identity, disposition, reason, and optional rendered
text; sensitive or invalid text is rejected or redacted by Foundation policy.
Production hosts do not expose the development stream.

A development session owns a bounded event ring. `devtools_events_since` and
`devtools_panels` return typed projections; hosts do not decode event text to
recover panel meaning. A cursor older than retained history requests a reset
and marks truncation, rather than pretending that missing history exists.

## Server-function observations

`action`, `form`, and `data` registrations publish typed server-function facts
in the same `jet.devtools.v1` stream. A fact can identify the endpoint and
method, input/output/error wire types, CSRF policy, capability and declared
effects, retry and idempotency policy, middleware, call state, attempt count,
and dependent-data keys for revalidation.

The fact is metadata only. Request bodies, authentication credentials,
capability tokens, handler results, and handler errors are never devtools
payloads. A development App may render the projection; release HTML omits the
panel and server-function state.

## Query and mutation observations

`query` and `mutation` events carry identity and lifecycle facts, not private
values. A query fact includes its explicit key, dependency footprint,
generation, freshness age, observer count, invalidation cause, and state. A
mutation fact includes its key, lifecycle, attempt count, queue length, and
invalidation targets. Query and mutation payloads and handler results remain in
typed runtime state.

Query keys are cache identities. Reusing a key with a different footprint is
E2473, a runtime contract error, not a second cache row. A mutation without
explicit targets revalidates the declaring query's footprint; explicit
`key:<key>` and footprint targets remain in the fact.

Offline-first mutations use the durable FIFO queue identified by
`jet-web-query-queue-v1`. The queue is loaded before replay and persisted by an
atomic replacement. Missing state configuration, malformed rows, and I/O
failures are durability errors; the runtime never reports a successful enqueue
after losing the payload. Development panels may show queue count and lifecycle
metadata, but never payload bytes or error text.

## Devtools hosts

The canonical panel catalog has these IDs: `Build`, `Routes`, `Queries`,
`Mutations`, `Forms`, `Table`, `Store`, `Traces`, `Cost`, `Gates`, `Structure`,
`Tests`, `Jobs`, `UiTree`, `Database`, `Topology`, `Game`, `World`, and
`Profiler`. A checked capability can make a panel available, unavailable with
a reason, or omitted from a host projection; unavailable is never a silent
no-op.

| Host | Entry point | Shared behavior |
|---|---|---|
| Browser in-app | `/__jet_devtools` | Shows status, selected fact, and available panels. |
| Browser workbench | `/__jet_devtools/workbench` | Shows the panel rail, event timeline, inspector, and cursor. |
| Terminal | `jet dev` | Renders the same panel facts through the terminal host; `NO_COLOR` uses plain text. |
| Editor | `jet/devtools/open` | Returns a bounded workbench projection for editor clients. |
| Native overlay | Foundation native-host callbacks | Receives the same typed events and bounded frame commands. |

The browser workbench uses three columns at wide widths and stacks the rail,
facts, and inspector at narrow widths. Browser selection posts `panel_id` and
`item_key` to `/__jet_devtools/selection`. Reconnection requests use the
bounded cursor at `/__jet_devtools/reconnect?cursor=<sequence>`; a stale cursor
returns an explicit reset/truncation projection. The next projection carries
the shared selection and cursor to every host.

The shared lifecycle states are `starting`, `building`, `ready`, `error`,
`unavailable`, and `stopped`. A host shows the state and its diagnostic facts;
it does not turn an unavailable panel or command into a silent no-op.

## Recorder boundary

D-RUN-RECORD1=A ratifies `--record=<name>` on `run`, `dev`, and `test` as the
user-verb recorder. The name is not a path, and the artifact is written under
`.jet/replays/<name>.jetproof-replay`. This guide does not add a second
recording flag; live snapshots, GC evidence, wall-clock sessions, and browser
transport remain distinct until their contracts can share an envelope without
losing meaning.

## Recorded-run queries

`jet run <file> --record=<name>` attaches source-level act snapshots to the
same `.jetproof-replay` receipt. The receipt keeps its Time authority, identity,
frame hashes, and footer; it does not create a `.jettrace` file or a second
trace format.

`jet debug <file> --replay=<name>` reads the receipt without changing it. The
source-level prompt supports:

- `why <place> == <value>` — names the recorded acts that produced the value;
- `when <place>` — lists recorded changes and names the last change.

These queries use recorded local snapshots. They do not reverse execution, move
the program through time, or keep a standing per-variable history buffer.

## Implementation homes

- Live scheduler: `crates/jet-devserver/src/LiveInspect.rs`
- GC promotions: `Source/CmdGc.rs`
- Wall-clock session: `Source/CmdPerf.rs` and `crates/jet-foundation/src/JetTrace.rs`
- Browser rows: `crates/jet-devserver/src/BrowserTrace.rs`, merged by `Source/CmdPerf.rs`
- Devtools protocol and panel policy: `crates/jet-foundation/src/Devtools.rs` and `crates/jet-codegen/src/Prelude/Core/DevtoolsPanelCatalog.rs`

The `jet ? why is my program slow` route prints this file verbatim. The guide
and the CLI output therefore have one home.
