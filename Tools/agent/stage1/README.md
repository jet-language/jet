# Stage-one compiler tooling

These scripts drive retained bootstrap host binaries and generated compilers.
Code lives here; candidates, locks, logs and rung output stay in
`$HOME/.cache/jet-dev`. `JET_REPO` overrides the checkout derived from this directory.
Use the repository scripts directly; cache copies are retained only for running agents.

```sh
Tools/agent/stage1/fast-cand4.sh cand29f 12
Tools/agent/stage1/ladder.sh -r L1,L2,L3 "$HOME/.cache/jet-dev/cand29f/jetc0" "$HOME/.cache/jet-dev/stage1/ladder29"
Tools/agent/stage1/selfcheck.sh -c "$HOME/.cache/jet-dev/cand29f/jetc0" -b
Tools/agent/stage1/fixedpoint.sh -c "$HOME/.cache/jet-dev/cand29f/jetc0" --stages 3
```

`fast-cand.sh` uses the retained host3; `fast-cand4.sh` uses host4.
They require the matching `host3.path`/`host4.path` cache file and overlay checkout;
`fast-loop*.sh` accept `JETC0_LOOP_HOST`, `JETC0_LOOP_OVERLAY` and `JETC0_LOOP_DIR`.
`lib.sh` documents the environment protocol, caps, backend pinning and locking.
`goldens.sh` stages fixtures from `fixtures.list`; reports and gates are sibling tools.
Linux systemd user scopes, flock, Node, Python runners and the project devshell are required.

`ladder.mjs rungs SOURCE_TREE RUNGS_DIR` assembles the selected tree's manifest
sources without rewriting their bodies. Staged sources have independent inodes
(copy-on-write extents where supported), never hard links: creating a rung must
not invalidate the original package's single-link authority or let private
rung edits change the checkout. Each subset keeps the manifest-listed
shared sources outside `Compiler/`, including Jetpack's canonical syntax names,
in manifest order. Keep its entry anchors aligned with the package APIs; the
lowering anchor borrows the checked rows and uses a single
compiler thread. `Compiler/JetCodegen/Tests/run-field-owner-fixtures.mjs OUTDIR`
adds the field-owner contracts to the freshly assembled L4 unit and generates
their package budget, including the compiler's `Time.Wait` effect. Fix source or
generator inputs and regenerate; never patch the emitted fixture unit.

The private assemble HOME intentionally still uses `.cache/jet-luna/compiler-bootstrap`.
It must match the running host4's embedded `Compiler/Bootstrap/Tests.rs` constants.
Change the compiler bootstrap files, both copies of `lib.sh`, `ladder.mjs` and
isolated-check consumers together with the host rebuild, not independently.
See [gates/README.md](gates/README.md) for growth and phase-profile gates.

## Production native compiler

`Tools/agent/native-compiler.mjs` assembles the canonical bootstrap source
manifest and appends the existing `Compiler/JetCli/run.jet` entry. Unlike the
private factory harness, the resulting executable accepts real CLI arguments.
Run through `Tools/agent/jet-env`, with a fresh disk-backed output directory:

```sh
Tools/agent/jet-env node Tools/agent/native-compiler.mjs \
  --jet /path/to/cli-capable-jetc0 --runtime-dir /path/to/jet-runtime \
  --out /disk/native-compiler
```

The seed must execute the Jet compiler's Jet CLI code, not the Rust reference
CLI. Plain `build` selects native O0 lowering, ELF object emission and the Jet
static linker. The runtime directory must have `native.key`, the matching
`KEY/jet_runtime.pack.o`, and `provenance.json` identifying a Jet-native pack
with zero Rust/LLVM objects and its SHA-256. Every output stage receives this
runtime beside `jet`. Stage 1 and stage 2 must have byte-identical objects and
executables; `native-compiler.json` records successful production and comparison.
`--stages 1` builds one successor without claiming a fixed point.

`--dry-run` performs real source assembly and records the entry and command
plan without running a compiler or manufacturing outputs. The explicit
`--seed-build --jet /path/to/rust-host --out /disk/cli-seed` bootstrap exception
builds the same production entry through the host's `build --release` route.
Its `seed/jet` is a Jet-built, Rust-origin seed, **not** a Rust-free delivery;
`seed.json` records that distinction. It uses no private `Tests.rs` main.
Builds and full fixed-point runs still require Main's measured proofq admission.

