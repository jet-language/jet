# Bounded resources and liveness

Load this reference for a worker wave, long proof, recovery, or any command that
can consume shared machine resources. Controls are bounded defaults; choose the
smallest limit that fits the artifact and owner-approved scope.

## Time and liveness

Use 300 seconds for mechanical fixture work, 720 seconds for normal work, and
1,200 seconds only for one narrow semantic root cause. At the limit, cancel the
lane, salvage only a coherent owned patch, and rebrief a smaller slice. Use
`hub jobs` or `hub wait` with pid-aware status; a log is not a process and a
missing completion marker is not failure by itself.

Use the host's task tool as the first dispatch path. If it cannot run the
required role, record the exact harness failure in the handoff before a bounded
fallback. Prompts do not select a model.

## Disk and memory

Use the one shared bounded checkout `target/`, with `CARGO_INCREMENTAL=0`.
Keep scratch at `~/.cache/jet-test-scratch` and logs/briefs at
`~/.cache/jet-luna`; `/tmp` is RAM-backed and must not hold Cargo targets,
large logs, or test scratch. Monitor available RAM, swap, disk, and target size.
Respect `JET_TARGET_CAP_GB` (120 GiB by default).

Run repository commands through `Tools/agent/jet-env`. For source checks,
use the checkout-aware commands in the brief, for example:

```sh
Tools/agent/jet-env jet check path/to/file.jet
Tools/agent/jet-env jet fmt --check path/to/file.jet
```

Before any runtime claim, build a fresh binary through the wrapper and exercise
the actual path. A worker type-check, stale `target/debug/jet`, or static log
cannot establish runtime, tier, snapshot, golden, or I9 evidence.
