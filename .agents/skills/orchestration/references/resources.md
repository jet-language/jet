# Shared artifacts and bounded build output

Use this reference when parallel work shares build resources or proof artifacts.
Choose the smallest resource budget that fits the approved work.

## Share heavy artifacts

Build heavy artifacts once for each integrated source revision and share them
with every worker that needs them. Record the source revision with the artifact;
a stale binary is not proof. Avoid per-worker copies or private builds unless
the worker's unintegrated change is itself what must be measured. Serialize
writers to shared artifacts.

Before admitting a shared compiler build, collect the workspace/all-target
bootstrap check, read-only generator checks, and a frontend check of the complete
compiler assembled by `Compiler/Bootstrap/assemble.mjs` with the frozen bootstrap
verifier. A file-local parse receipt does not establish that the assembled
compiler resolves its current APIs. Migrate callers to the retained one-walk
Core collector, in-place effect graph publisher, payload-place shared writer,
multi-root backend lowering, source-owned expiry carriers, and current owner-gated
recipe refusal; do not restore retired providers or compatibility aliases.
Pin the admitted build to an immutable export of the checked commit and derive
runtime freshness checks from that build's own all-parts emission.

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
