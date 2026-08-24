# Jet observation guide

Use one query when a program is slow:

```text
jet ? why is my program slow
```

This table is the single vocabulary for observation. Choose a surface by the
question it answers.

| Surface | Question answered | Start here | Flag or recorder | Artifact envelope and convergence |
| --- | --- | --- | --- | --- |
| Live scheduler | What runs, waits, or queues now? | `jet run app.jet --observe`, then `jet inspect live <pid>` | `--observe` publishes a bounded snapshot. `--once` and `--json` project that snapshot. | `jet-observe-<pid>.json`, `schema_version: 1`. No shared recorder or envelope: live state has a freshness bound, process identity check, and no history. |
| GC promotions | Which allocations enter automatic memory management, and what ownership rewrite helps? | `jet run app.jet --gc-trace`, then `jet gc report` | `--gc-trace` records promotion evidence. | `.jet/gc/trace-v1.json` uses `jet.gc.trace` v1. `jet gc report` emits `jet.gc.report` v1. No shared envelope: the report requires complete promotion and identity evidence and rejects dropped rows. |
| Wall-clock session | Which symbols consume wall time, CPU, task, lock, or I/O time? | `jet perf run app.jet`, then `jet perf view <trace.jettrace>` | `jet perf run`, `jet perf test`, and `jet perf attach` record sessions. The ratified user-verb on-ramp is `--record=<name>`. | `.jettrace` uses the shared `jet.trace` v1 envelope. This is the historical recorder home. |
| Browser rows | Which browser, WebAssembly, or DOM rows consume time? | `jet dev --target=web`, then `jet perf attach <pid>` | The dev server sends payload-free relay rows. `jet perf attach` requests collection. | The relay uses `jet.browser.relay.v1` as transport. `jet perf attach` maps rows into `.jettrace` and `jet.trace` v1. The transport stays separate because it is not a user report or second artifact. |

`jet perf export <trace.jettrace> --chrome` writes Chrome Trace Event JSON. Open
the output in Perfetto UI or `chrome://tracing`. Captured wall/cpu, allocation,
browser, task-span, I/O, lock, and native rows become complete (`X`) events;
process, domain, and task lanes use metadata (`M`) events. Unavailable states,
async flows, counters, screenshots, and source-map events remain outside this
projection. `--pprof` and `--otel` remain Jet JSON projections, not wire-format
exports.

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
without a receipt has no backward query result. Those deferred features remain
under D-TIMETRAVEL1=C and D-RUN-RECORD1's deferral.

## Implementation homes

- Live scheduler: `crates/jet-devserver/src/LiveInspect.rs`
- GC promotions: `Source/CmdGc.rs`
- Wall-clock session: `Source/CmdPerf.rs` and `crates/jet-foundation/src/JetTrace.rs`
- Browser rows: `crates/jet-devserver/src/BrowserTrace.rs`, merged by `Source/CmdPerf.rs`

The `jet ? why is my program slow` route renders this file. The guide and the
CLI output therefore have one home.
