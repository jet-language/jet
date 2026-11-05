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

The testkit package's own `fn test` is the fixture gate. Run it from the
repository root with the compiler under test in `JET`:

```sh
JET=/path/to/jet jet test Tools/testkit                    # every suite
JET=/path/to/jet jet test Tools/testkit "suite golden"     # one suite
TESTKIT_ARGS="--filter basics/fizzbuzz --tier o0" JET=/path/to/jet jet test Tools/testkit "suite golden"
JET_CI_EVIDENCE_DIR=/disk/scratch JET=/path/to/jet jet test Tools/testkit "suite golden"  # writes golden.json
```

Each suite (`suite golden`, `suite ui`, `suite cases`) is one test. The suites
a run selects share one pool sized by cores and free memory, so a filtered run
runs and reports only its own suite. Rows print as cases finish; the reports
and each failing test's list of case rows keep suite-then-path order.
`TESTKIT_ARGS` carries the kit flags below, split like
`.case` step words.

`o0` runs through `jet run`; `o1` builds an ordinary artifact; `o2`
builds with `--release`. Every tier compares against the same expected bytes,
except that `<stem>.<tier>.out`, `.err.out` or `.stderr.out` replaces the stem's
streams for that tier alone, for a program whose output follows its build
profile. A rejected build compares its stderr against the tier's `.err.out`
with exit 1, and no run follows it. On `o0` an entry whose sidecar spells no
steps gets no separate `jet check`: its `jet run` reports the rejection, and a
diagnostic report (`Error [jet.…]`) expects exit 1 there too.
A PASS/FAIL disagreement is reported as `tier-split`, not blessed separately.
An E2104 build refusal is `UNSUPPORTED` only on a native tier.

Discovery covers `Examples/features` and `tests/run`. A package or workspace
root stops descent; its `run.jet` is the entry, not its support modules.
`.out` expects successful stdout, `.err.out` expects stderr and exit 1 or 70,
and `.stderr.out` pins successful stderr. `.stdin` contains exact piped input.
`<stem>.test.out` is the entry's `jet test` stdout, checked on each tier once
the run passes: `o0` tests with `--optimize=none`, `o1` with
`--optimize=basic` and `o2` with `--release`, a package or workspace entry
tests as its directory, and `<stem>.<tier>.test.out` replaces it for that tier
alone. Its last line is the harness summary, and the exit must be 1 when the
summary counts a failed or unexpected-pass test, else 0.
Golden sidecars live beside expected streams as `<stem>.case`; runtime-test
streams and sidecars live beside their source or project directory.

