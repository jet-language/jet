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

The hosted seed preflight enables `JET_DEBUG_LOWER_ALL=1` to collect every
per-function MIR failure before another bootstrap rebuild. Require its final
`0/<actual total>` summary, then successful Rust emission and metadata checking
as separate gates. Earlier `__comptime` probe summaries are not the main-program
census. A completed per-function sweep does not establish final program assembly
or Rust type correctness. Native stages still require their empty all-phases TSV.
Also retain the optimizer's per-pass accepted/discarded census and full Debug
violations after the final function-sweep marker. Debug discovery rolls back an
invalid pass output and examines later passes, but any discarded main-program
pass still fails the preflight; later accepted passes cannot turn it green.
Earlier comptime-probe rejections stay visible separately. Budget for one
additional owning MIR snapshot in this debug-only collector; normal mode does
not allocate snapshots or publish a partially optimized program.

## Keep build output separate and bounded

Keep build output outside the repository on disk-backed storage. Give each
checkout exclusive ownership of its Cargo target; shared binaries do not imply
shared writable build targets. Configure external targets through `jet-env`.
Bound disk and memory use before starting work, account for peak allocations,
and reclaim idle output promptly after integration. Never remove source,
protected evidence, or artifacts still in use to make room for another job.
Forward a collected child's complete diagnostic buffer synchronously before
throwing on failure. An asynchronous pipe write followed by an uncaught exception
can truncate the last diagnostics; a partial log is not a collect-all census.

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
The D-SIGIL-ALGEBRA1=B cutover removes `core.math.pow` and `core.math.xor`,
including catalog/signature rows, dispatcher exports, evaluator aliases and
runtime C wrappers. Numeric `.pow()` and `.bit_xor()` remain the single routes;
do not restore retired wrappers to make a fresh ABI census pass.

Core metadata distinguishes source-owned nominals (`source_module … types { … }`)
from owner-only implementation admission (`impls { … }`); list both explicitly
when a public Core boundary owns its types and implements only some of them.
When an owned wrapper calls a private runtime kernel, remove its obsolete public
dispatcher and any unused Rust leaf. A provider still used by a separate bootstrap
builtin remains live; never replace the source boundary with a compatibility shim.
Source-owned URL parsing remains public in Core; its obsolete `parse`,
`urlparse`, and `urlsplit` dispatcher rows must not reintroduce the deleted
Rust `jet_url_parse` policy wrapper into native route classification.
Borrowed runtime fields that use canonical C carriers must project into
caller-owned stack storage, not lend a pointer to a different Rust layout.
IP24/socket56 projections copy only the fixed carrier bytes; they allocate
nothing and transfer no owner. Source-owned record fields require their exact
native provider; never reinterpret a host/port record as a socket56 carrier.
Reviewed narrow-parameter widening in `runtime-route-abi.mjs` is symbol/position
specific and requires that parameter's current `.bit_and` mask in the export
body. Comments, strings and unrelated functions are not mask evidence. Return
values, output slots and every unlisted symbol keep the strict physical ABI.
Closed native enum metadata must retain the current declaration's complete case
list: `Dyn` has Null, Bool, Int, Float, Number, Text, Bytes, DateTime, Array, and
Object. Restore a lost declaration row from history before regenerating; do not
extend a source-owned enum merely because an older native projection listed it.
Restore lost hidden dispatcher rows with their original authority: Clock's
`__clock_mono_ns` retains its pure route and the host epoch; substituting the
effectful `__host_monotonic_ns` route changes the existing public Clock contract.
The private crypto arithmetic owner `__core_crypto_ct` may use only
`core.mem.__list_reserve_exact` from the backing-leaf family. Exact preallocation
is a security property for its arenas and outputs: geometric growth can leave
freed secret-bearing copies. This admission does not grant capacity/type-size
inspection or expose any private leaf to user code; keep the Jet and bootstrap
checker boundaries in parity.
Bootstrap request mirrors bind every declared field, including defaulted fields,
through checked field symbols and explicit initializer rows. Keep the canonical
`selected_entry` spelling and the declared Dev/build/adapter/transport defaults;
source defaults alone do not initialize the host's generated Rust struct literal.

