# Compiler scaling gate (Tower #4319, #4601)

Dependency-free Node tools. The gate compiles every corpus shape at a size
ladder with every compiler and fails when any phase or work counter grows
faster than k * N log N, naming the phase and the counter that dominates it,
or when a phase is slower than its lines/s floor.

```sh
node Tools/perf/scaling/generate.mjs --list                 # every shape
node Tools/perf/scaling/generate.mjs web 125000 NEW_DIR     # one corpus
node --test Tools/perf/scaling/scaling.test.mjs
Tools/agent/jet-env node Tools/perf/scaling/run-gate.mjs --compiler "rust=jet:$RUST_JET" \
  --compiler "jet=bootstrap:$JETC0" --mode counters --jobs 2 \
  --memory-max 8G --timeout 25 --work-dir $HOME/.cache/jet-dev/scratch/ScalingGate2/landing
node Tools/perf/scaling/report.mjs --runs-dir RUNS_DIR --output report.md
```

Set `RUST_JET` and `JETC0` to the current dev head's shared pair recorded in
`~/.cache/jet-dev/dev-artifacts.json`, after that head includes these counters.
Changing compiler binaries requires fresh receipt roots. Older candidates may
be additional explicitly labelled historical comparisons, not the current gate.

Every corpus is measured in two layouts: `files` (as generated, many files)
and `single` (one main.jet). A compiler that fails the multi-file layout fails
the gate. Run `jet` compilers through `Tools/agent/jet-env` (rustc on PATH);
the single `run-gate.mjs` command above is the landing counter proof: it covers
every stress and domain shape, both layouts, and all rungs, with bounded
parallel compiles. Counter mode gates deterministic work counters and compile
outcomes, not contended time/RSS or lines/s floors. A timed-out rung fails with
its open phase. Other compile failures retain the last observed open phase when
available, without being classified as timeouts. Both modes attempt
every rung despite earlier compile failures, retaining each rung's failure. Timing
mode remains serial and gates wall/RSS growth and floors; only its measurements
feed 1M-line projections.
Every baseline, rung and profile compile is atomically checkpointed. Repeat
the identical command to resume without retrying completed rungs (including
failures); changed options or compilers require a new work directory.
`--work-dir` contains evidence only (receipts, reports, traces and logs).
Generated projects, compiler stores/output and `perf.data` go under
`--build-root` (default `/mnt/jetscratch/scaling-gate`), keyed by evidence
directory; each point records its `build_dir` for artifact cleanup after landing.
Re-render a profile with `node Tools/perf/scaling/profile.mjs BUILD_DIR TRACE_FILE PHASE...`,
passing the recorded build directory and the evidence directory's `trace.json`.
To queue the gate without monopolizing the quiet lane, run `enqueue.mjs` with
the compiler/work-directory options and `JET_SCALING_QUEUE_STAMP` naming the
replaced job whose mtime must be retained. It writes three
`ScalingGate-counters-N.sh` correctness jobs (all shapes, two parallel rungs,
25 s cap and 8G per compile) and one `PerfScalingGate-timing.sh` sample
(`huge-fns`, `fan-in`, all domains at 125k/1M; serial, 20 s cap and 24G).
The counter jobs share `scaling-gate.lock`; timing takes it exclusively.
Each group has disjoint receipt/report paths, recorded in `queue-plan.json`.
Jobs declare scheduler peaks of 18G (counter groups) and 25G (timing), and
check a conservative 30 GiB of build-root disk headroom before compiling.
Add active evidence/build roots to `disk-protect.txt` before admission.
`report.mjs --plan PLAN.json --wait-proofq DONE_DIR --output FILE` waits for
those jobs' completion events and merges their receipts. `--findings FILE`
appends retained probe evidence. Missing/unfinished receipts and absent timing
coverage are explicitly uncovered, never inferred from counter-mode time.

