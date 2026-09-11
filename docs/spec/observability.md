# Jet observation guide

Use one query when a program is slow:

```text
jet ? why is my program slow
```

This table is the single vocabulary for observation. Choose a surface by the
question it answers.

| Surface | Question answered | Start here | Flag or recorder | Artifact envelope and convergence |
| --- | --- | --- | --- | --- |
| Live scheduler | What runs, waits, or queues now? | `jet run app.jet --observe`, then `jet inspect live <pid>` | `--observe` publishes bounded `jet.devtools.v1` events. `--once` and `--json` project that stream. | `jet-observe-<pid>.json` is a bounded projection with freshness and process identity checks; it does not expose unpublished values. |
| GC promotions | Which allocations enter automatic memory management, and what ownership rewrite helps? | `jet run app.jet --gc-trace`, then `jet gc report` | `--gc-trace` records promotion evidence. | `.jet/gc/trace-v1.json` uses `jet.gc.trace` v1. `jet gc report` is a typed projection and rejects dropped rows. |
| Wall-clock session | Which symbols consume wall time, CPU, task, lock, or I/O time? | `jet perf run app.jet`, then `jet perf view <trace.jettrace>` | `jet perf run`, `jet perf test`, and `jet perf attach` record sessions. The ratified user-verb on-ramp is `--record=<name>`. | `.jettrace` remains the historical artifact; its trace and cost facts can be projected from `jet.devtools.v1`. |
| Browser rows | Which browser, WebAssembly, or DOM rows consume time? | `jet dev --target=web`, then `jet perf attach <pid>` | The dev server sends typed relay rows. `jet perf attach` requests collection. | The relay is a host transport; rows map into the canonical trace, frame, request, and custom-payload families without inventing a second observation protocol. |

`jet perf export <trace.jettrace> --chrome` writes Chrome Trace Event JSON. Open
the output in Perfetto UI or `chrome://tracing`. Captured wall/cpu, allocation,
browser, task-span, I/O, lock, and native rows become complete (`X`) events;
process, domain, and task lanes use metadata (`M`) events. Unavailable states,
async flows, counters, screenshots, and source-map events remain outside this
projection. `--pprof` and `--otel` remain Jet JSON projections, not wire-format
exports.

## Canonical devtools protocol

`jet.devtools.v1` is the single observation envelope for devtools hosts.  Every
tier publishes the same typed event stream; hosts project it instead of
reconstructing meaning from a transport payload.  The event families are
`build`, `route`, `query`, `mutation`, `form`, `table`, `store`, `trace`, `cost`,
`gate`, `structure`, `test`, `job`, `frame`, `request`, and explicitly typed
custom payloads.

Values are private unless a producer publishes them through the typed devtools
boundary.  Hosts must not infer locals, fields, or payload values from an event
that does not carry them.  Devtools transport is a development surface and is
served on loopback; production hosts do not expose the stream.

Development sessions retain a bounded event ring.  A time cursor selects the
newest retained state at or before that cursor; a cursor older than the ring
requests a reset and reports truncation rather than pretending history exists.


## Server-function observations

`action`, `form`, and `data` registrations publish a typed
`server_function` fact in the same `jet.devtools.v1` stream. The fact names the
stable endpoint, method, input/output/error wire types, CSRF policy, declared
effects, retry and idempotency policy, middleware order, call state, attempt
count, and dependent-data keys for revalidation.

The fact is metadata only. Request bodies, authentication credentials,
capability tokens, handler results, and handler errors are never devtools
payloads. A development App may render this fact projection for inspection;
release HTML does not include the panel or the server-function state.

## Query and mutation observations

The `query` and `mutation` event families carry identity and lifecycle facts,
not private values. A query fact includes its explicit key, dependency
footprint, generation, freshness age, observer count, invalidation cause, and
state. A mutation fact includes its key, lifecycle state, attempt count, queue
length, and invalidation targets. Payloads and handler results remain inside
the typed runtime signals.

