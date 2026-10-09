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

Admit the shared host/release build at Main's committed bootstrap-build pin;
retired Rust integration tests are not the compiler-source gate. Do not delay the
fresh verifier behind an older verifier's grammar or memory failures. Pin the build
to an immutable export of that commit. Publish
the bootstrap host and release `jet` entries in `jet-regression-artifacts/v1`
as they become available, and derive runtime freshness checks from that release's
own all-parts emission.
Keep the all-parts seed's public Core imports current, including `core.db.sql`;
otherwise a newly exposed source-owned boundary is absent from the error census.

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

Jet-emitting generators must use the current `.bit_and`, `.bit_or`, `.bit_xor`
and `.copy()` forms in their emitted bodies. Keep JavaScript bit operations,
foreign bodies, and deliberate retired-syntax diagnostic inputs unchanged.
Fix stale output at its template, regenerate canonically, and run the read-only
checks; editing a generated Jet file alone is not a generator repair.

Core metadata distinguishes source-owned nominals (`source_module … types { … }`)
from owner-only implementation admission (`impls { … }`); list both explicitly
when a public Core boundary owns its types and implements only some of them.
When an owned wrapper calls a private runtime kernel, remove its obsolete public
dispatcher and any unused Rust leaf. A provider still used by a separate bootstrap
builtin remains live; never replace the source boundary with a compatibility shim.
Closed native enum metadata must retain the current declaration's complete case
list: `Dyn` has Null, Bool, Int, Float, Number, Text, Bytes, DateTime, Array, and
Object. Restore a lost declaration row from history before regenerating; do not
extend a source-owned enum merely because an older native projection listed it.
Restore lost hidden dispatcher rows with their original authority: Clock's
`__clock_mono_ns` retains its pure route and the host epoch; substituting the
effectful `__host_monotonic_ns` route changes the existing public Clock contract.

Use fresh artifacts for runtime claims. A source check, worker receipt, or stale
binary cannot establish runtime, tier, snapshot, golden, or I9 evidence.
