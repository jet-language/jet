# Shared artifacts and bounded build output

Use this reference when parallel work shares build resources or proof artifacts.
Choose the smallest resource budget that fits the approved work.

## Share heavy artifacts

Build heavy artifacts once for each integrated source revision and share them
with every worker that needs them. Record the source revision with the artifact;
a stale binary is not proof. Avoid per-worker copies or private builds unless
the worker's unintegrated change is itself what must be measured. Serialize
writers to shared artifacts.

During consolidation repair, remove exact duplicate declarations while retaining
the canonical regression scenarios; compilation fixes must not narrow coverage.

Admit the shared host/release build as soon as the workspace/all-target bootstrap
check is green; do not delay the fresh verifier behind an older verifier's grammar
or memory failures. Pin it to an immutable export of that checked commit. Publish
the bootstrap host and release `jet` entries in `jet-regression-artifacts/v1`
as they become available, and derive runtime freshness checks from that release's
own all-parts emission.

The separate candidate build requires read-only generator checks and a frontend
check of the complete compiler assembled by `Compiler/Bootstrap/assemble.mjs`,
using the fresh release verifier. A file-local parse receipt does not establish
that the assembled compiler resolves its current APIs; inspect nested diagnostics,
not just the transport's top-level `ok` flag. Migrate callers to retained current
APIs without restoring retired providers or aliases. Append the candidate entry
only after its real CLI hello succeeds and its collect-all report contains no
failure rows; typed traps are failure evidence, not a green compiler.

## Keep build output separate and bounded

Keep build output outside the repository on disk-backed storage. Give each
checkout exclusive ownership of its Cargo target; shared binaries do not imply
shared writable build targets. Configure external targets through `jet-env`.
Bound disk and memory use before starting work, account for peak allocations,
and reclaim idle output promptly after integration. Never remove source,
protected evidence, or artifacts still in use to make room for another job.

`jet-env` preserves caller-selected Cargo wrappers; it does not select sccache
based on whether a debug output directory exists. Set `JET_NO_SCCACHE=1` to
clear both Rust wrapper variables after shell imports, including for release
builds and immutable source exports. Generator freshness checks use `--check`
without rewriting their outputs; diagnostic row generation follows this same
contract.

Use fresh artifacts for runtime claims. A source check, worker receipt, or stale
binary cannot establish runtime, tier, snapshot, golden, or I9 evidence.