Query keys are cache identities. Reusing a key with a different footprint is a
runtime contract error (`E2473`), rather than a second cache row. A mutation
without explicit targets revalidates the declaring query's footprint; explicit
`key:<key>` and footprint targets are retained in the event fact.

Offline-first mutations use a durable FIFO queue. The queue file is written
with an atomic replacement and is loaded before replay. Missing state
configuration, malformed rows, and I/O failures are reported as a durability
error; the runtime never reports a successful enqueue after losing the
payload. Development panels may show queue count and lifecycle metadata, but
never payload bytes or error text.

## Devtools hosts

`jet.devtools.v1` gives every development host one typed event stream. The
resident session owns the event cursor, panel catalog, selection, and time
cursor. Hosts request a typed projection with `devtools_events_since` and
`devtools_panels`; they do not decode event text to recover panel meaning.

The first-party catalog includes `Build`, `Routes`, `Queries`, `Mutations`,
`Forms`, `Table`, `Store`, `Traces`, `Cost`, `Gates`, `Structure`, `Tests`, and
`Jobs`. It also includes `UiTree`, `Database`, `Topology`, `Game`, `World`, and
`Profiler` when their checked capabilities are present. Missing capabilities
remain visible as an unavailable panel with a reason.

Five user-facing hosts use the same projection:

| Host | Entry point | Shared behavior |
| --- | --- | --- |
| Browser in-app | `/__jet_devtools` | Shows the status, selected fact, and available panels. |
| Browser workbench | `/__jet_devtools/workbench` | Shows the panel rail, event timeline, inspector, and time cursor. |
| Terminal | `jet dev` | Renders the same panel facts through `TerminalHost`; `NO_COLOR` uses plain text. |
| Editor | `jet/devtools/open` | Returns a bounded workbench projection for VS Code and Zed clients. |
| Native overlay | Foundation native-host callbacks | Receives the same typed events and bounded frame commands. |

The browser workbench uses three columns at wide widths. At narrow widths it
stacks the rail, facts, and inspector. A browser or terminal selection posts
`panel_id` and `item_key` to `/__jet_devtools/selection`. A time change posts
the event sequence to `/__jet_devtools/cursor`. The next projection carries
that state to every host.

The shared lifecycle states are `starting`, `building`, `ready`, `error`,
`unavailable`, and `stopped`. A host must show the state and its diagnostic
facts. It must not turn an unavailable panel or command into a silent no-op.

## Recorder boundary

D-RUN-RECORD1=A ratifies `--record=<name>` on `run`, `dev`, and `test` as the
user-verb recorder. It writes the `.jetproof-replay` artifact. This guide does
not add a second recording flag. The table keeps live snapshots, GC evidence,
historical sessions, and browser transport distinct until their contracts can
share one envelope without losing meaning.

## Recorded-run queries

`jet run <file> --record=<name>` can attach source-level act snapshots to the
same `.jetproof-replay` receipt. The receipt keeps the existing Time authority,
identity, frame hashes, and footer. It does not create a `.jettrace` file or a
second trace format.

`jet debug <file> --replay=<name>` reads the receipt without changing it. The
source-level prompt supports these queries:

- `why <place> == <value>` names the recorded acts that produced the value.
- `when <place>` lists recorded changes and names the last change.

The queries use recorded local snapshots. They do not reverse execution, move
the program through time, or keep a standing per-variable history buffer. A run

## Implementation homes

- Live scheduler: `crates/jet-devserver/src/LiveInspect.rs`
- GC promotions: `Source/CmdGc.rs`
- Wall-clock session: `Source/CmdPerf.rs` and `crates/jet-foundation/src/JetTrace.rs`
- Browser rows: `crates/jet-devserver/src/BrowserTrace.rs`, merged by `Source/CmdPerf.rs`

The `jet ? why is my program slow` route renders this file. The guide and the
CLI output therefore have one home.