Sidecars replace filename-specific harness rules. `requires:` declares GTK,
foreign tools, or environment requirements; missing hosts report `SKIP-HOST`.
`run: none` keeps a serves-until-stopped entry compile-only. `host-port: N`
declares a fixed TCP port the program binds; the kit and
`Tools/agent/run-feature-examples.mjs` hold the host-wide flock
`/tmp/jet-host-port-N.lock` while the case runs, waiting at most one step
timeout, so concurrent runs on one host take turns. `normalize: argv0`
pins CLI usage to the source name. Ordered `step:` commands accept quoted argv,
`stdin:`, `exit:`, exact `stdout:` / `stderr:`, and `file:` assertions.
`step: build` invokes the selected tier; `step: built [args…]` runs its artifact
(and inserts a build if necessary). Ordinary setup commands can precede build.
The artifact is `.jet/build/<entry-stem>` (dots in the stem become underscores)
under the nearest package root, or under the entry's directory for a standalone
program. A programmable build with a named executable declares `artifact: <name>`
in its sidecar instead.
The kit checks that the artifact exists after a successful build; progress
messages do not identify the executable.
The default expected streams apply to the last step unless it has explicit
stream assertions. `stdout: one-of FILE FILE` permits only those exact,
documented host-dependent alternatives and must be edited by hand.
Golden sidecars and CLI cases share one step sequencer and assertion checker.
In a CLI case, `step: built [args…]` runs the artifact from the preceding
successful `step: jet build <entry>`; Golden also supplies its implicit build.
Shared kit record types, the step sequencer and the gate entry are colocated in
`run.jet`. List field defaults use explicitly typed empty lists, just like kit
constructors.
Fixture copies preserve file permission bits, including executable tools nested
under `state/` when that directory is staged as `.jet/` in the case workspace.
Integer right-hand sides of `file: P size`, `stdout: lines` and numeric
`json:` comparisons (`=`, `!=`, `<`, `<=`, `>`, `>=`) may be `{cap:x}+N` or
`N*{cap:x}`: non-negative integer `+` and `*` only, no other operators.
Integer transform arguments on an assertion subject, such as
`file.slice({cap:a},{cap:b}): P ~ "text" # reason`, may be whole `{cap:x}`
tokens; such a subject is matched raw, without normalization, so captured
byte offsets stay exact.
`recv LABEL [line] [collect] [until PREDICATES] [timeout|within D] [every D]
[optional]` reads a live session's stdout. Content-Length sessions read
frame bodies, `line` sessions and the `line` keyword read lines, and other
sessions read raw bytes. Without `until` the step's stdout is the next unit
(raw: whatever has arrived). With `until`, raw output accumulates until the
predicates hold, lines accumulate through the first line that holds, and
frames before the first matching frame are discarded; `collect` tests the
newline-joined units gathered so far. Predicates are `$PATH = JSON`,
`ndjson …` or stdout assertion forms joined by `&&`. Without a deadline the
run's step timeout applies; `optional` passes with partial output at the
deadline instead of failing. A later `close` or `wait` reports only the stdout
no `recv` consumed.
`pty: true [size ROWSxCOLS]` runs a step or session on a terminal (0x0 or no
size leaves the size unset); its stdin and stdout share the terminal, so
stdout is the transcript, echo and `\r\n` included. On a step,
`pty-send: "TEXT" after D` and `pty-close: after D` run D after the previous
send or close (the first after spawn), and `pty-resize: ROWSxCOLS at D` runs D
after spawn. On a session they may also follow its `recv` and `send` lines and
run where they stand. A close types the terminal's end-of-file character; a
session's `close` does the same before waiting, since closing the terminal
would hang up the child. Partial wiring such as `pty: stdout,stderr` is
refused as `pty-stdio`.

Use `--list`, `--cases FILE` (exact repository-relative IDs), or `--filter TEXT`
to select cases. `--shard I/N` partitions the sorted corpus deterministically.
Scratch must be disk-backed and outside the checkout; it is staged and removed
per case. `JET_TEST_STATE_DIR` (default `state/` under the scratch root, under the
same rule) outlives runs: `history/<suite>.tsv` keeps each entry's last wall
time and resident peak, and `ffi/` is the FFI cache every run shares.
Selected entries run in one `task.group`. Measured entries start longest
first, after every unmeasured one. An entry starts once fewer than `--jobs N`
run (default one per core, capped so each keeps 1 GiB of the available
memory) and its claim fits in the memory budget beside the running ones'; the
first always starts. The claim is half again the entry's last peak (at least
64 MiB; 1 GiB unmeasured). The budget is the memory the kit may still claim
under its cgroup caps, or at most N GiB with `--jobs N`. Every child process
keeps its own timeout and an 8 GiB resident-memory guard, raised to the
entry's own claim and never above that room; a case's `limit: resident=`
tightens it. Each entry's verdict diffs and rows
print as soon as it finishes; the results file stays in corpus order and
records every entry's `wall_ms` and `peak_kib`.
`--timeout SEC` is each child's step timeout, 60 s by default; a step that
needs longer declares `limit: time=SEC`, which also makes the overrun a
failure of that step. Timeouts must be positive seconds within `Duration`'s
representable range; out-of-range values are refused before corpus discovery
with exit code 2. A known-failing case waits at most 10 s for each answer
(`recv`, `poll`, `await`, `wait` with a deadline, `repeat while`), so a
product that never answers fails fast, and one that now answers still shows
as an unexpected pass.

UI comparison uses the corpus-relative source ID for the primary diagnostic
location header, whether the compiler printed that ID or just its basename.
This same normalization applies to expected and actual text; imported-source
headers, line/column positions, diagnostic codes, and message bodies remain
exact. `Tools/testkit/Ui.jet` owns the rule.

Expected files are read-only unless explicitly selected for blessing:

```sh
TESTKIT_ARGS="--bless Examples/features/basics/fizzbuzz.jet" JET=/path/to/jet jet test Tools/testkit "suite golden"
TESTKIT_ARGS='--filter basics/ --bless-all --reason "compiler output change"' JET=/path/to/jet jet test Tools/testkit "suite golden"
```

