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
sources without rewriting their bodies. Each subset keeps the manifest-listed
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
manifest and appends the existing `Compiler/JetCli/main.jet` entry. Unlike the
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
runtime beside `jet`. Stage 2 and stage 3 must have byte-identical objects and
executables; `native-compiler.json` records successful production and comparison.
`--stages 1` builds one successor without claiming a fixed point.

`--dry-run` performs real source assembly and records the entry and command
plan without running a compiler or manufacturing outputs. The explicit
`--seed-build --jet /path/to/rust-host --out /disk/cli-seed` bootstrap exception
builds the same production entry through the host's `build --release` route.
Its `seed/jet` is a Jet-built, Rust-origin seed, **not** a Rust-free delivery;
`seed.json` records that distinction. It uses no private `Tests.rs` main.
Builds and full fixed-point runs still require Main's measured proofq admission.

## Compiler-owned registries

`Tools/agent/compiler-registries.mjs` is the canonical census and runner for
executable `Compiler/*/Tests` Jet registries and the complete testkit case corpus.
It uses the canonical bootstrap assembly order, appends one registry plus its
shared support, and keeps generated units outside the checkout. It does not run
Cargo, select a Rust oracle, filter tests, accept skipped tests, or treat
known-failing cases as passes.

```sh
Tools/agent/jet-env node Tools/agent/compiler-registries.mjs \
  manifest FULL_CURRENT_HEAD --out /disk/compiler-registries
Tools/agent/jet-env node Tools/agent/compiler-registries.mjs \
  run --jet /path/to/stage3/jet --manifest /disk/compiler-registries/manifest.json \
  --receipt /disk/compiler-registries/receipt.json
```

Manifest generation is source assembly only; execution still needs Main's
measured proofq admission. The manifest binds the current head, source manifests,
compiler source bytes, registry inventory, assembled units, testkit sources and
case IDs. Unsupported fixture transports are explicit `BLOCKED` entries and
prevent a successful receipt, never silently disappear. A `run` registry counts
one complete registry callback and prints an exact terminal counted sentinel.
`test` registries use the existing `jet.test.v1` JSON reporter, requiring the
complete test-name multiset, exact outcome count, and zero skips or failures.
Testkit builds natively with the supplied compiler, enables opt-in cases, and
requires every case outcome to be `PASS`. Logs and an incrementally written
receipt preserve every failure while the runner continues through the census.
Rust bootstrap tests are separately inventoried in `retired-with-Rust.json`;
they are not Jet registry entries or cutover blockers.