The sharded stage-zero factory is a separate bootstrap input, not a CLI seed.
Its explicit `JET_BOOTSTRAP_*` protocol can feed the Jet compiler's own
`JET_FRONT_LIBRARY_OBJECT_OUTPUT` and `JET_FRONT_NATIVE_OUTPUT` paths. Produce
and package the runtime first, then set `JET_RUNTIME_PACK` to
`<jet-runtime>/<key>/jet_runtime.pack.o` inside that packaged runtime directory
(its `native.key` must name `<key>`) while lowering the exact assembled CLI unit.
The native feed runs `jet build`'s own native route
(`jet_cli_native_executable` in `Compiler/JetCli/Source/Cli/Host.jet`) at O0
with that directory as the runtime directory: the same C closure (inline
`#FFI(c)` bodies, c@path archives, the musl platform from
`<jet-runtime>/foreign/platform` or the toolchain), the same runtime object or
pack, and the same link order, so stage1 matches stage2 when stage1's
`jet-runtime/` is a copy of that directory. The C closure needs the same C
toolchain environment as the stage2 `jet build` (`PATH`'s or `CC`'s compiler,
`AR`, `NIX_HARDENING_ENABLE`, and `JET_PACK_MUSL_LIB` when
`foreign/platform` is not packaged). `lib.sh` forwards
these request paths and `JET_KEEP_GOING`, `JET_KEEP_GOING_REPORT`, and
`JET_KEEP_GOING_SKIP` through its otherwise-clean environment. Under
`JET_KEEP_GOING=1` the native feed runs whenever the front end produced MIR:
each function whose native lowering or x64 compile fails becomes a trap, and
every lowering, object and link finding is one more `native.*` census row, so
one run reports every backend defect. A factory process
exit alone admits neither object nor executable: check its reports, the emitted
files, their hashes, and the empty all-phases failure census before publishing
the native CLI. The factory's own Rust sharded build remains the explicit
bootstrap exception; subsequent native stages and their runtime are Rust-free.

The sharded path is admitted by a `jet-sharded-stage-zero/v1` receipt containing
the actual retained factory and generated-source hashes, source commit/tree,
and successful canonical build/hello result. `native-factory.mjs` checks those
identities before calling the environment protocol. Its native/library receipt
may have zero Rust-source bytes, but must be complete, have zero reports, select
the AOT factory, and produce an empty all-phases TSV. The library helper uses
the real lifecycle and platform archive inputs to write ABI JSON and the
Rust-free pack, not just an unlinked object.
The canonical sharded backend uses `cargo build --keep-going`, so every reachable
shard rejection is collected before the bootstrap build stops. Cargo test
invocations use `--no-fail-fast`, never the build-only `--keep-going` flag.

```sh
Tools/agent/jet-env node Tools/packaging/prebuild-runtime.mjs \
  /disk/retained/jetc0 /disk/native-runtime \
  --factory-record /disk/stage-zero/receipt.json --repo /disk/pinned-source
Tools/agent/jet-env node Tools/agent/stage1/native-feed.mjs \
  --factory-record /disk/stage-zero/receipt.json \
  --runtime-dir /disk/native-runtime --out /disk/native-fixedpoint \
  --repo /disk/pinned-source
Tools/agent/jet-env node Tools/agent/stage1/native-feed.mjs \
  --self-rebuild --out /disk/native-fixedpoint
```

`native-feed.mjs --dry-run` performs only canonical CLI-unit assembly and records
the future artifact dependencies; it does not admit missing upstream artifacts.
The real run lowers that exact unit through `Entry.native` to stage 1, then
uses stage 1's real `build` argv to compile the same unit into stage 2. Every
all-phases census must exist and be empty. The fixed point is whole-file byte
identity of the stage 1 and stage 2 objects and ELFs (`comparisons.json`); the
GREEN `native-compiler.json` (`fixedPoint: true`, `stages` = the stage 1/2
pair, `jet` = `stage2/jet`) is published as soon as it holds. The stage 3
self-rebuild is a separate follow-up: `--self-rebuild --out <published dir>`
has stage 2 compile the same unit into stage 3 and writes `self-rebuild.json`
(`selfRebuild.identical`: stage 2 == stage 3, whole files); it never gates the
publication and may run beside smoke. Producer/harness hashes are recorded
separately from the single pinned compiler/runtime source identity.

The factory and every CLI stage run inside the source export's
`Tools/agent/jet-env` shell entered from the caller's environment (the
factory's drops inherited `JET_*`/`RUST_*`), so one C toolchain builds the
inline-C closure; each records its resolved `cc`/`ar` paths, versions and
`NIX_HARDENING_ENABLE`, and a CLI stage whose toolchain differs from the
factory's is refused. Each runs under GNU `time -v` (seconds, peak RSS) with a
1 GiB main stack set by `prlimit` before exec: the native compiler runs on the
main thread, and the Rust thread-stack variable does not raise it. CLI stages
get `JET_COMPILER_IDENTITY` from the stage-1 factory receipt's
`compiler_source_identity` and a `NATIVE_STAGE_TIMEOUT` (default 10800 s).
Compiler thread admission (`compiler_threads_admit`) starts from the CPUs a
process may run on. That is its only input the chain can set: the factory
takes no `--threads` cap. So the stage 1 factory and every CLI stage run
under `taskset -c 0-(N-1)` with N = `NATIVE_COMPILER_CPUS` (default 4), and
the receipt records N as `compilerCpus`. A self-rebuild reuses its
publication's N. The admission's memory term (MemAvailable and cgroup
headroom) can still lower the worker count below N when free memory cannot
hold N threads' scratch.
`native-fixedpoint.sh` needs a real CLI builder (`stage1/jet` or `stage2/jet`,
never `jetc0`) with `jet-runtime/` beside it, `JET_TOOLCHAIN_ROOT`, and the
same identity (`--factory-receipt`); run it through the same `jet-env`.