`--bless ID` selects only named cases. `--bless-all` requires a nonempty reason;
neither blessing mode accepts `--shard`. Only the first selected tier writes.
UI/lint blessings also refuse fewer rendered diagnostics without an explicit
`--allow-shrink ID`. The kit prints selected IDs and the reason for the commit
message. Blessing never weakens a `one-of` contract.

Crypto UI fixtures inherit the standard library's declared `CryptoError -> Err`
and `FileCryptoError -> Err` conversions (D-FAIL-CONV2=A, D-CRYPTO-ERRFAM1=A).
Valid Core crypto calls in `fn run()` therefore do not expect E2402. Keep
non-convertible user-error witnesses separate from crypto boundary misuse
(E2702), and retain type, effect, and authority precedence diagnostics.

For same-code snapshot refreshes, compare the diagnostic text with its current
Jet emitter or active registry row, and source excerpts with the fixture.
Do not bless different codes, lost diagnostics, or a Rust-host/native-tier
discrepancy merely because one compiler printed it.

## Collect-all closeout

Run the complete census with an already-built compiler and a committed source
tree:

```sh
JET=/path/to/jet JET_TOOLCHAIN_ROOT=/path/to/source \
  Tools/agent/jet-env node Tools/agent/goldens-census.mjs \
  --jobs 2 --timeout 90 --registry-timeout 600
```

The command exports that source commit into private disk-backed scratch. Tests
and examples run from a separate mutable copy, never from the supplied checkout.
It builds the native testkit once, runs its complete golden/UI/CLI-case corpus
with deterministic shards and all three tiers, runs the compiler registries
with independently sealed shard receipts, and runs Core's named `#Test` suites
and `Examples/suites` goldens. The compiler itself is never rebuilt.

All children share **one total memory scope**, not one allowance per shard.
The default is 4 GiB; `JET_CENSUS_MEMORY=8G` selects 8 GiB when the coordinator
permits it. Launch is refused below 12 GiB `MemAvailable`. `--runtime DIR`
selects a matching Jet-native runtime directory for native registry transports;
otherwise the compiler's adjacent `jet-runtime` is used. Missing prerequisites
are recorded as blocked/skipped, never as passes, so native-only transports and
host-specific fixtures stay visible in the census.

The codec/file runner's `--test CodecCSV|CodecEnv|DynCodec|CompilerFileLeaves`
mode assembles the selected Jet test with the canonical runtime sources,
lowers a fresh native library, and calls its exported fixture inside
`jet_rt_main` from a C driver. Every fixture must return zero with empty output;
receipts preserve its actual compiler, object, driver and executable identities.
The default admitted-pack C witness remains separate and does not replace
the compiler-file fixture's write/read/refill assertions.
Files-walk and host-parallelism recipes use their existing five-stage runners:
assembly, native lowering, import/export ABI audit, C witness link and execution.
Both require the canonical zero-status, empty-output `run.receipt.json`; merely
producing an object or running an ABI audit is not a test pass.
Process/terminal `--test` recipes likewise compile their Jet fixtures into a
fresh library. `JET_PROCESS_JET_FIXTURES` selects the real exported helpers
instead of the admitted-pack witness's local C helpers; exact witness output
and zero exit remain required. The default pack witness and its typed-field
case are unchanged. An unexecuted recipe or missing pack is never a pass.

Evidence goes into a unique `census-*` directory below `--scratch DIR`, else
`$JET_SCRATCH_ROOT/goldens-census`, else `~/.cache/jet/goldens-census`. `census.tsv` records
the test, kind, pass/fail/skip, and first diff/error lines; JSON retains raw
statuses, tiers, repro commands and complete log paths. Native testkit result
records retain their diff alongside the existing detail and exit fields.
`remaining-root-table.{json,tsv}` lists counts, complete affected-test lists,
sample repros and owner proposals. `--roots JSON` carries an earlier root
classification forward without blessing a failing output. `latest.json` points
to the run, and the latest TSV/root tables are also published at the scratch
base. Counts are verdict rows (including separate optimization tiers); compiler
registry receipts and named compiler Tests are reported as separate kinds.
An interrupted run is explicitly incomplete, and any non-pass exits nonzero.

Compiler fixture text follows the same language rules: escape literal braces
in quoted parser input, and use explicit filesystem cleanup rather than an
arbitrary `defer { ... }` block. S63 permits `defer close(^resource)`, not general
deferred actions.
