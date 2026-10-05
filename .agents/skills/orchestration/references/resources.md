# Bounded resources and liveness

Load this for worker waves, long proofs, recovery, or commands consuming shared
machine resources. Use the smallest limit fitting the owner-approved scope.
The [dispatch reference](dispatch.md) owns the Opus/Sol concurrency budget.

## Time and liveness

Use 300 seconds for mechanical fixture work, 720 seconds for normal work, and
1,200 seconds for one narrow semantic root cause. At the limit, cancel the
lane, salvage only a coherent owned patch, and rebrief a smaller slice. Use
pid-aware host job status; a log is not a process and a missing completion
marker is not failure by itself. Use the host task tool first; record the exact
harness failure before a bounded fallback. Prompts do not select a model.

## Host paths and build placement

Set `JET_SCRATCH_ROOT` to an absolute path on a mounted, disk-backed scratch
filesystem. There is **no default**. The checked-in `Tools/agent/host-env.sh`
sources `${JET_HOST_ENV_FILE:-${XDG_CONFIG_HOME:-$HOME/.config}/jet/env}` if it
exists. Put host-specific defaults there, using shell assignments that preserve
an existing environment value. Alternatively configure the variable in the
host environment (for example NixOS `environment.variables.JET_SCRATCH_ROOT`).
The host file covers the interval before an environment/NixOS switch takes
effect; neither machine configuration nor its paths belong in this repo.

`JET_DEV_ROOT` defaults to `${XDG_CACHE_HOME:-$HOME/.cache}/jet-dev` for small
receipts, logs, locks, and queue state. `JET_PROOFQ_ROOT` defaults to
`$JET_DEV_ROOT/proofq`; `JET_REPO` defaults to the tooling's own checkout.
Use `$JET_SCRATCH_ROOT/scratch` for bulk scratch, `targets/<Agent>` for each
agent's Cargo target, and `candidates` for shared candidate binaries. Keep
only evidence (md/tsv/json receipts and small logs) in the cache scratch area.
Do not use RAM-backed `/tmp` or create Cargo targets in worktrees/repo root.

Run every repository command through `Tools/agent/jet-env`, explicitly setting
`CARGO_TARGET_DIR="$JET_SCRATCH_ROOT/targets/<Agent>"`. Its external-target
policy stamps `.jet-checkout` and rejects reuse by another checkout; one target
has one owner. Delete the target after the work lands, when idle and unprotected.
Respect `JET_TARGET_CAP_GB` (120 GiB). Keep `CARGO_INCREMENTAL=0`, except the
orchestrator's lock-serialized, one-writer targets may use
`JET_CARGO_INCREMENTAL=1` and are pruned between builds.

## Shared heavy artifacts

The integrator builds one candidate jetc0 and one release `jet` per `dev` head,
then shares those exact artifacts with all measuring agents. Record the source
commit with the artifact so stale binaries never become proof. Agents do not
build private candidates unless the changed candidate is itself being measured
and the change has not yet landed on `dev`. Candidate-overlay builds hold
`$JET_DEV_ROOT/jetc0-run.lock`. Rebuild before runtime smoke tests; a source
check or stale binary cannot establish tier, snapshot, golden, or I9 evidence.

## Proof queue

The checked-in runner is `Tools/agent/proof-queue.sh`, invoked through
`jet-env`. Main starts lane 1 (`--lane 1`) and, when useful, lane 2 (`--lane 2`)
as named services outside build lanes. Do not run a third lane. Each runner
claims work under a shared lock, then records `done/<name>.log`, `.rc`, and
`.sh`. It never starts a job while `PAUSE` exists or the main filesystem is
88% full. Jobs time out after 3,600 seconds; non-zero receipts remain failures.

