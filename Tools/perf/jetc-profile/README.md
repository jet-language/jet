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
