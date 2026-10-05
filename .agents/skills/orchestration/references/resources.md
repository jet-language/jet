# Shared artifacts and bounded build output

Use this reference when parallel work shares build resources or proof artifacts.
Choose the smallest resource budget that fits the approved work.

## Share heavy artifacts

Build heavy artifacts once for each integrated source revision and share them
with every worker that needs them. Record the source revision with the artifact;
a stale binary is not proof. Avoid per-worker copies or private builds unless
the worker's unintegrated change is itself what must be measured. Serialize
writers to shared artifacts.

## Keep build output separate and bounded

Keep build output outside the repository on disk-backed storage. Give each
checkout exclusive ownership of its Cargo target; shared binaries do not imply
shared writable build targets. Configure external targets through `jet-env`.
Bound disk and memory use before starting work, account for peak allocations,
and reclaim idle output promptly after integration. Never remove source,
protected evidence, or artifacts still in use to make room for another job.

Use fresh artifacts for runtime claims. A source check, worker receipt, or stale
binary cannot establish runtime, tier, snapshot, golden, or I9 evidence.
