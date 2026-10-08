# Example authoring

Examples are executable specifications under I5 in
[`AGENTS.md`](../../../AGENTS.md). The source file and its expected output are
the behavior contract; the learning order is maintained in
[`Examples/README.md`](../../../Examples/README.md). This page explains how to
add or rewrite an example without losing a consumer such as a golden harness,
editorial link, or generated artifact. Name streams, readers, and events as the
[Jet vocabulary](../vocabulary.md) defines them.

## Lead with the safe path

1. Put the short path first. Teach the safe default, such as `para_map` /
   `task`, streaming readers, or `ui.mount`.
2. Put expert control beside it. Keep the long manual form in a sibling
   `*_expert.jet` file with a one-line header naming it as the expert variant.
3. Match both execution lenses. For a runnable example, prove both commands:

   ```sh
   jet run path/to/example.jet
   jet run --release path/to/example.jet
   ```

   Update only the matching file under `Examples/features/expected/` after the
   output agrees on both paths.
4. Do not hide a floor. If a topic has a directory such as `math/` or
   `lowlevel/`, put a beginner flagship there instead of leaving the only proof
   in tests.

The short example should make the common operation obvious. The expert
variant should expose control that a reader can choose deliberately, not a
second mechanism with a different meaning.

## Keep every consumer aligned

Before moving or renaming an example, search for its path in expected output,
golden discovery, corpus manifests, harness lists, decision records, and docs.
Then update every consumer in the same cutover. A path can be part of expected
output even when the program's values are unchanged.

Use [`agent-engineering.md`](agent-engineering.md) for fixture and generated-
artifact traps. Preserve the example's meaning; do not rewrite a snapshot only
to hide a compiler or runtime regression.

## Cargo-free golden kit

Build the Jet test package with the admitted compiler, then invoke the reported
artifact from the repository root:

```sh
jet build --release Tools/testkit
testkit golden --jet /path/to/jet --tier ref --filter basics/fizzbuzz
testkit golden --jet /path/to/hosted/jet --tier o0,o1,o2 --results /disk/scratch/golden.json
```

`ref` is the temporary Rust-emitting verification tier and retires with that
compiler. `o0` runs through `jet run`; `o1` builds an ordinary artifact; `o2`
builds with `--release`. Every tier compares against the same expected bytes.
A PASS/FAIL disagreement is reported as `tier-split`, not blessed separately.
An E2104 build refusal is `UNSUPPORTED` only on a native tier.

Discovery covers `Examples/features` and `tests/run`. A package or workspace
root stops descent; its `run.jet` is the entry, not its support modules.
`.out` expects successful stdout, `.err.out` expects stderr and exit 1 or 70,
and `.stderr.out` pins successful stderr. `.stdin` contains exact piped input.
Golden sidecars live beside expected streams as `<stem>.case`; runtime-test
streams and sidecars live beside their source or project directory.

Sidecars replace filename-specific harness rules. `requires:` declares GTK,
foreign tools, or environment requirements; missing hosts report `SKIP-HOST`.
`run: none` keeps a serves-until-stopped entry compile-only. `normalize: argv0`
pins CLI usage to the source name. Ordered `step:` commands accept quoted argv,
`stdin:`, `exit:`, exact `stdout:` / `stderr:`, and `file:` assertions.
`step: build` invokes the selected tier; `step: built [args…]` runs its artifact
(and inserts a build if necessary). Ordinary setup commands can precede build.
The default expected streams apply to the last step unless it has explicit
stream assertions. `stdout: one-of FILE FILE` permits only those exact,
documented host-dependent alternatives and must be edited by hand.

Use `--list`, `--cases FILE` (exact repository-relative IDs), or `--filter TEXT`
to select cases. `--shard I/N` partitions the sorted corpus deterministically.
Scratch must be disk-backed and outside the checkout; it is staged and removed
per case. The gate supplies concurrency, not a thread pool inside the kit.

Expected files are read-only unless explicitly selected for blessing:

```sh
testkit golden --jet /path/to/jet --bless Examples/features/basics/fizzbuzz.jet
testkit golden --jet /path/to/jet --filter basics/ --bless-all --reason "compiler output change"
```

`--bless ID` selects only named cases. `--bless-all` requires a nonempty reason;
neither blessing mode accepts `--shard`. Only the first selected tier writes.
UI/lint blessings also refuse fewer rendered diagnostics without an explicit
`--allow-shrink ID`. The kit prints selected IDs and the reason for the commit
message. Blessing never weakens a `one-of` contract.