The Rust ledger indexes declarations, aliases and checked reference sites by
source module. Implicit package names resolve through one loader-owned name→file
index, not a complete set of synthetic sibling imports. Structural auto-derive
publication retains one shared canonical table of actual nominal declarations,
while derive/codegen selections remain module-local.
Empty marker-rule and non-App entry paths do not construct unused bundle contexts.
Declaration/alias lookup uses borrowed keys in module-owned maps, and canonical
module/declaration identities are cached. Structure observations retain source
order with collision-checked hash buckets; import-edge presence uses a source/span
index. Body snapshots borrow loader alias roots rather than cloning that set.
Semantic state facts and readonly codegen program facts are frozen once and
shared across module contexts. Namespace callable values retain the real defining
module; constants, nominal shapes, methods, traits and erased unit/alias metadata
use local-first indexed lookup. Canonical boxing, iteration/indexing hooks and
trait-method flags are program facts; generated instance nominals retain one
registry/declaration owner rather than a copy in every consumer. Display spelling
lookups use an inverse scope index, and alias metadata keeps its declaration
scope while preserving transparent substitution. Derive helper snapshots are
shared by provider scope and built only for actual providers; checked comptime
projections are demanded only when an expansion will use them. Comptime nominal
projection borrows the declaration ledger: each source module projects its own
types and written imports without cloning the global canonical identity set or
rescanning the global nominal map. Source-body comptime scopes query the same
owner index, freeze own canonical callable signatures, and project only the
namespace functions their source mentions. Runtime and comptime callable values
share one foreign/generator/failure-carrier return policy on `Func`. Qualified
package calls and imported callable values retain their actual defining-module
binding. Devtools panel validation borrows one package struct index. Compile-time value keys
(`jet.comptime-value/v2`) use shared ordered
peer-source commitments, excluding the evaluating file just as before; edits to
unreachable functions in that file do not invalidate its constant-value reuse.
Package item-reuse log rows aggregate all member files of the checked package.
Focused reuse checks count unchanged bodies across that package and require
warm-store diagnostics to match a fresh-store check byte for byte.

Shapes (`corpora.mjs`, `domains.mjs`, helpers in `package.mjs`): stress shapes
grow one dimension with N (files, small functions, statements per function,
nesting depth, enum width and arm tables, impls/traits, generic
instantiations, comptime items, table elements, Core imports, call depth,
fallible functions, diagnostics, interpolation holes, call sites) at N, 2N, 4N,
8N; domain shapes are realistic programs (web, game, cli, data, gui,
embedded, compiler, scientific, generic-lib, generated) at 125k/250k/500k/1M
code lines. Adding a shape is one builder function plus a registry row; see
the header comments of `corpora.mjs` and `domains.mjs`. `mixed` (`mixed.mjs`)
remains the compile-throughput cell's pinned program.

Per rung the gate records wall/user/RSS (GNU time), per-phase wall from the
`JET_TRACE_FILE` phase spans, the trace's span and item counts
(`trace.<kind>.*`), and `JET_PHASE_COUNTERS` lines (`counter.<phase>.<name>`):
the Jet half emits `sink.progress("counters.<phase>.<name>", n, -1)`, the
Rust half counts through `jet_foundation::WorkCounters` (tokens, items,
scope lookups, item/impl scans, whole-program copies, functions lowered and
emitted, allocations), drained at every phase end by `jet_driver::Trace`.
The Jet half counts module item scans, ordinary and return-view-summary body
checks, method jobs and impl-row scans in sema; function rows in lowering;
global-instruction scans, impls, top-level functions and item-output lines in emit.
Ordinary checks count prepared parallel bodies and actual serial fallback checks,
not reuse of a prepared result. Summary checks are counted where the summaries run,
whether the module prepares parallel bodies or checks serially.
Counters are deltas per loop invocation, so repeated module passes sum rather
than overwrite one another. They add no traversal or per-item sink calls.
Line packing and single-file assembly append iteratively, so large rungs do not
depend on the JavaScript engine's call-argument limit.
Series are baseline-subtracted (an N=1 rung), judged between the first and
last measurable rung against N log N (bounds: counters 1.25, time/RSS 1.5),
and projected to 1M lines. Floors come from
`Tools/perf/throughput/bench.json` phase budgets plus 1M lines in 10 s end
to end. `--profile` re-runs the top rung of a failing series under perf and
attributes samples per phase to source functions (`profile.mjs`). Exit 0
pass, 1 failure, 2 invalid invocation; the receipt is JSON in `--work-dir`.

Import bindings retain the exact target module index resolved during their
registration pass (including an absent Core namespace target). The pass memoizes
each identity once; name-fact projection uses the stored index instead of
resolving every alias in the dense implicit-import surface again. Registration
graphs keep their module order through cloning and staged value application.

The focused deterministic regression runs the unchanged comptime/files corpus
at 128, 256, 512 and 1024 using an already-built Jet compiler:

```sh
Tools/agent/jet-env node Tools/perf/scaling/module-lookups-proof.mjs \
  --compiler /path/to/built/jetc0 --root FRESH_PROOF_DIR
```

It requires a successful baseline and every rung, then bounds baseline-subtracted
`counter.sema.check.module_lookups` by a linear envelope with 25% packing slack.
The proof does not rebuild the compiler or invoke the generated Rust backend.
It writes the complete scaling receipt and a focused counter verdict; unrelated
gate failures remain visible in the complete receipt. Run it only on a compiler
containing the binding-index change, not by retrying predecessor failures.
