# Jet observation guide

Use one query when a program is slow:

```text
jet ? why is my program slow
```

This page is the terminal guide for package authors and toolchain maintainers.
It covers live runtime facts, GC evidence, wall-clock traces, browser rows, and
recorded runs. Executable truth is in `crates/jet-foundation/src/Devtools.rs`,
`crates/jet-devserver/src/LiveInspect.rs`, `Source/CmdGc.rs`, and
`Source/CmdPerf.rs`. The observation terms follow `Docs/spec/vocabulary.md`;
an observation event is not a private payload channel.

The table is the single vocabulary for observation. Choose a surface by the
question it answers.

| Surface | Question answered | Start here | Flag or recorder | Artifact envelope and convergence |
|---|---|---|---|---|
| Live scheduler | What runs, waits, or queues now? | `jet run app.jet --observe`, then `jet inspect structure --live <pid>` or another named inspect plane | `--observe` publishes bounded `jet.devtools.v1` facts. Use `--json` for a machine projection. | The live snapshot is `jet-observe-<pid>.json` in the host temporary directory; it carries freshness and process identity and exposes no unpublished values. |
| GC promotions | Which allocations enter automatic memory management, and what ownership rewrite helps? | `jet run app.jet --gc-trace`, then `jet gc report` | `--gc-trace` records promotion evidence. | `.jet/gc/trace-v1.json` uses `jet.gc.trace` v1. The separate `jet.gc.report` v1 projection requires complete promotion and identity evidence and rejects dropped rows. |
| Wall-clock session | Which symbols consume wall time, CPU, task, lock, or I/O time? | `jet perf run app.jet`, then `jet perf view <trace.jettrace>` | `jet perf run`, `jet perf test`, and `jet perf attach <pid> --source app.jet` record sessions. The ratified user-verb recorder is `--record=<name>` on `run`, `dev`, and `test`. | The default artifact is `.jet/perf/<stamp>-<short-id>.jettrace`, with schema `jet.trace` v1. |
| Browser rows | Which browser, WebAssembly, or DOM rows consume time? | `jet dev --target=web`, then `jet perf attach <pid>` | The dev server sends bounded, payload-free relay rows; `jet perf attach` requests collection. | The request transport uses `jet.browser.request.v1`; `jet perf attach` merges rows into the canonical `.jettrace` rather than creating a second trace format. |

## Compiler pass-boundary journal

The compiler proof observer uses the opt-in `jet.canonical-pass.v1` journal,
not the runtime trace or the devtools event ring. Its source owner is
`Compiler/JetFoundation/Source/CanonicalPass/`. Activation requires nonempty
native environment values for both `JET_ADAPTER_MODE` and
`JET_ADAPTER_OPERATION_IDS`. Recording additionally requires a strict-Unicode,
exact comma-separated operation match; whitespace is not trimmed, and invalid
Unicode cannot select an operation even when its encoded value enables capture.

Records retain actual before/after AST or MIR snapshots, identities, source,
premises, disposition, and order. AST snapshots use authored item order and
UTF-8 source-byte lengths, not reconstructed registration facts. Occurrence
counts span stages within the same thread. A private occurrence index replaces
scanning all earlier records on each append; neither that index nor journal
storage is part of the public record. Clear and move-take reset both.

Process persistence selects `JET_ADAPTER_CANONICAL_PASS_PROCESS` by exact
Unicode equality and keeps `JET_ADAPTER_CANONICAL_PASS_PATH` as a native path.
It drains before opening with create-and-append semantics, writes one
`jet.canonical-pass-journal.v1` JSON line, and ignores only the original
open/write errors. Snapshot JSON is inserted as raw JSON, not encoded as a
digest or quoted text. The inspect projection adds the process label and uses
the canonical `StatusValue` parser, retaining the original literal-string
fallback when snapshot parsing fails.

Native TLS, environment activation, native-width overflow, and retained append
owner bodies require the real Core providers under #4494. The inward AST
bundle/constructor cutover and compound original Debug callees are separate
source prerequisites. Source delivery does not prove those boundaries or the
composed compiler/CLI consumer.

## Exporting a wall-clock trace

`jet perf export <trace.jettrace> --chrome` writes Chrome Trace Event JSON.
Open it in Perfetto UI or `chrome://tracing`. Captured wall/CPU, allocation,
browser, task-span, I/O, lock, and native rows become complete (`X`) events;
process, domain, and task lanes use metadata (`M`) events. Unavailable states,
async flows, counters, screenshots, and source-map events remain outside this
projection. `--pprof` and `--otel` remain Jet JSON projections, not wire-format
exports.

