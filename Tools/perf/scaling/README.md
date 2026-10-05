# Compiler scaling gate (Tower #4319)

Dependency-free Node tools; GNU time measures the complete compiler process tree.
Default Jet: `~/.cache/jet-dev/scratch/jet-release-night12/jet`.
Default time: `/nix/store/n0wrh3vjfwcqfyswwai0zcxvkpibq34v-time-1.10/bin/time`.

```sh
node Tools/perf/scaling/generate.mjs functions 32 /tmp/new-functions
~/.cache/jet-dev/laneS.sh node --test Tools/perf/scaling/scaling.test.mjs
~/.cache/jet-dev/laneB.sh 8 node Tools/perf/scaling/harness.mjs --repeats 3 --work-dir ~/.cache/jet-dev/scratch/sol/SolScaleGate --receipt /tmp/scaling.json
```

Axes: `functions` (N helpers), `declarations` (N structs/enums/generic aliases),
`depth` (N nested additions), `modules` (N imported files), `match-arms` (N value
arms plus a fixed fallback), `string-literals` (N bytes), `interpolation` (N
holes), `fan-in` (N call sites into one shared helper, two fixed definitions),
and `generics` (N distinct fixed-array type instantiations, fixed generic body).
Only the selected dimension scales; necessary consuming statements also scale.
Modules contain only a constant, not additional function/type definitions.
Generated programs have a runnable entry; no runtime timing is included.
The generator refuses to overwrite source files; use a new output directory.
`mixed` (`mixed.mjs`) is the compile-throughput cell's whole-program profile:
`node Tools/perf/scaling/generate.mjs mixed --code-lines N --seed S DIR` writes
one package of at least N code lines and prints its digest, golden stdout and
shape as JSON (Tools/perf/throughput measures it).
`trait-facts-fix.json` records the module allocation evidence and dual-compiler fix spec.
The Rust ledger indexes declarations, aliases and checked reference sites by
source module; package sibling iteration borrows the loader's sorted membership
list. Structural auto-derive publication retains one shared canonical table of
actual nominal declarations, while derive/codegen selections remain module-local.
Empty marker-rule and non-App entry paths do not construct unused bundle contexts.
Declaration/alias lookup uses borrowed keys in module-owned maps, and canonical
module/declaration identities are cached. Structure observations retain source
order with collision-checked hash buckets; import-edge presence uses a source/span
index. Body snapshots borrow loader alias roots rather than cloning that set.
Default ladders: 256/512/1024; modules 64/128/256 (8 GiB memory cap), depth
16/32/64, match arms 48/96/192 (nesting limits), literal bytes 1/2/4 MiB.
Check and build each measure N=1 plus their ladder, three fresh runs per point.
Ratios are marginal: `(metric(upper)-metric(1))/(metric(lower)-metric(1))`.
Receipts keep commands, hashes, samples/logs, raw medians and marginal ratios.
Exit 1: ratio >2.2, compile/counter failure, or an unmeasurable non-positive
marginal baseline (noise); exit 2: invalid invocation. No warm build cache reuse.

`--n BASE` replaces every selected ladder; repeat `--axis NAME` to select axes.
Templates run in a shell, with shell-quoted `{jet}`, `{source}`, `{dir}`, `{out}`,
`{axis}`, `{n}` placeholders; e.g. `--command 'jetc0=/path/jetc0 {source}'`.
Repeat `--command`; `--jet`, `--time`, `--threshold`, `--receipt` replace defaults.
`--overrides FILE` accepts an axis table, e.g.
`{"depth":{"bound":3,"reason":"Documented algorithm and issue reference"}}`.
There are no built-in exemptions; every override requires a reason and bound.
`JET_PHASE_COUNTERS {"phase":"parse","visits":42,"bytes_copied":64}` lines on
stdout/stderr are summed by phase/key; nested numeric objects also work.
Counter ratios gate independently; no counter lines means timing/RSS-only data.
The self-hosted compiler (a jetc runner, via `--command`) writes them when run
with both `JET_TRACE_FILE` and `JET_PHASE_COUNTERS=1`: per phase (`register`,
`sema.check`), the registration graph's `module_lookups` and `name_lookups`.
