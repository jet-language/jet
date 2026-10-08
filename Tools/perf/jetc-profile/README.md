# Live Jet compiler CPU profile

Attach to a running Jet-written compiler; it is never started or restarted:
```sh
~/.cache/jet-dev/laneS.sh Tools/perf/jetc-profile/live.sh <pid> 20
```
Requires Linux perf permissions, `perf`, Node.js 20+, and an unstripped compiler.
Frame pointers are preferred (`perf record -g --call-graph=fp`, 99 Hz user cycles).
If most stacks are <=2 frames or have unresolved roots, auto mode captures a
fresh N-second DWARF window (16 KiB stacks); total capture may then be 2N seconds.
`JET_CALL_GRAPH=fp|dwarf|auto` overrides that choice (default `auto`).
`PERF=/path/to/perf` overrides PATH/Nix-store discovery.
`JET_PROFILE_OUT=/path` overrides the timestamped scratch output directory.
`JET_SOURCE_MAP=/path/compiler.map.json` overrides the loop6g bootstrap v2 map.
`JET_REPO=/path/to/jet` overrides the source root used for declaration locations.

Outputs: `top.txt` (top 30 inclusive/self counts and percentages), `phases.txt`,
`stacks.folded`, `flamegraph.svg`, `summary.json`, raw `perf.data`/`perf.script`.
Open the SVG in a browser; hover any frame for its full name, location and count.
Perf demangles Rust first; the analyzer decodes Jet `_u/_d/_c/_s/_b/_h/_xHH`
UTF-8 escapes and replaces compiler crate names with Jet module names.
The map inventories original Jet files, not generated Rust DWARF lines:
`[decl file:line]` means a uniquely matched declaration, not the sampled line.
SHA-256 mismatches are labeled `(current source)`; ambiguous names stay unlocated.
Inclusive counts deduplicate recursion; self counts use the leaf; all are samples.
Phase attribution uses the nearest recognizable leaf-side Jet function prefix,
then module fallback; generic type arguments alone do not establish a phase.
Unclassified samples and stack quality are reported, not silently redistributed.
Physical callchains omit inline expansion to keep symbolization inside laneS.
Re-render: `node Tools/perf/jetc-profile/profile.mjs render stacks.folded flamegraph.svg`.

## Wall time is not an off-CPU trace

A cycle-sampled busiest-thread estimate subtracted from wall time does not
identify waits. Record scheduler events or timestamped syscall durations for
that question, alongside uninstrumented wall/user/system time and major faults.
Thread syscall durations overlap: a launch thread's `futex` joining the
compiler worker is not contention in that worker. `strace` adds substantial
overhead to syscall-heavy workloads; never use its wall time as the speed cell.
Cold executable/image page-in and shared-machine scheduling must stay distinct
from compiler locks, sleeps, subprocesses, and store synchronization.

## Native startup implementation pair

The `native-host-shape-view` cell compares the full-program Source projection
with the shape-only projection and cached index. It changes marshalling, not
host-shape policy: the same Source checker still validates nominal definitions,
Core owner identities, source-module membership and owned handle lifecycles.
The scoped program view retains the canonical header and only four collection
tables: `types`, `core_owners`, `modules`, and `handles`. Its index is built once
for every ABI query in a native compile, then released with the view. The full
native compiler image and its identity/root checks are unchanged.

The immutable compiler-image owner also caches its canonical execution identity.
Restore computes and checks that identity against the archived identity once;
adapter descriptors, callable templates and callback activation reuse that same
artifact-bound identity instead of rehashing source maps. The bootstrap harness
mode `image-identity-check` checks cache ownership and compares the cached bytes
with a fresh canonical computation for the same image. Run that check separately
from the timed hello processes: its intentional recomputation is not startup
work. Source edits may change a candidate's compiler identity; reuse must not
change the identity of the same artifact, or the emitted hello output.
Bootstrap backend builds honor `CARGO_TARGET_DIR` for the compiler and fixture
artifacts; when it is unset they use the checkout's `target` directory. Pass the
same explicit target into the detached build environment, not merely the shell
that launches it.

Before measuring, the main agent must admit a written pre-flight. Use the same
trusted hello project, build profile, environment and machine for both compiler
artifacts. Persist every process's stdout/stderr and exit status through the
proof lane; retain verbose output from the pinned `GNU_TIME` exported by
`Tools/perf/throughput/measure.mjs`, the canonical compile receipt,
emitted `out.rs` and `JET_TRACE_FILE` trace. After one warmup per arm, interleave
at least three matched trials under `plain/trial-001/` and
`optimized/trial-001/`, and so on. Each trial has `rc`, the canonical
line-oriented `receipt` (factory tier `Aot`), `out.rs`, `trace.json`,
`time.txt` and `invocation.json`.

Keep the actual immutable project snapshot in the pair's `inputs/` directory
and use that same absolute `JET_BOOTSTRAP_SOURCE_ROOT` for every process.
Calculate its hash with the exported `startupInputHash` function, which hashes
sorted relative paths and file-byte hashes. Pin the selected relative source
entry in `pair.json` as `entry`; each actual launch records that same `entry`,
`source_root` and freshly calculated `source_sha256` in `invocation.json`.
The launcher also records the actual compiler `process_pid`, independently
captured `process_started_unix_ms` and `process_finished_unix_ms`, and the
SHA-256 of the final trace bytes as `trace_sha256`. Do not synthesize these
process facts from the trace. The canonical reader binds `trace.start`, every
event's process ID and every timed event to the successful recorded process
window. Unix anchors have millisecond precision; relative timestamps are
microseconds. Producer metadata without timestamps remains valid.
The comparator checks the source facts against the snapshot bytes, not a
free-standing manifest hash. Do not include stores or outputs in the snapshot.
The shared `pair.json` records schema `jet.native-host-shape-pair.v1`,
`source_sha256`, `entry`, `machine` and `build_profile`; retain exact immutable
compiler paths and build receipts with that evidence. Publish the comparison:

```sh
node Tools/perf/jetc-profile/startup.mjs /path/to/pair /path/to/new/receipt.json
```

The comparator requires complete AOT receipts with no reports, identical actual
project inputs and byte-identical emitted artifacts. Receipt `source_bytes`
counts emitted Rust bytes, not the input closure. Every median must strictly
win: process wall time, compile trace duration and the interval from compile
start to the selected source entry's parse span. Trace labels use the canonical
loader's relative source path, not a literal `main` or `main.jet`. The canonical
trace reader accepts the producer's open-array, object/comma-per-line format;
completed evidence rejects malformed/truncated events, invalid schemas,
out-of-window times, unbalanced scopes and mismatched threads. The startup
comparator selects the pinned entry and rejects missing or ambiguous spans.
The pre-parse interval
includes startup/load work, not an off-CPU or lock-contention measurement.
Losses, missing evidence or identity/output drift keep the cell open.
This startup cell does not replace cold frontend acceptance on the same pinned
30k fixture and real compiler sources; neither warm-cache nor removed emission
work earns frontend speed credit. Unit tests are in
`Tools/perf/jetc-profile/startup.test.mjs`; the actual Source `TaskRoots.jet`
fixture separately receives the canonical codec's actual projected program,
compares all registered handle shapes with the evaluator's full program/index,
and retains negative module/duplicate-ID checks.