The canonical trace wrapper is `jet.trace` version 1. The Jet wire owner is
`Compiler/JetFoundation/Source/Trace/JetTrace.jet`; its typed `TRACE_VERSION`
and `TRACE_SCHEMA` are shared by consumers. The streaming compiler trace sink
is a different contract. `jet.gc.trace` and `jet.gc.report` also retain their
own schemas; sharing a version fact does not make GC evidence a `jet.trace`.

Trace's borrowed hash-input and validation windows use bare range places,
`value[start..<end]`, not the retired `.view(range)` spelling. Sort comparators
compare `<=>` results with `Ordering.Equal`, not an integer sentinel.

Artifact bytes use Foundation's A-canonical JSON encoding with exactly one
terminal LF. `trace_id` is lowercase Hex64 SHA-256 of the canonical **content**
bytes, including that LF, not the outer wrapper. Verification checks closed
content keys, source attribution, causal task references, capture limits,
privacy exclusions, receipt/source-map digests, and measurement states before
checking the claimed identity and its hash. Capture policy has one current
reader/writer, schema 5; retired policy schemas are refused rather than
normalized into invented limits or truncation facts. Hardware facts must come
from Core sys's selected-build/available-parallelism provider, not trace-local
architecture or processor-count guesses. Source delivery alone does not prove
that native hardware provider or the CLI/package registration: those composed
boundaries require their own witnesses.

A session's `profile` record carries the sampling rows and the run window. The
run window is the runtime's own account of the run: the wall time and process
CPU since the Jet runtime started, which leaves out any in-process compile
that the session `wall`/`cpu` samples include. `window_status` is `exit` when
the runtime published the window at its shared exit seam, `live` for the last
periodic publication of a run that had not exited, and `unavailable` when no
window was published. On Linux, `jet perf run` reads the exit publication
before it reaps the child, so a completed short run records the same profile
rows, in the same order and with the same source ranges, on every capture.
Rows attribute to the parsed `fn run` declaration (`fn` through its closing
brace). The observe snapshot has no current-function or stack sample, so rows
do not yet separate the functions that `run` calls.

## Canonical devtools protocol

`jet.devtools.v1` is the single observation envelope for development hosts.
Every execution tier publishes the same typed event record; a host projects it
instead of reconstructing meaning from transport text. The first-party event
families are `build`, `route`, `query`, `mutation`, `form`, `table`, `store`,
`trace`, `cost`, `gate`, `structure`, `test`, `job`, `frame`,
`game-frame-sample`, `game-draw-event`, `game-launch-profile`, `game-swap`,
`request`, and `response`. Package-specific data may use the typed payload seam,
but a host must not invent a family or reinterpret a payload.

Foundation's Jet typed body and function/source-span carriers have their sole
home in `Compiler/JetFoundation/Source/Devtools/EventBody.jet`. Trace projects
the typed `GameFrameSample` and `GameDrawEvent` variants directly and preserves
their identities and source coordinates; it does not reparse transport tags.

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

The Jet observation carriers, bounded event/state rings, and wire rendering
live beside `EventBody.jet` in `Devtools/Observation.jet` and `Devtools/Wire.jet`.
Ring insertion replaces one retained slot; cursors and event iterators borrow
retained records rather than copying snapshots. State lookup selects the newest
snapshot at or before the cursor, including equal cursors.

The flat event boundary checks object framing, byte limits, balanced braces,
string escape state, and Unicode control characters; it is deliberately not a
second JSON parser. `from_wire` retains the supplied framed fields alongside the
typed body, following the original Foundation operation order. Observation
quotation uses `\\u00xx` for control characters other than newline, carriage
return, and tab, including DEL and C1. Host command/relay quotation instead uses
`\\b` and `\\f` for backspace and form feed.

`Devtools/Control.jet` owns the host-only checked database/game/rebuild command
DTOs; these are not generated-runtime observation records. Native callback
interfaces and relay validation belong to `Devtools/Native.jet`, not a second
host-local event vocabulary. The original native active-session binding is
process-global, not thread-local. Physical callback retention, automatic
registration teardown, poisoned shared registries, and native relay filesystem
operations need the canonical Core resource providers. Source declarations and
unexercised tests do not establish those native or generated-producer boundaries.

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