Release holds only after their immutable source export and managed output
directories exist. Queue completion is not a pass: consumers require the real
driver verdict and same-source artifact hashes. Retry a failed shared proof
under a fresh queue name while preserving its earlier receipts; a completed
same-source GREEN may be reused only after verifying its artifacts.
Validate every generated proofq header as well as `bash -n` before release:
the scheduler accepts only `kind=build`, `kind=test`, or `kind=measure`.
Pack/native compiler producers are builds; smoke and S5 validation phases are
tests. Shell syntax alone does not prove scheduler admission.
Before every build release, run all 18 active generator `--check` gates against
the immutable export, not the shared worktree, and require 18/18 before moving
the hold into the runnable queue. A worktree check can consume uncommitted
hash refreshes that the export omits; commit the canonical generated outputs
and re-export rather than bypassing the build-script stale-table guard.
Core catalog changes also require the canonical structure registry and enum/
record payload projections. Regenerate them with `gen-core-tables --write`
(the structure registry is edited directly); refreshing the surface ledger alone
does not keep the assembled compiler's catalog or source hashes current.
Before releasing a producer, pin every live compiler target and read-only
snapshot in an active `proofq/*plan*` document: the disk guard reads these plans
as live references. Historic receipts and `disk-protect.txt` are not live pins.
Keep the current verifier pinned across idle gaps; include the coming release
target before its build starts, and remove old pins only after their consumers
have switched or stopped. Debug and RC producers use separate atomic plans so
one concurrent producer cannot overwrite the other's input lifetime.
Retired arithmetic entry points have no internal compatibility exemption:
Core implementations and tests use numeric `.pow()` / `.bit_xor()` receivers,
with explicit `Float` receivers for floating literal bases. Sweep both Core
and Runtime sources before the final embedded-Core bake; preserve `Dyn` in
user-facing codec diagnostics instead of teaching retired dynamic-tree names.
The native fixed point is stage1 (built by the candidate) compiling stage2.
Require both objects and ELF executables to be byte-identical, and require each
stage's own all-phases diagnostic report to exist and contain zero failures.
Before a fresh pack exists, lower-fixture `--assemble-only` permits a full
frontend check and `--route-carriers` permits the carrier execution probe.
Neither qualifies native fixture execution: full bits/ContainerCodecs images
still require the same-source admitted Jet pack and unfiltered native registry.
Run the final full-suite/golden gate with `JET=stage2/jet`, not only the earlier
candidate S5 signal. Require the same-source complete suite receipt, registries,
and hello/sample native runs at O0 and O2. Rust/LLVM execution is default-denied;
only committed, source-proved user Rust FFI cases may admit their foreign crate
builds with the exact declared dependency manifest and foreign-build ancestry.
The native suite includes the product interop cases and user-FFI examples.
Freeze the source shell's opt-in `JET_RUST_MUSL_TOOLCHAIN`,
`JET_RUST_MUSL_LINKER`, and `JET_RUST_MUSL_AR` values in the smoke job and
receipt after checking that the toolchain, musl standard library, linker, and
archiver exist. Only the declared helper child selects that toolchain; never
replace global `PATH` or `RUSTC` or treat these cases as zero-Rust executions.
The exact `rustc -vV` identity query is admitted only under direct Jet ancestry
in that same allowlisted case/dependency cache namespace; report it separately
as `allowedFfiQueries`, not a crate build. Never admit a Rust fallback for
compiling the Jet program. Preserve PASS cases without an observed command
marker as an explicit category without inferring why the marker is absent.
The cutover suite composes the same native runner commands as `verify-full`
after its formal closeout prefix, without adding a skip-closeout option.
Report qualified formal proof composition and milestone closeout separately
as `NOT MET — deferred per owner #59`; stage equivalence and passing suites
cannot fabricate Lean discharge/standing-safety-net qualification.
The Rust-hosted `native-compiler --seed-build` does not execute Jet Host/Entry
and cannot produce that TSV. Its proof instead requires the real host build
exit zero, a seed log with no error/ICE/panic rows, and verified seed JSON and
executable ELF hashes. Preserve this hosted/native boundary in the receipt;
never fabricate an empty keep-going report to pass a hosted seed.
Before the RC seed build, assemble its exact CLI unit and emit the complete
inline Rust monolith with the final debug snapshot. Capture that snapshot's
small release build rustc argv, then type-check the monolith using
`--emit=metadata` and the captured release edition/cfg/type-affecting flags,
without backend codegen/LTO/link. Record the Dev Jet frontend versus Release
rustc boundary; this preflight is not a successful release seed build.
For an emit-only preflight, set `JET_TRACE_FILE` and retain the actual
`sema.check`, `lower`, `mir.digest`, and `emit` trace spans, together with the
per-step UTC/exit records. The emit CLI has its own entry and must not rely on
the build entry's trace wrappers. Entry into the following `mir.digest` span
proves MIR lowering returned and bounds successful sealing; an end event alone
can also occur during unwinding. Neither is a distinct internal seal timestamp.
Keep externally observed canonical-order acceptance separate from exact trace
timestamps. On a retry that already has complete MIR dumps, do not enable `JET_DUMP_MIR`
again: the debug text is many gigabytes and is not a mandatory clean-run gate.
Require the hosted emitter's own final `0/<actual item count>` census in
collect-all mode, preserving every failed item's kind, lossless identity,
source span, and full diagnostic. A missing emitter census, any failed item,
or later accepted output after an earlier emission failure is not a pass.
When failed hosted emission retains a diagnostic-only `JET_DUMP_RUST` file,
type-check its surviving real items in the same discovery cycle. Keep the
partial dump, its failure/item counts, and metadata diagnostics separate from
the full monolith proof: even a clean partial metadata check never admits the
failed emission, a seed executable, or an RC. Failed-item comments are not code.
The missing historical display comparator and AGE testkit generator are retired
Rust-oracle modes (owner #38(b)/#59), not native-suite failures. Report their
individual rows as `UNCOVERED`, print them, and require a Tower card; never label
them PASS. The native golden union and `run-crypto-fixtures` AGE decoder/sealer
tests remain mandatory and their failures still block the final native gate.
S5 native runners select the admitted `JET` from the same-source runtime-pack
publication; its executable hash must match `candidate_jetc0`. Do not pass the
retired `JET_FRONT_JETC0` factory override into native suite runners.
Effect-budget witness origins borrow the checked graph until enforcement
materializes denied-module diagnostics. Project effects after the compile
transfers that graph, and retain fallback checked bundles through enforcement;
do not clone a whole program merely to extend the witness lifetime.

Use fresh artifacts for runtime claims. A source check, worker receipt, or stale
binary cannot establish runtime, tier, snapshot, golden, or I9 evidence.
