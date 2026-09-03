# Primitive probe: distributed execution and streams

## What I built

I built five runnable Jet programs: a 16-worker `para_map`, a bounded two-stage channel pipeline, a two-process TCP protocol, and a Stream-backed event-time window with in-memory checkpoint replay. The process program was built to native executables and run both through `jet run` and the generated parent binary. The final requested stream behavior is only partly buildable: finite in-memory windows work, but Stream has no keyed, watermark, or restartable-checkpoint contract.

Files under `pkg/`:
- `package.jet`
- `parallel_map.jet`
- `channel_pipeline.jet`
- `socket_worker.jet`, `socket_parent.jet`
- `window_checkpoint.jet`
- `window_api_attempt.jet`, `watermark_attempt.jet`, `checkpoint_attempt.jet`, `typed_process_attempt.jet`, `stream_runtime_attempt.jet` (negative API probes)
- `bench_parallel.py` (incumbent comparator attempt; not runnable on this host)

## What worked

- Local work-stealing map: `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-distributed/pkg/parallel_map.jet` -> `parallel_map workers=16 items=256 results=256`. The callback is pure, input order is preserved, and 16 workers execute successfully.
- Bounded backpressure pipeline: `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-distributed/pkg/channel_pipeline.jet` -> `bounded_pipeline capacity=2 input=8 output=8` and `[1, 4, 9, 16, 25, 36, 49, 64]`. Both channel edges have capacity 2; a 5ms worker delay makes the bounded path exercise blocking sends.
- Native process launch: `scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/prim-distributed/pkg/socket_worker.jet` -> `Built build/socket_worker`; `scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/prim-distributed/pkg/socket_parent.jet` -> `Built build/socket_parent`.
- Two local processes and typed application protocol: `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-distributed/pkg/socket_parent.jet` -> `socket_workers=2 typed=Int results=49,121` and `workers_exited=true`. Running `build/socket_parent` produced the same two lines. The worker parses an `Int`, squares it, writes the response, closes, and the parent waits for both process receipts.
- Local Stream and finite event-time state: `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-distributed/pkg/window_checkpoint.jet` -> six timestamped events with watermarks 7, 7, 7, 16, 16, 26; then `window_keys=5 recovery=in-memory replayed=3 watermark=26`. This proves a library can carry timestamps in an Event struct, compute keyed tumbling buckets, copy a map checkpoint, and replay a finite suffix.

Research cross-check: the distributed-systems and hpc-parallel reports describe Jet's current task/channel support as local and find no launcher, rank, or inter-process typed exchange. The stream-processing report/claims describe `Stream<T>` as a local pull/cancellation primitive with no timestamp, window, or persisted operator state. The messaging-eventing report says channels/mailboxes and receipts are process-local, not a durable broker stream.

## Gaps

### G1 — event-time keyed window surface

Tags: `boilerplate`. Primitive capability missing: a first-class `Stream<T>` operator contract for keyed windows, event-time timestamps, watermark advancement, and late-event disposition. Evidence: `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/prim-distributed/pkg/window_api_attempt.jet` reports `Error [E0102]: Stream has no method key_by`; `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/prim-distributed/pkg/watermark_attempt.jet` reports `Error [E0102]: Stream has no method watermark`. The successful workaround is 58 lines in `window_checkpoint.jet`, including a manually keyed `[String:Int]` map, watermark arithmetic, bucket naming, and a duplicated replay loop. Shared by `backend`, `ai-ml`, and `science` stream libraries.

### G2 — restartable durable stream checkpoint

Tags: `boilerplate`. Primitive capability missing: a restartable checkpoint handle that records source position and keyed operator state, persists it, and resumes with an explicit replay/commit contract. Evidence: `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/prim-distributed/pkg/checkpoint_attempt.jet` reports `Error [E0102]: Stream has no method checkpoint`. The only successful result is explicitly `recovery=in-memory replayed=3`; it does not survive process restart or define exactly-once behavior. A library can copy an in-memory map and hand-code suffix replay, but it cannot obtain a standard durable source/state checkpoint contract. Shared by `backend`, `ai-ml`, and `science`; this blocks the requested production recovery behavior.

### G3 — typed inter-process channel

Tags: `call-site`, `boilerplate`. Primitive capability missing: a schema-checked, framed inter-process message endpoint with typed send/receive, lifecycle, and failure receipts. Evidence: `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/prim-distributed/pkg/typed_process_attempt.jet` reports `Error [E0102]: ProcessChild has no method send`. The working two-process version requires parent socket setup and accept/close/wait code (`socket_parent.jet:7-24`) plus per-message `to_string`/`to_int` conversion and raw byte writes (`socket_worker.jet:8-12`). Every message schema needs its own framing and conversion code; every caller sees text/protocol choices. Shared by `backend`, `ai-ml`, and `science`; one local request/reply protocol is buildable, but a reusable typed process channel is not.

## Friction

- The event-time workaround repeats window-key calculation and map mutation in both live and replay loops: 2 copies of each, 58 Jet lines for six events. This is author boilerplate for a common stream operation, not a user-visible library API.
- The process workaround is 41 Jet lines for two workers and one `Int` request/reply schema. The parent manually creates a listener, spawns two children, accepts two sockets, sends raw text, reads raw text, closes both streams, and joins both children.
- `jet check` emits `Warning [L2510] (hidden_cost_in_loop)` for the arithmetic in `parallel_map.jet`, for channel worker arithmetic, and for each keyed map update. It warns about exact-Int spill or shared-map representation cost. I record this as friction, not a `slow` gap: Python and Rayon comparators were not runnable because this host has no `python`, `python3`, or `rayon` command, and cargo was prohibited.
- The startup-inclusive parallel run took `real 2m11.879s`, `user 11m9.497s`, `sys 2m15.228s` for 256 items × 12,000 loop iterations. No incumbent speed claim is made without the required same-input comparator.

## Defects

None observed. The negative API probes produced the expected missing-method diagnostics, and all five positive programs returned the expected results.

## Verdict

Buildable today: local `para_map`, bounded channels with backpressure, local Stream pull, and two local native processes.
Finite event-time keyed state and in-memory replay are possible as ordinary Jet library code.
The requested production restartable stream is not buildable with a standard timestamp/window/checkpoint/replay contract.
Typed process data works only through a hand-coded text protocol; it is not a typed inter-process channel.
The highest-value primitives are G1's event-time/window state, G2's durable checkpoint contract, and G3's typed process endpoint.
