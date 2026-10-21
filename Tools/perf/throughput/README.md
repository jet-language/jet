# Compile-throughput cell (Tower #4529)

`harness.mjs` measures `axes.compile_throughput` of
`Tools/gauntlet/measurement-manifest.json`: clean builds of the cell's
programs by one compiler binary, in a default-jobs arm and a single-job arm.
Dependency-free Node; Linux `/proc`; GNU time for exact tree rusage.

```sh
# quick lane: bench30k frontend, 3 samples, against quick-baseline.json
node Tools/perf/throughput/harness.mjs --compiler jet=PATH --compiler-profile TEXT
# full lane: bench300k + selfhost, 5 samples, golden run
Tools/agent/jet-env node Tools/perf/throughput/harness.mjs --lane full --compiler jet=PATH --compiler-profile TEXT
node --test Tools/perf/throughput/throughput.test.mjs
```

Compilers: `jet=PATH` is a `jet` binary; every sample is one `jet build`, which
builds the artifact inside the measured process. Every measured command runs in
its own `systemd-run` scope (`--memory-max`, no swap).

Programs: `bench300k` and `bench30k` come from
`node Tools/perf/scaling/generate.mjs mixed --code-lines N --seed 1 DIR`
(`Tools/perf/scaling/mixed.mjs`); `bench.json` pins their package digest,
golden stdout and shape, and the harness fails closed when a regenerated
program differs. `selfhost` is `jet build Compiler` over a copy of the
compiler's packages of `--source-tree` (`Tools/agent/compiler-packages.mjs`),
with that tree's Core.

Each sample: a fresh copy of the program at one fixed path, an empty
`JET_STORE_DIR`, no `.jet` state, one unmeasured warmup per arm. Recorded
per sample: wall, user and system CPU ns (GNU time rusage), peak RSS (the
larger of the sampled process-tree sum and the largest process high-water
mark), threads (sampled tree maximum), startup (spawn to the first trace
span), per-phase `wall_ns` (union of the kind's `JET_TRACE_FILE` phase spans),
`span_ns` and `cpu_ns` (the 20 ms tree CPU curve across those spans),
`JET_PHASE_COUNTERS` lines, artifact and normalized diagnostic digests, and
the run's stdout digest. The receipt also records the machine, load and other
processes above 1 GiB, the commit, the compiler's digest and its build
profile (read from the build's Cargo manifest, or `--compiler-profile`).

Fails (exit 1): a nonzero, timed-out or incomplete compile, error
diagnostics, a missing trace or open phase span, a wrong golden, a missing
sample, artifacts or diagnostics differing across samples or arms, and in
the full lane every cell gate (memory, the 2 s and 150k lines/s thresholds,
and each `bench.json` phase budget, reported with its owning cards). The quick
lane compares per-phase medians, the whole frontend and peak RSS with
`Tools/perf/baseline.json` budgets (15%), skipping baseline values under
100 ms or 64 MiB; `--write-baseline FILE` records a passing run's baseline.

No compiler has a job-count control before D-JOBS1, so the single arm is
recorded as uncontrolled (`--single-control env:NAME=VALUE|arg:FLAG` sets one).