Write one executable bash job to `queue/<Agent>-<slug>.sh`. Use `set -e`, change
to your own worktree first (never the live checkout), and run every tool via
`Tools/agent/jet-env`: the service has no development shell. Cargo goes through
`LANES_MEM=8G bash Tools/agent/cache/laneS.sh Tools/agent/jet-env cargo …`.
Isochecks hold `flock -o "$JET_DEV_ROOT/iso-priority.lock"` and call the
checked-in `Tools/agent/cache/isocheck-priority.sh` with `ISOCHECK_BASE=<tree>`.
An isocheck summary reporting non-zero `rc` or errors is a failure even when
its wrapper exits zero: job authors must propagate that result. Pause a queued
job by renaming `.sh` to `.sh.hold`; restore `.sh` to requeue it.

Oldest mtime runs first. Landing/correctness work (`Lander*`, `Fix*`, `Impl-*`,
`Idiom-*` unit checks) may be promoted by backdating its mtime. After landing
jobs, owner-directed normal-path (`GoldenFix*`) and native-backend (`BackendO*`)
goldens get priority. Main may reorder. Performance (`Perf*`, `SP0*`) jobs stay
FIFO, on lane 1; lane 1 waits for running jobs before starting them. Lane 2 may
run correctness beside a measurement for throughput, but performance-gate
receipts must be taken separately on a quiet machine.

Lane 1 requires MemAvailable ≥30 GiB (45 during a fixed point). Lane 2 requires
≥40 GiB (55 during a fixed point), an active lane-1 job, and every running job
aged at least ten minutes so memory has ramped. Jobs cannot assume isolation:
keep Cargo in laneS and isochecks under their lock. Direct whole-compiler-unit
`jet check`/`jet test` runs (5–8 GB) outside the queue hold
`$JET_DEV_ROOT/unitcheck.lock`, one machine-wide at a time.

## Shared sccache

Main starts one server **outside every lane**, idle timeout 0, listening on
127.0.0.1:4226 with cache `$JET_SCRATCH_ROOT/sccache` capped at 30 GiB. A server
started inside laneS charges other compiles to that 8 GB cgroup and OOM-kills
them. If `sccache --show-stats` fails, tell Main; do not start another server.
Use `JET_NO_SCCACHE=1` when building without it.

## Disk gates and protection

Before a job expected to write **more than 20 GB**, check estimated peak
additional allocation on **each destination filesystem**:

```sh
Tools/agent/jet-env bash Tools/agent/disk-guard.sh \
  --check 30 "$JET_SCRATCH_ROOT/targets/MyAgent"
```

Replace 30 with peak additional GiB. Exit 75 means hold the job until Main
frees space. Reserve headroom below 85% on the main filesystem and 75% on
scratch, with at most 350 GB (decimal) allocated on scratch. Never bypass a
failed gate or a queue `PAUSE`. The guard pauses at 85% on the main filesystem,
recovers toward below 80%, and releases **only its own pause**.

Main starts `Tools/agent/jet-env bash Tools/agent/disk-guard.sh` outside build
lanes with Node and lsof available (use the host's package environment when
needed). `--once` performs one cycle. `JET_DISK_ROOT` defaults to `/`,
`JET_DISK_LSOF` to `lsof`; other path variables are defined above. The guard
logs both filesystems every ten minutes to `disk-usage.log`, and alerts through
`DISK-ALERT` in `JET_DEV_ROOT`.

Before jobs start, list active inputs and evidence in `disk-protect.txt` (one
absolute path or glob per line; blank/comment lines ignored). Missing protection
fails closed. `disk-kept-binaries.tsv` also protects retained binaries.
`DISK-PLAN.tsv` approves idle bulk using columns
`path, size_bytes, class, safe, reason, last_modified` (tab-separated; header
required, `safe=yes` only for explicitly approved entries). The guard records
moves in `disk-moves.tsv`. Only approved idle candidates, fixed-point bulk, and
Cargo targets may move or be evicted; it never removes protected evidence,
source worktrees, or shared sccache. Recently modified or process/queued-job
referenced paths remain protected. Main extends the protection-aware plan
when no safe recovery remains; a full disk is not permission to delete more.
