# In-place port queue tools

Reusable queue code lives here. Mutable queue data remains in
`$HOME/.cache/jet-dev/port/wave` (`INDEX.tsv`, `PLACED.tsv`, `CLAIMS.tsv`,
reservations and packet briefs). `JET_REPO` overrides the checkout derived
from this directory; packet targets resolve under its `.agent-worktrees`.

```sh
node Tools/agent/port/claim.mjs status
node Tools/agent/port/claim.mjs claim WorkerName port-compiler
node Tools/agent/port/claim.mjs done WorkerName packet-id "translated source and tests"
node Tools/agent/port/claim.mjs release WorkerName packet-id
node Tools/agent/port/unplanned.mjs
node Tools/agent/port/place-rust.mjs --dry
```

`place-rust.mjs` without `--dry` writes pending Rust blocks into packet targets.
`defer.mjs` mutates claims for the explicitly excluded CoreLib/Prelude/JIT ranges;
run it only when that exclusion is still authorized. Claim/done/release use the
queue lock; these commands do not compile or write Tower.
[INPLACE-BRIEF.md](INPLACE-BRIEF.md) is the owner's source-translation procedure.
Cache copies remain until running writers switch to these repository paths.
