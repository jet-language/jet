# Agent runtime and cache maintenance

All mutable development output belongs under `$JET_STATE_ROOT`.
`$HOME/.cache/jet` is reserved for Jet's own cache. Code lives in this repository;
cache script copies remain temporarily for running agents, not as new tooling homes.
`JET_REPO` overrides the checkout derived from these tools.

```sh
jet Tools/agent/cache/Lanes.jet small node your-small-job.mjs
jet Tools/agent/cache/Lanes.jet heavy 8 command args
jet Tools/agent/cache/Lanes.jet jet check path/to/program.jet
node Tools/agent/cache/purge-bulk.mjs
```

- `Lanes.jet small` gives four capped small-job slots (`LANES_MEM`, 1G..8G,
  default 3G); `Lanes.jet heavy GB` gives two heavy slots capped at GB (at most
  12). `Lanes.jet jet` runs `$JET` (default the frozen `scratch/jet-current`
  binary) in three serialized slots; `SAFE_JET_MEM` (default 6G) and
  `SAFE_JET_TIMEOUT` (default 300 s, exit 124) override it. The cap is the
  child's address-space limit (`core.process` `memory_limit`); slots are
  exclusive file locks under `$JET_STATE_ROOT`.
- `Lanes.jet guard` is an opt-in long-running emergency guard: below 7 GiB available
  RAM it kills the largest eligible compiler/build process and logs the action.
  Only Main should start it; it is not a substitute for job memory caps.
- `purge-bulk.mjs` dry-runs deletion candidates; `--apply` deletes eligible output.
  It keeps small files, the newest three candidate generations, named live lanes,
  process-referenced paths and anything modified within six hours. Only large
  scratch files and old candidate/loop build trees are eligible. Add
  `--keep=AgentName` for an additional active scratch directory. It never enters
  stage1, port, ballots, cards, status, handoffs or BACKLOG files.

Read the dry-run output before applying the maintenance command. Do not
remove compatibility symlinks or old cache script copies until Main has rolled
over live agents and rebuilt the coupled bootstrap host.

Jet/Jetpack product state in `~/.jet` and `~/.local/share/jet` is not agent
scratch; any product-storage relocation requires a separate owner decision.
