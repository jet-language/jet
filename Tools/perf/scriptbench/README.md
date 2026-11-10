# ScriptBench

ScriptBench measures Jet against the scripting runtimes on fifteen script workloads (#5072 c7).
`manifest.json` is the single source of truth for the method, the comparator, the pinned peer
runtimes and the workloads. The runner and the input generator are Jet programs.

```sh
jet Tools/perf/scriptbench/Bench.jet --list
jet Tools/perf/scriptbench/Bench.jet --data "$JET_SCRATCH_ROOT/scriptbench/data" \
    --out "$JET_SCRATCH_ROOT/scriptbench/run" [--only w05,w13] [--jet PATH]
```

Run it from the repository root. `--list` prints every peer with its pinned store path (and
whether it is present), the tools, and the workloads; it exits 1 if a pinned path is missing.
A full run does the following:

1. `Gen.jet DATA` writes the inputs. The bytes are the same on every run, and an input that
   already exists is kept.
2. Each `wNN-*/run.jet` is built at O0 (`--optimize=none`), O1 (`--optimize=basic`) and O2
   (`jet build --release`). Every build, each `jet run` phase, and every peer must print exactly
   `wNN-*/expected.out`.
3. Timing uses hyperfine `-N`, with the jet-bin tier (the release binary), the three jet-run
   phases and every peer. Each phase keeps its programs in its own user cache,
   `OUT/cache/wNN-PHASE`:
   - `jet-run-first` is the first run with nothing kept: the cache is emptied before every
     timed run.
   - `jet-run-warm` is the warm run of the kept program.
   - `jet-run-edit` is the run after a one-line edit. It runs `OUT/edit/wNN/run.jet`, a copy of
     `run.jet` whose last line, `bench_edit: fn() -> Int { N }`, counts up before every timed
     run (`Bench.jet --edit FILE`).

   Each command gets 2 warmups and 5 timed runs per round, over 3 rounds. The commands go one
   after another, starting one position later each round, so they interleave. A command whose
   check run took over 10 s gets 0 warmups and 1 timed run per round instead.
4. The metrics are wall (the median of all runs), CPU (the mean of user + system) and peak
   RSS (the largest across runs). The w08 and w11 fixture server, `server.js`, runs only while
   its workload does.
5. One cell is printed per workload, tier, peer and metric, with the ratio Jet / peer; each
   jet-run phase is its own tier. The cells are also written to `OUT/cells.tsv`, whose `cards`
   column names the cards that own the workload's cells (the manifest's `cards`); a printed
   `LOSS` names them too. The exit is 1 on any `LOSS` (a ratio of 1.00 or more) or any wrong
   output. A jet-run number cited on a card names its phase.

The comparator is the AGENTS.md rule: Jet beats every peer on every metric, with no averaging
across peers or workloads. A peer that overruns the 120 s timeout is reported as such: its wall
and CPU cells count as a Jet win with the timeout as a lower bound, and its RSS cell is not
measured. The `report_only` cells in the manifest are printed but don't fail the run.

Each workload directory has its own README with the workload's origin and the cards it backs.
The Jet programs and inputs reproduce the 2026-10-28 audit bench and the 2026-11-01 card-close
bench that the cards cite. Where a card's numbers came from a different size or task, the
workload README says so. Run measurement jobs through proofq as `kind=measure`.
