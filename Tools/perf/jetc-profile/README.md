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

## Emit-text implementation pair

`Tools/gauntlet/measurement-manifest.json` records the `emit-text-passes`
implementation cell under `axes.compile_throughput.implementation_pairs`.
This changes existing compiler internals, not a user-facing performance spelling.
Prepare the two real programs from the immutable plain commit and this checkout:

```sh
node Tools/perf/jetc-profile/emit-text.mjs prepare 747eef360 /path/to/new/pair
```

Both programs use `emit-text.jet`: identifier-boundary, quoted/raw-string,
comment, multiline-string, leading-zero ID, and full-width MIR ID assertions,
then the same fold/unwrap/compact workload. The plain helper's byte argument
is adapted at its call site; neither arm's input, output, or measured pass
sequence changes. Build both with the same compiler and optimization settings.
After one warmup per arm, alternate at least three matched executions and keep
stdout as `plain/trial-001.out`, `optimized/trial-001.out`, and so on.
Then publish a new receipt:

```sh
node Tools/perf/jetc-profile/emit-text.mjs compare /path/to/pair /path/to/new/receipt.json
```

Comparison checks the prepared source hashes and exact non-timing stdout.
Every pass median must strictly beat its plain counterpart: equality, missing
trials, assertion failure, output drift, or any losing pass keeps the cell open.
The paired cell supplements, never replaces, end-to-end compile-throughput
and determinism gates. Comparator tests: `node --test
Tools/perf/jetc-profile/emit-text.test.mjs`.

## Wall time is not an off-CPU trace

A cycle-sampled busiest-thread estimate subtracted from wall time does not
identify waits. Record scheduler events or timestamped syscall durations for
that question, alongside uninstrumented wall/user/system time and major faults.
Thread syscall durations overlap: a launch thread's `futex` joining the
compiler worker is not contention in that worker. `strace` adds substantial
overhead to syscall-heavy workloads; never use its wall time as the speed cell.
Cold executable/image page-in and shared-machine scheduling must stay distinct
from compiler locks, sleeps, subprocesses, and store synchronization.
