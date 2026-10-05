# Agent runtime and cache maintenance

All mutable development output belongs under `$HOME/.cache/jet-dev`.
`$HOME/.cache/jet` is reserved for Jet's own cache. Code lives in this repository;
cache script copies remain temporarily for running agents, not as new tooling homes.
`JET_REPO` overrides the checkout derived from these tools.

```sh
Tools/agent/cache/laneS.sh node your-small-job.mjs
Tools/agent/cache/laneB.sh 8 command args
Tools/agent/cache/safe-jet.sh check path/to/program.jet
Tools/agent/cache/safe-cargo.sh check -p jet
Tools/agent/cache/isocheck.sh WorkerName Compiler/JetLexer/SomeFile.jet
node Tools/agent/cache/cache-cutover.mjs
node Tools/agent/cache/purge-bulk.mjs
```

- `laneS.sh` gives four capped small-job slots; `laneB.sh` gives two capped heavy
  slots. `safe-jet.sh` defaults to the frozen `scratch/jet-current` binary and
  three serialized slots; `JET`, `SAFE_JET_MEM` and `SAFE_JET_TIMEOUT` override it.
  `safe-cargo.sh` serializes Cargo and sets disk-backed scratch and resource caps.
  These wrappers require Linux systemd user scopes and flock.
- `isocheck*.sh` assemble a worker-specific source snapshot under `iso/` and
  check it with the frozen compiler. The priority variant bypasses the shared
  unitcheck lane and is Main-only. Both retain the old compiler-bootstrap path
  **inside a private HOME** until the coordinated host rebuild.
- `mem-guard.sh` is an opt-in long-running emergency guard: below 7 GiB available
  RAM it kills the largest eligible compiler/build process and logs the action.
  Only Main should start it; it is not a substitute for job memory caps.
- `cache-cutover.mjs` dry-runs literal cache-path migration and prints an exact
  inventory. `--apply` performs it. Immutable dated evidence, Tower data,
  worktrees, outputs and the coupled compiler bootstrap files are excluded.
- `purge-bulk.mjs` dry-runs deletion candidates; `--apply` deletes eligible output.
  It keeps small files, the newest three candidate generations, named live lanes,
  process-referenced paths and anything modified within six hours. Only large
  scratch files and old candidate/loop build trees are eligible. Add
  `--keep=AgentName` for an additional active scratch directory. It never enters
  stage1, port, ballots, cards, status, handoffs or BACKLOG files.

Read the dry-run output before applying either maintenance command. Do not
remove compatibility symlinks or old cache script copies until Main has rolled
over live agents and rebuilt the coupled bootstrap host.

Jet/Jetpack product state in `~/.jet` and `~/.local/share/jet` is not agent
scratch; any product-storage relocation requires a separate owner decision.