`native-lowering-census.mjs FACTORY_RECORD PINNED_SOURCE FRESH_OUT` may run in
parallel with the pack producer. It uses the existing `Entry.native` object
boundary and deliberately supplies no pack: all front-end TSV and backend
findings are retained, while linking is **BLOCKED**, never passed. Even a clean
object census qualifies neither the factory nor a CLI. The real native feed
runs after the pack and must establish the complete linked fixed point.

## Compiler-owned registries

`Tools/agent/compiler-registries.mjs` is the canonical census and runner for
executable `Compiler/*/Tests` Jet registries and the complete testkit case corpus.
It uses the canonical bootstrap assembly order, appends one registry plus its
shared support, and keeps generated units outside the checkout. It does not run
Cargo, select a Rust oracle, filter tests, accept skipped tests, or treat
known-failing cases as passes.

```sh
Tools/agent/jet-env node Tools/agent/compiler-registries.mjs \
  manifest FULL_COMMITTED_SHA --out /disk/compiler-registries
Tools/agent/jet-env node Tools/agent/compiler-registries.mjs \
  run --jet /path/to/stage3/jet --manifest /disk/compiler-registries/manifest.json \
  --receipt /disk/compiler-registries/receipt.json
```

Manifest generation is source assembly only; execution still needs Main's
measured proofq admission. Both commands use the same immutable SHA export from
`proof-source.mjs`, validating every exported Git blob rather than racing the
live worktree. The manifest binds that commit and export receipt, source
manifests, compiler source bytes, registry inventory, assembled units, testkit
sources and case IDs. Unsupported fixture transports are explicit `BLOCKED` entries and
prevent a successful receipt, never silently disappear. A `run` registry counts
one complete registry callback and prints an exact terminal counted sentinel.
`test` registries use the existing `jet.test.v1` JSON reporter, requiring the
complete test-name multiset, exact outcome count, and zero skips or failures.
Testkit builds natively with the supplied compiler, enables opt-in cases, and
requires every case outcome to be `PASS`. Logs and an incrementally written
receipt preserve every failure while the runner continues through the census.
Rust bootstrap tests are separately inventoried in `retired-with-Rust.json`;
they are not Jet registry entries or cutover blockers.
The owner-approved `Tools/testkit/retired-with-rust.tsv` is the single explicit
retirement list for Rust-dependent case and compiler-registry sources. Normal
testkit discovery and the manifest's `retiredWithRust` section read those exact
IDs and reasons.
Retirement changes the active corpus; the runner uses no CLI filter or shard,
and accepts no skipped outcome among the remaining cases.
Testkit consumes Prelude `Dyn` directly: JSON `Number` tokens retain their
spelling, numeric assertions preserve integer precision and accept equivalent
fraction/exponent spellings, and diagnostic edit offsets use strict integer
accessors.
Backend/runtime transports reuse the existing Node assemblers and verdict
checkers, with production native Jet CLI compilation and permitted system C
drivers. `run --runtime DIR` explicitly names the admitted runtime directory;
otherwise transports use `jet-runtime` beside the supplied compiler. Its
`native.key`, Jet-only object, full pack and matching zero-Rust/LLVM provenance
are required. Transport command evidence is retained in the same receipt.
The compiler environment C witness runs in both normal and empty-environment
processes. Compiler filesystem/wait/status routes use their C witness with a
disk-backed scratch path. Source-owned compiler routes use their standalone
native `#Test` registry, without appending the compiler aggregate.
All standalone/compiler-aggregate registries receive private disk-backed
`TMPDIR` scratch; fixtures requiring `JET_CLI_FMT_TEST_ROOT` receive that same
declared root. Missing admitted runtime prerequisites produce explicit blocked
receipt entries, never successful or skipped outcomes.
Compiler-needed JSON, URL and math/text raw fixtures use their exported native
zero-success callbacks through the same C-driver transport. GoldenLower and
LintCorpus receive every committed feature golden with an output/status oracle:
the native checked-MIR driver is built once, stages the real feature projects,
and feeds the complete corpus to the original Jet registry. GoldenLower then
uses the existing native run-only golden comparator; LintCorpus requires every
lint count and findings file to be empty. No synthetic input program substitutes
for either corpus.
Dyn/native-layout coverage initializes the real runtime and runs both the
permuted native descriptor round trip and the registered DateTime E3001 stop.
Optimizer fixtures append the actual LowerFixtures MIR builders (omitting its
separate report/run entry); trace validation appends JetTrace shared helpers and
counts every resulting `#Test` block, including the helper file's tests.
Export-only ABI doors with no test entry remain visible as support inventory,
not invented standalone registries.

