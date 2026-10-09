# Safety

This specification defines Jet's default safety boundary and its audited
expert escape. It is for Jet users, library authors, and reviewers. The
executable spelling lives in [`Syntax.rs`](../../crates/jet-foundation/src/Syntax.rs);
sema checks the gate and its obligations in
[`UnsafeObligations.rs`](../../crates/jet-sema/src/Sema/UnsafeObligations.rs).
The focused [`unsafe_ratchet` tests](../../tests/unsafe_ratchet.rs), the
[`golden` safety checks](../../tests/golden.rs), and the
[`ratchet checker`](../../Tools/agent/check-unsafe-ratchet.mjs) provide the
repository proof; gated examples live under
[`Examples/features/`](../../Examples/features/). Terms such as stream and
reader follow the [Jet vocabulary](vocabulary.md).

## The audited escape

Safe Jet code cannot perform a raw memory operation, call an unsafe function,
or cross an unsafe foreign boundary without an enclosing audited gate. Write
the gate with Pascal-case `Unsafe` and a string reason:

```jet
cell :: 1
#Unsafe("`cell` is live on this stack frame and the pointer never escapes") {
    ptr :: *cell
    value :: ptr.*
}
```

The same contract applies to a whole function:

```jet
#Unsafe("the foreign ABI preserves the scalar contract") fn add(a: Int, b: Int) -> Int {
    a + b
}
```

The reason belongs to `#Unsafe` itself. Bare `#Unsafe { … }` and
`#Unsafe fn …` are rejected (E3112). Lowercase `unsafe` is recognized only to
teach the foreign-spelling diagnostic; it is not Jet syntax. An unsafe function
body is an audited region, but calling an unsafe function still requires an
enclosing `#Unsafe` region.

`use core.mem` is the import gate for the low-level memory names. It does not
replace the audit gate: raw-pointer formation, dereference, address-based
pointer construction, and volatile access also require `#Unsafe`. The sema
obligation pass can require `assert valid_ptr, aligned, no_alias` immediately
after a tracked operation. Those assertions are meaningful only inside the
gate and are removed before TIR.

Inline `#FFI(c)`, `#FFI(cpp)`, and `#FFI(asm)` bodies require an enclosing
`#Unsafe("reason")` gate. The same boundary covers foreign calls whose
signature transfers ownership or exposes raw memory. A package or
organization policy may forbid unsafe code; a lexical gate cannot raise that
safety floor. Under a per-site policy, the gate chooses
`obligations: .Track` or `.Skip` according to the governing policy.

Generated Rust may contain `unsafe` only for an approved audited Jet gate or
inside vetted runtime, standard-library, and memory-internal implementations.
The generated code is not a new user escape hatch; the gate and sema proof are
the source-level authority.

## Standing native safety gate

`Tools/agent/safety-gate.mjs OUT ARTIFACTS_JSON [SEED COUNT]` runs the
Core unsafe-region/safe-abstraction audit and the two unsafe-wrapper diagnostic
fixtures, generates independent-model programs plus stateful Core API programs,
and checks admitted native **O0, O1 and O2** outputs. The Core programs exercise
map replacement/removal/lookup, list growth/pop/copy isolation and strict UTF-8
decoding, including invalid sequences. The golden slice includes successful
memory, collections, strings, low-level, safety and concurrency examples.
Native stdout, stderr and status must agree with the model or golden. This is
independent of generated Rust; agreement at every tier also implies tier-to-tier
agreement on those observables. Independent MIR Lint receipts are required for
each case. Generated-Rust differential runs are supplemental, not Rust-free proof.

The native artifact producer adds `native_safety.tiers.O0`, `.O1` and `.O2`
to the admitted artifacts JSON. Each tier has `fuzz` and `golden` objects with
`directory` and `runtime`. Each directory uses the existing backend harness
layout (`results.tsv`, `run/*.stdout`, `*.stderr`, `*.status.json`,
`lower/*.asan.o`, `lint.tsv`) and a `native-safety-input.json` binding
`head`, `backend: "jet-native"`, `tier`, `corpus_sha256`, `seed`, `count`,
`frontend_language` and `compiler_language`. `--prepare` writes the generated
corpus and its hash without running checks; the full run records the golden
case list/hash too. Producers must compile the newly generated corpus for each
seed, not relabel an old run. A release build of the harness driver is **not**
evidence of native O2.

`runtime` binds `library`, `sha256`, `head`, `language`, and
`instrumentation: "address"`. Its `negative_control` binds `receipt` and
`sha256`; that JSON receipt names the same `head`, `runtime_sha256` and
`detected` error kinds (`heap-buffer-overflow`, `heap-use-after-free`, `leak`).
Only an exact-head, hash-matched, negatively tested archive is admitted.
`run-sanitized.mjs --runtime-library ARCHIVE` links prebuilt native code without
invoking rustc. Its older `--runtime runtime.rs` mode still instruments the
current Rust runtime, but cannot satisfy Rust-free runtime coverage.
Every requested native object must be instrumented. Missing objects, missing
references, link errors, empty corpora, timeouts/crashes and missing
instrumentation fail rather than silently passing. ASan/LSan reports and any
sanitized/plain stdout, stderr or status difference fail too.

Additional instrumentation/self-build producers publish
`native_safety.additional_coverage[NAME]` with a receipt path and SHA-256.
That receipt must bind the admitted `head`, exact `coverage` name, `verdict:
"PASS"`, a positive `checked_cases` count and an empty `failures` array.
Absent, stale, empty or failing receipts leave the named coverage gap open.
The #4754 regression additionally requires exact coverage named
`inline moved-payload/Option-unwrap and backedge aliases`: the producer's
whole-class instrumented matrix, including the backedge-alias control and the
native `memory/last_use_moves` golden. The ordinary tier golden slice also
includes `memory/last_use_moves` and `memory/last_use_field_moves`. A golden
pass alone is not a substitute for the whole-class receipt.

The single local proofq definition is `Tools/agent/safety-gate-job.sh`.
`Tools/agent/safety-schedule.mjs --watch` polls admitted artifacts every minute
and schedules it on each changed admission, and every **six hours** with a
rotating seed. Only one safety job may be queued/running. It snapshots the
artifacts JSON, pins the tooling tree, runs exact-tree pre-verification through
`cheap.sh`, and passes `pq-preflight` before enqueueing. Do not start the
watcher on a mutable private branch; the integrator starts it from the landed
dev tooling. It uses the existing proof queue, respects `PAUSE`, and does not
create hosted CI or a nightly job. Paths can be set with `SAFETY_ARTIFACTS`,
`SAFETY_STATE`, `SAFETY_SCRATCH`, `PQ_ROOT`; `SAFETY_COUNT` controls each of the
two generated case families (default 50).

The gate records `PASS` (exit 0), findings `FAIL` (1), infrastructure error
(2), or unavailable coverage `GAP` (3). Pre-verification findings block
enqueueing; an owner-authorized gap receipt remains non-green in the queued
run. It is never converted to a success. Receipts bind artifact hash, source
head, seed, corpus hash, all findings and every coverage gap.

The checker is the registry's current-head debug compiler while a stale
release entry remains; a native producer can instead bind
`native_safety.checker` with `path` and `head`. Core source is embedded in the
compiler, so an older release cannot prove a newly audited wrapper.

Current limits: native lowering has non-frame Load/Store ASan checks, not
stack redzones/use-after-return, uninitialized-memory/UB or data-race checks.
The gate explicitly lists these, sanitized native compiler self-build and
mutation-control proofs for MIR `alias.exclusive`, `view.valid` and
`send.boundary` as missing. The current runtime/frontend still require Rust,
no native O1/O2
producer has been admitted, and no current runtime pack is recorded in the
shared artifact registry. The standing gate can exercise the audit/model legs
now and consume actual native outputs when admitted; it cannot manufacture
Rust-free proof. The Rust-removal gate remains open until native tier producers,
the instrumented Jet runtime and the missing instrumentation/self-build proofs
exist.

## The post-Rust safety argument

The post-Rust safety argument requires two independent checks before code is
generated. Sema owns every user-facing check (I3) and reports each violation as
a registered diagnostic. MIR Lint ([mir-lint.md](mir-lint.md)) independently
checks the canonical MIR; a failure is an internal compiler error. On the Rust
path rustc's borrow checker and `Send` checks provide another check (I2).
The native backend has no rustc and must admit the independent MIR controls
alongside its runtime instrumentation receipts:

- `ownership.move` backs use-after-move (E0121);
- `alias.exclusive` backs in-call aliasing (E0204), closures that keep a write
  live, and overlapping task-group loans (E1101);
- `view.valid` backs views whose owner changed (E0212), owners changed while
  lent to a task (E1101), and views that escape their owners (E2305);
- `send.boundary` backs mutable or borrowed captures that cross a task or
  parallel boundary (E1101) and Cell, view, or function values that cross one
  (E1102).

The MIR Lint fixtures pair clean and planted, hand-built MIR shapes and bypass
sema entirely. They are independent mutation controls, not receipts obtained by
deleting semantic checks in a compiler build. Source-bound control receipts
must still be admitted to the standing gate; their presence alone does not
close the native runtime or sanitizer gaps above. Writing through a frozen value
(E1113) is not backed yet, because the MIR has no frozen fact; the native backend
still relies on sema alone for that rule.

## Unsafe-region ratchet

The repository records every user-written semantic unsafe region in the
baseline below. Each row records its package, source file, marker position, and
reason. The JSON data and Markdown tables are one generated registry; do not
edit either representation by hand.

[`check-unsafe-ratchet.mjs`](../../Tools/agent/check-unsafe-ratchet.mjs)
recursively scans `.jet` files while excluding its configured generated and
build directories. It recognizes block and whole-function
`#Unsafe("reason")` gates and grouped
`#[Unsafe("reason"), …]` function gates. It skips comments, strings, malformed
or incorrectly placed markers, and generated FFI files below `.jet/bindings/`.
A lone declaration gate is bare `#Unsafe("reason")`; the bracket form is for
two or more markers, such as `#[Unsafe("reason"), FFI(asm)]`.

Run the checker without `--update` to compare source with this baseline. A
larger count fails and reports each unmatched semantic region. When a new
region is approved, refresh the baseline in the same change with:

```sh
node Tools/agent/check-unsafe-ratchet.mjs --update
```

If source removes only recorded regions, the checker lowers and rewrites the
baseline automatically. A new semantic region cannot hide behind a net
decrease. A count-preserving edit that changes a recorded file, position, or
reason is stale and fails until the baseline is refreshed. The
`unsafe-ratchet` block is intentionally kept byte-stable between such
refreshes.

The baseline is also consumed by `tests/golden.rs` to identify the audited
example stems whose generated Rust may contain `unsafe`. The positive check
matches every source region against the baseline's count, position, and reason,
then requires that region's lowered audit sentry with its source line and reason.
An unrelated prelude `unsafe` or another region's sentry cannot satisfy it.
Every region kind lowers to its own sentry (D-MEM-SENTRY1): a block, a gated
`#Unsafe fn` body, and a gated `#FFI`/`#Import(c)` declaration, whose sentry
wraps each call. Regions inside `#Test` are checked against the test-harness
build, the only build that contains them.
A source-level gate does not itself require a Rust unsafe operation: expert
crypto calls may use safe, source-owned Core Jet bodies while sema still requires
`#Unsafe` for nonce, algorithm, and secret-exposure obligations (E0510).
All actual Rust unsafe operations retain the gated-form scan, and every ungated
example retains the negative unsafe-outside-vetted scan.

<!-- unsafe-ratchet:begin -->
<!-- unsafe-ratchet:data
{
  "schema": 1,
  "total": 983,
  "counts": {
    "Examples": 38,
    "jet": 589,
    "jetpack": 1,
    "tests": 355
  },
  "regions": [
    {
      "package": "Examples",
      "file": "Examples/features/crypto/crypto_migration.jet",
      "line": 31,
      "column": 5,
      "reason": "AES-256-GCM protocol interoperability"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/crypto_suite.jet",
      "line": 36,
      "column": 5,
      "reason": "known-answer check reads the RFC 7748 shared secret"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/expert_projection.jet",
      "line": 30,
      "column": 5,
      "reason": "expert cryptography is an explicit opt-in"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/primitive_matrix.jet",
      "line": 96,
      "column": 5,
      "reason": "known-answer check reads the RFC 7748 shared secret"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/primitive_matrix.jet",
      "line": 158,
      "column": 5,
      "reason": "expert known-answer vectors need raw key bytes"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/random_api_split.jet",
      "line": 33,
      "column": 5,
      "reason": "compare the typed and raw HKDF rungs"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/vault_keys.jet",
      "line": 41,
      "column": 5,
      "reason": "restore audited raw signing-key material"
    },
    {
      "package": "Examples",
      "file": "Examples/features/effects/audited_gate_ladder.jet",
      "line": 6,
      "column": 5,
      "reason": "the audited block has no low-level operation"
    },
    {
      "package": "Examples",
      "file": "Examples/features/effects/single_use_discard.jet",
      "line": 35,
      "column": 5,
      "reason": "event cancelled; the ticket admits to nothing, so voiding it is correct"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/cleanup_matrix.jet",
      "line": 33,
      "column": 5,
      "reason": "the stop runs the shared cleanup boundary"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/cleanup_matrix.jet",
      "line": 78,
      "column": 5,
      "reason": "the exit callbacks have no captured state"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/os_process_control.jet",
      "line": 30,
      "column": 5,
      "reason": "POSIX process and pipe control for the core.sys surface"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/os_stop_cleanup.jet",
      "line": 24,
      "column": 5,
      "reason": "the stop callback uses the shared cleanup boundary"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/process_exit_cleanup.jet",
      "line": 20,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/system_privileged_entry.jet",
      "line": 10,
      "column": 5,
      "reason": "run only inside a disposable user namespace with a dedicated test identity"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/system_projection.jet",
      "line": 14,
      "column": 5,
      "reason": "the clock file was created above in this test-owned directory"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/system_projection.jet",
      "line": 20,
      "column": 5,
      "reason": "fork and reap both children before returning"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_asm.jet",
      "line": 8,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_asm.jet",
      "line": 19,
      "column": 5,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_c.jet",
      "line": 6,
      "column": 1,
      "reason": "the scalar ABI contract matches the C definition"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_c.jet",
      "line": 13,
      "column": 1,
      "reason": "the scalar ABI contract deliberately returns a mismatched result"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_c.jet",
      "line": 19,
      "column": 5,
      "reason": "call the audited inline C contract"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_c.jet",
      "line": 26,
      "column": 9,
      "reason": "call the audited inline C contract with bounded Int values"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_c.jet",
      "line": 32,
      "column": 9,
      "reason": "call the audited inline C contract with a deliberately wrong C result"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/lowlevel.jet",
      "line": 8,
      "column": 5,
      "reason": "`cell` is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/mmio_board_write.jet",
      "line": 9,
      "column": 5,
      "reason": "stand-in MMIO cell stays on this stack frame"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/opaque_handles/run.jet",
      "line": 34,
      "column": 5,
      "reason": "fixed headers define handle close ownership, exclusive buffers, and bounded output lengths"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/pointer_cast_deref.jet",
      "line": 11,
      "column": 5,
      "reason": "flag is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/unsafe_obligations.jet",
      "line": 9,
      "column": 5,
      "reason": "cell stays live and the pointer remains local"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/expiring_secret.jet",
      "line": 20,
      "column": 5,
      "reason": "vault key import: expiring secret lifetime"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/pin.jet",
      "line": 43,
      "column": 5,
      "reason": "node storage is fixed for the returned pin; self_addr names this place"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/rawptr.jet",
      "line": 7,
      "column": 5,
      "reason": "`cell` is a live Int on this stack frame; the pointer never escapes"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/uninit.jet",
      "line": 12,
      "column": 5,
      "reason": "uninitialized plain-data storage is filled before every read"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/unsafe_sentries.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/unsafe_sentries_package_off/run.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this package-policy proof"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/unsafe_sentries_provenance.jet",
      "line": 6,
      "column": 5,
      "reason": "external address must be a live allocation"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/unsafe_sentries_source_off.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "Examples",
      "file": "Examples/features/tooling/checked_text_type.jet",
      "line": 20,
      "column": 5,
      "reason": "checked text example output"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Calls.jet",
      "line": 74,
      "column": 9,
      "reason": "each argument cell is inside this call's writable mapping"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Calls.jet",
      "line": 79,
      "column": 5,
      "reason": "loaded.entry is the selector-generated pointer-to-cells adapter; all cells remain mapped through its return"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 48,
      "column": 5,
      "reason": "function is the resolved (ptr,len)->word runtime constructor; buffer remains readable until it returns"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 117,
      "column": 5,
      "reason": "word is a live String handle and both functions borrow it through the canonical runtime C ABI"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 124,
      "column": 9,
      "reason": "the runtime's borrowed UTF-8 projection holds length readable bytes until the owning String is dropped"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 144,
      "column": 13,
      "reason": "word is a live exact-Int carrier and stringify returns a fresh owning String"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 148,
      "column": 13,
      "reason": "the temporary String is owned here and decoding has copied its bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 186,
      "column": 5,
      "reason": "word transfers one live String owner to the runtime's canonical consuming drop"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/CoreSharedParent.jet",
      "line": 264,
      "column": 5,
      "reason": "the admitted canonical control surrounds the producer's checked no-argument System V getter and its owned Shared result; the Core image remains process-pinned"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 165,
      "column": 9,
      "reason": "the slot is an aligned word inside the writable mapping, checked above"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 171,
      "column": 9,
      "reason": "the relocation is an aligned word inside the writable mapping, checked above"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 191,
      "column": 5,
      "reason": "the admitted control uses begin(fuel,depth,session), status(), end(); startup is the producer's checked no-argument Unit static-fixup boundary"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 212,
      "column": 5,
      "reason": "`main` is the runtime's `int32_t jet_rt_main(void (*)(void))` and `entry` is the image's checked no-argument entry"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 234,
      "column": 5,
      "reason": "the slot is an aligned word inside the mapping, now writable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 247,
      "column": 9,
      "reason": "this loaded image's checked zero-argument Unit finalizer remains executable; no native owners or returning frames survive"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 180,
      "column": 5,
      "reason": "state is the compiler-process-lifetime fourteen-word execution resource"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 184,
      "column": 5,
      "reason": "state is the compiler-process-lifetime fourteen-word execution resource"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 207,
      "column": 5,
      "reason": "terminated paths and admitted identity and five writable resource words remain live through posix_spawn"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 232,
      "column": 5,
      "reason": "the raw host leaf returns the compiler-process-lifetime resource words; requests serialize access"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 238,
      "column": 5,
      "reason": "logical identities share the process-owned execution counter and mutex"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 241,
      "column": 5,
      "reason": "release the private execution mutex acquired above"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 252,
      "column": 5,
      "reason": "wait writes only the owned status word, after this child's socket was closed"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 262,
      "column": 5,
      "reason": "close is serialized with every request on this process-owned host"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 267,
      "column": 5,
      "reason": "release the private execution mutex acquired above"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 272,
      "column": 5,
      "reason": "logical close serializes with turns and shared-child teardown"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 274,
      "column": 5,
      "reason": "release the execution mutex acquired for this logical close"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 318,
      "column": 5,
      "reason": "all compiler threads share this one process-owned execution resource"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 320,
      "column": 5,
      "reason": "release the private execution mutex acquired above"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 492,
      "column": 5,
      "reason": "this is the canonical runtime's checked single-word C ABI"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 696,
      "column": 13,
      "reason": "each initialized argument is inside this request's mapped cells"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 702,
      "column": 9,
      "reason": "execution_begin takes three u64 words: fuel, depth, is_comptime"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 707,
      "column": 9,
      "reason": "entry is the selector-generated pointer-to-cells adapter for this checked request; images and runtime stay pinned"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 717,
      "column": 17,
      "reason": "successful result storage belongs to this exact pinned image's generated typed release glue"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 723,
      "column": 21,
      "reason": "the successful checked adapter initialized its result cell"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 39,
      "column": 1,
      "reason": "a raw Linux x86-64 system call; the caller passes the call number and arguments that call expects"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 85,
      "column": 5,
      "reason": "the caller owns `count` writable bytes at `address`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 92,
      "column": 5,
      "reason": "the caller owns `count` readable bytes at `address`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 100,
      "column": 5,
      "reason": "an anonymous mapping aliases no existing memory"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 107,
      "column": 5,
      "reason": "the caller owns the mapping it changes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 113,
      "column": 5,
      "reason": "the caller owns the mapping and nothing uses it any more"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 120,
      "column": 5,
      "reason": "exit_group ends every thread of the process"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 125,
      "column": 1,
      "reason": "stores one word at an address the caller owns and has mapped writable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 132,
      "column": 1,
      "reason": "reads one word at an address the caller owns and has mapped readable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 139,
      "column": 1,
      "reason": "reads one byte at an address the caller owns and has mapped readable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 146,
      "column": 1,
      "reason": "stores one byte at an address the caller owns and has mapped writable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 175,
      "column": 9,
      "reason": "the caller owns the destination words"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 185,
      "column": 1,
      "reason": "calls machine code; the caller vouches for the code and its System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 198,
      "column": 1,
      "reason": "calls machine code; the caller vouches for the code and its two-word System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 213,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked zero-argument Unit System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 223,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked three-word Unit System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 240,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked one-word Unit System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 252,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked two-word Unit System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 267,
      "column": 1,
      "reason": "dlopen reads the NUL-terminated path at `path`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 275,
      "column": 1,
      "reason": "dlsym reads the NUL-terminated name at `name`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 298,
      "column": 5,
      "reason": "`name` is a NUL-terminated copy of `path`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 310,
      "column": 5,
      "reason": "`name` is a NUL-terminated copy of `symbol`; dlsym only reads the library's symbol table"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 319,
      "column": 1,
      "reason": "a raw Linux x86-64 syscall; the caller supplies the syscall's valid machine-word arguments"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 354,
      "column": 1,
      "reason": "reads one aligned machine word in live allocator-owned mapped memory"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 361,
      "column": 1,
      "reason": "stores one aligned machine word in live allocator-owned mapped memory; x86 TSO makes this a release store"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 370,
      "column": 1,
      "reason": "atomically compares and exchanges one aligned live machine word; every non-atomic access to protected data holds its lock"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 385,
      "column": 1,
      "reason": "x86 pause only hints that this thread is waiting in a spin loop"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 392,
      "column": 1,
      "reason": "reads one byte from a live readable mapped address supplied by the caller"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 399,
      "column": 1,
      "reason": "writes one byte to a live writable mapped address supplied by the caller"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 413,
      "column": 1,
      "reason": "writes at most eight bytes from a stack-local word while that word is live; the balanced pushes and pops do not expose stack storage after return"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 438,
      "column": 1,
      "reason": "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 485,
      "column": 1,
      "reason": "the calling thread is attached and GS points at its live Jet TCB"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 494,
      "column": 1,
      "reason": "ARCH_GET_GS writes one initialized aligned stack word only during this syscall; no TLS or heap is read, and the push/pop is balanced"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 509,
      "column": 1,
      "reason": "atomically adds to one aligned live word and returns its previous value"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 522,
      "column": 1,
      "reason": "atomically exchanges one aligned live word; xchg has an implicit full barrier"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 535,
      "column": 1,
      "reason": "calls executable System V code with one raw word argument"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 548,
      "column": 1,
      "reason": "calls a Jet closure's code with the live closure header and one raw word argument"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 566,
      "column": 1,
      "reason": "raw clone child runs only Jet/syscall code, never Rust or musl; supplied stack contains entry and argument"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 607,
      "column": 1,
      "reason": "calls checked System V Unit code; the caller lends one live raw machine-word argument"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 619,
      "column": 1,
      "reason": "calls checked System V Unit code; the caller vouches for both raw machine-word arguments"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 633,
      "column": 1,
      "reason": "calls checked System V Unit code; the caller vouches for all three raw machine-word arguments"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 649,
      "column": 1,
      "reason": "calls a checked native runtime callback with one machine-word argument"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 662,
      "column": 1,
      "reason": "calls a checked native runtime callback with two machine-word arguments"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 677,
      "column": 1,
      "reason": "calls a checked native runtime callback with three machine-word arguments"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 694,
      "column": 1,
      "reason": "calls a checked native runtime callback with four machine-word arguments"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 716,
      "column": 1,
      "reason": "source and directory are live owned descriptors, name is terminated; linkat borrows a stack-local empty path and retains no pointer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 738,
      "column": 1,
      "reason": "ioctl reads one stack-local machine word with the supplied audited request and live fd; it retains no pointer and all pushes are balanced"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 762,
      "column": 1,
      "reason": "object is a live fd for fstat or a terminated path for stat/lstat; output lends 128 writable bytes; the initialized stack stat frame never escapes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 851,
      "column": 1,
      "reason": "the audited caller lends the stat scalar row and valid syscall object"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 853,
      "column": 5,
      "reason": "packed Linux 32-bit fields become independent portable machine words without any allocation"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 865,
      "column": 1,
      "reason": "calls a checked System V void(void) process entry that remains executable for the entire call"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 876,
      "column": 1,
      "reason": "directory and path identify a source-approved statx object; output lends three writable words; the zeroed 256-byte statx stack buffer never escapes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 974,
      "column": 5,
      "reason": "address names an initialized canonical native word owned by the pinned request or retained slot"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 981,
      "column": 5,
      "reason": "address names an initialized canonical native byte owned by the pinned request or retained slot"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 988,
      "column": 5,
      "reason": "width and address range are checked; the caller retains the initialized slot's physical permit"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1024,
      "column": 5,
      "reason": "the caller owns exactly size writable bytes under the native payload permit; width and value were checked above"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1042,
      "column": 9,
      "reason": "both addresses have the checked canonical alloc(size,align) / free(pointer,size,align) ABI"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1058,
      "column": 5,
      "reason": "the allocation receipt pairs this owner with its original free address and exact size/alignment"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1066,
      "column": 5,
      "reason": "the admitted runtime signature or pinned clone/drop role was checked before invoking its fixed word ABI"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1074,
      "column": 1,
      "reason": "calls machine code; the caller vouches for its three-word System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionOutput.jet",
      "line": 7,
      "column": 5,
      "reason": "fd is this host's owned anonymous output file; no request is running"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionOutput.jet",
      "line": 16,
      "column": 5,
      "reason": "the child's completed request no longer writes this anonymous file"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionOutput.jet",
      "line": 36,
      "column": 13,
      "reason": "index is within the bytes just read into this mapping"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 6,
      "column": 1,
      "reason": "returns the process-lifetime execution resource words; the caller serializes access"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 16,
      "column": 1,
      "reason": "serializes access to this process's private execution resource"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 24,
      "column": 1,
      "reason": "releases the private execution mutex acquired by this thread"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 32,
      "column": 1,
      "reason": "posix_spawn creates a fresh process, not a forked copy of compiler/runtime locks; paths and output are caller-owned"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 77,
      "column": 1,
      "reason": "waits only for this owned child; status points at one writable word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 96,
      "column": 5,
      "reason": "sendto borrows the caller-owned readable bytes; MSG_NOSIGNAL is 0x4000 on Linux"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 101,
      "column": 5,
      "reason": "fd belongs to this private execution host"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 149,
      "column": 9,
      "reason": "header has32 writable bytes; a chunk count has at most5 decimal digits"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 158,
      "column": 9,
      "reason": "both decimal digits are inside the caller-owned header scratch"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 165,
      "column": 5,
      "reason": "the newline follows at most5 digits in32 writable header bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 213,
      "column": 9,
      "reason": "one byte was read into this owned mapping"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/SessionTransport.jet",
      "line": 229,
      "column": 13,
      "reason": "index is within the bytes just read into this mapping"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Tests/CallImages.jet",
      "line": 138,
      "column": 5,
      "reason": "the separate observer mapping remains owned, readable and writable until the final assertion"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Tests/CallableStatics.jet",
      "line": 77,
      "column": 5,
      "reason": "the loaded image owns both observer pointers and their writable static cells through unload; fini is the checked zero-argument Unit function"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Tests/SessionHostFixtures.jet",
      "line": 151,
      "column": 9,
      "reason": "half-close only this owned socket's write side; leave read open for HostError"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Tests/SessionHostFixtures.jet",
      "line": 291,
      "column": 5,
      "reason": "the checked fixture is a no-argument System V function returning the word 7; its mapping is retained through the call"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 69,
      "column": 5,
      "reason": "anonymous private mmap owns fresh writable bytes and cannot alias a live heap block; a failed syscall produces no allocation"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 83,
      "column": 5,
      "reason": "the caller has removed this mapping from all heap lists and no payload remains live"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 98,
      "column": 5,
      "reason": "root is the aligned lock word in the process-lifetime state mapping; CAS acquires all protected slab metadata"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 104,
      "column": 5,
      "reason": "the calling thread holds the heap lock; x86 release store publishes its completed metadata updates"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 111,
      "column": 5,
      "reason": "slab and its linked neighbours are live heap-owned headers protected by the held heap lock"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 129,
      "column": 5,
      "reason": "head and slab are live heap-owned metadata protected by the held heap lock"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 145,
      "column": 5,
      "reason": "the fresh 64KiB mapping holds every aligned free-block link; the heap lock excludes all list readers"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 162,
      "column": 5,
      "reason": "the held heap lock protects the class list, block links and slab live count; the chosen block has at least size+16+align-1 bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 195,
      "column": 5,
      "reason": "pointer's two header words and the aligned payload are wholly within this fresh page-rounded mapping"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 220,
      "column": 5,
      "reason": "the caller holds the policy lock and cell is a counter in alloc_root"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 228,
      "column": 5,
      "reason": "the aligned mode word is read atomically on x86; system allocation needs no policy lock"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 234,
      "column": 5,
      "reason": "the policy lock exclusively owns the mode, cap and accounting words"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 263,
      "column": 5,
      "reason": "the policy lock exclusively owns live bytes; restoration does not erase outstanding tracked blocks"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 277,
      "column": 5,
      "reason": "the held policy lock serializes all native allocator configuration and reservations"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 294,
      "column": 5,
      "reason": "the policy lock serializes restoration with native reserve and free; tracked blocks remain in the side table"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 308,
      "column": 5,
      "reason": "the policy lock protects this bounded fact word in alloc_root"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 323,
      "column": 5,
      "reason": "the private raw-heap node is fully initialized before linking under the policy/tracker lock"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 336,
      "column": 5,
      "reason": "the process-lifetime tracker count is an aligned x86 word; a zero count avoids any lookup or lock"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 343,
      "column": 5,
      "reason": "the policy/tracker lock exclusively owns all side-table links and count"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 426,
      "column": 9,
      "reason": "the lowering returns this still-live direct allocation with exactly its original size and alignment"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 435,
      "column": 5,
      "reason": "the lowering frees exactly once with the allocation's original layout; its slab remains live until this last payload is returned under the heap lock"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 464,
      "column": 5,
      "reason": "remaining is in 1..=8, so write borrows only the initialized stack word; a short write advances within it"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 504,
      "column": 5,
      "reason": "the stack-word syscalls unblock SIGABRT and, if necessary, restore its default disposition before raising it again; no heap pointer is needed"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Atomic.jet",
      "line": 14,
      "column": 5,
      "reason": "place is caller-owned aligned8 private unpublished storage for one atomic word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Atomic.jet",
      "line": 19,
      "column": 5,
      "reason": "place borrows one aligned8 live atomic word; x86 aligned MOV is the SeqCst load mapping"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Atomic.jet",
      "line": 24,
      "column": 5,
      "reason": "place borrows one aligned8 live atomic word; XCHG is the full-barrier SeqCst store mapping"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Atomic.jet",
      "line": 29,
      "column": 5,
      "reason": "place borrows one aligned8 live atomic word; LOCK CMPXCHG orders success and failure SeqCst"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Atomic.jet",
      "line": 34,
      "column": 5,
      "reason": "place borrows one aligned8 live atomic word; LOCK XADD returns the previous wrapping64 value SeqCst"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/ClosureRefs.jet",
      "line": 9,
      "column": 5,
      "reason": "the caller's owned reference keeps the aligned closure header live; locked compare-exchange atomically publishes one more owner"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/ClosureRefs.jet",
      "line": 20,
      "column": 5,
      "reason": "consumes one reference to an aligned live closure header; locked compare-exchange provides release ordering and last-owner acquire before its release helper"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 9,
      "column": 5,
      "reason": "data lends index's initialized native U8 element; low byte is meaningful in either epoch"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 14,
      "column": 5,
      "reason": "data owns index's complete native U8 slot; initialize word-epoch padding before low-byte store"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 72,
      "column": 5,
      "reason": "checked native value-returning callback; its raw argument stays valid for the synchronous call"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 78,
      "column": 5,
      "reason": "checked native value-returning callback; both raw arguments stay valid for the synchronous call"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 84,
      "column": 5,
      "reason": "checked unary closure adapter with borrowed environment, input and output slots"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 90,
      "column": 5,
      "reason": "checked binary closure adapter with live environment, inputs and output slots"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 96,
      "column": 5,
      "reason": "checked native Unit callback; its raw argument stays valid for the synchronous call"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 102,
      "column": 5,
      "reason": "checked Unit clone callback; source remains live and target lends writable output storage"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/CompilerFS.jet",
      "line": 155,
      "column": 17,
      "reason": "the prefix remains terminated and scratch lends 128 writable portable stat-fact bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 113,
      "column": 13,
      "reason": "the owned readable file lends its descriptor; mmap creates exactly length read-only private bytes; the map owner alone unmaps them"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/CountMinSketch.jet",
      "line": 9,
      "column": 5,
      "reason": "caller holds this native CMS mutex and owns the initialized four-byte counter"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/CryptoEntropy.jet",
      "line": 49,
      "column": 13,
      "reason": "the test/fault harness owns the synchronous callback and context until reset; the callback borrows only this writable suffix"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/CryptoEntropy.jet",
      "line": 54,
      "column": 5,
      "reason": "Linux getrandom flags0 writes only the exclusively borrowed writable span and retains no address; kernel negative errno is preserved"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/DataEvent.jet",
      "line": 5,
      "column": 5,
      "reason": "address borrows an initialized word in a live native DataEvent or List"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/DataEvent.jet",
      "line": 8,
      "column": 5,
      "reason": "address owns a writable word in a native DataEvent"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/DataLimits.jet",
      "line": 6,
      "column": 5,
      "reason": "address borrows an initialized word in a live native limits carrier"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/DataLimits.jet",
      "line": 9,
      "column": 5,
      "reason": "address owns a writable word in a native limits carrier or getter out slot"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Dyn.jet",
      "line": 7,
      "column": 5,
      "reason": "address borrows an initialized word in a live native Dyn or List"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Dyn.jet",
      "line": 10,
      "column": 5,
      "reason": "address owns a writable word in a native Dyn or List"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Deadline.jet",
      "line": 65,
      "column": 5,
      "reason": "clock_gettime writes two thread-owned TCB words, not retained after the syscall"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Display.jet",
      "line": 8,
      "column": 5,
      "reason": "borrows an initialized aligned word in an E4 header, exact-Int node or owned formatter workspace"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Display.jet",
      "line": 14,
      "column": 5,
      "reason": "writes an aligned word in an exclusively owned formatter workspace or fresh E4 header"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Display.jet",
      "line": 20,
      "column": 5,
      "reason": "reads one initialized byte in an owned formatter digit buffer or borrowed exact-Int limb"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Display.jet",
      "line": 26,
      "column": 5,
      "reason": "writes one byte in an exclusively owned formatter buffer; value is in 0..255"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/EncodingErrors.jet",
      "line": 6,
      "column": 5,
      "reason": "borrows an initialized aligned word in a live encoding carrier or option input"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/EncodingErrors.jet",
      "line": 9,
      "column": 5,
      "reason": "owns a writable aligned word in a fresh encoding carrier or caller output"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/GeometryGlue.jet",
      "line": 5,
      "column": 5,
      "reason": "borrows an initialized aligned word inside a live native geometry carrier"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/GeometryGlue.jet",
      "line": 8,
      "column": 5,
      "reason": "owns a writable aligned word in a fresh native geometry carrier"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 40,
      "column": 5,
      "reason": "borrows an initialized word in the live body, chunks, closure, List or E4 allocation"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 46,
      "column": 5,
      "reason": "owns the word, or holds the body's transition lock for this state field"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 52,
      "column": 5,
      "reason": "atomic refcount, take/close flag or transition lock in a live body allocation"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 103,
      "column": 5,
      "reason": "the last closure reference calls its generated release(box), which drops captures and frees the box"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 213,
      "column": 5,
      "reason": "the claimed closer code has the audited code(box) ABI, with no transition lock held"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 326,
      "column": 17,
      "reason": "reader code(box, descriptor) borrows exactly count writable U8 bytes; no body lock is held"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 379,
      "column": 17,
      "reason": "the source holds count initialized UTF8 or raw reader bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 14,
      "column": 5,
      "reason": "runtime startup owns a fresh anonymous mapping before any program thread exists"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 28,
      "column": 5,
      "reason": "rt_sigaction synchronously borrows the four-word SIG_IGN action; no signal handler or restorer runs for ignored SIGPIPE"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 45,
      "column": 5,
      "reason": "the caller lends length readable bytes for the complete synchronous write"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 60,
      "column": 5,
      "reason": "the output lock exclusively owns the initialized buffer and length"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 88,
      "column": 5,
      "reason": "the caller holds the output lock; each byte stays within the 8192-byte mapping-owned buffer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 108,
      "column": 5,
      "reason": "the caller lends length bytes; the output lock owns the destination buffer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 153,
      "column": 5,
      "reason": "bytes lends length readable bytes; writev borrows its own LF until return"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 170,
      "column": 5,
      "reason": "the held host lock lends enough scratch for Linux TCGETS"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 193,
      "column": 5,
      "reason": "value is a borrowed E4 JetCText header; stdout copies its bytes before returning"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 8,
      "column": 5,
      "reason": "the caller borrows a live FD72 or runtime scratch word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 14,
      "column": 5,
      "reason": "the caller owns the FD72 lock or exclusive runtime scratch word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 54,
      "column": 5,
      "reason": "the fresh path allocation includes its terminator byte"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 264,
      "column": 9,
      "reason": "the caller lends 128 writable bytes for portable stat facts"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 352,
      "column": 5,
      "reason": "the terminated borrowed path and 128-byte stat fact output are live for the syscall"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 403,
      "column": 5,
      "reason": "the caller lends one writable Bool byte"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 418,
      "column": 9,
      "reason": "the held directory and terminated source-approved single name stay live through openat"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 440,
      "column": 9,
      "reason": "the held directory and terminated final name live through O_PATH|O_NOFOLLOW|O_CLOEXEC openat; no read access or link traversal"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 448,
      "column": 9,
      "reason": "openat returned an exclusively owned descriptor and the caller lends StatFacts128; fstat copies initialized scalar facts"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 482,
      "column": 5,
      "reason": "both duplicated descriptors and the terminated name live through the held-inode linkat syscall"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 504,
      "column": 9,
      "reason": "the descriptor is held live, output begins with the borrowed empty pathname, statx writes only its private stack frame"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 515,
      "column": 5,
      "reason": "the terminated source-approved path and three writable birth-time output words stay live through statx"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 569,
      "column": 5,
      "reason": "both terminated borrowed names and the caller's optional compact byte output remain live only through lgetxattr"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 598,
      "column": 9,
      "reason": "the held directory, terminated plain name and writable byte range are borrowed only through readlinkat"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 624,
      "column": 5,
      "reason": "held directory duplicates and terminated names prevent any descriptor-close/reuse race during renameat2"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 644,
      "column": 9,
      "reason": "the held fd and stack-local source-validated winsize word live only through ioctl"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 21,
      "column": 5,
      "reason": "value transfers one 56-byte error header and its nullable owned E4 text children"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 32,
      "column": 5,
      "reason": "value borrows the complete error header; result exclusively owns 56 fresh bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 65,
      "column": 5,
      "reason": "value exclusively owns a new initialized IOError with two owned text children; error lends the caller's pointer slot"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 82,
      "column": 5,
      "reason": "value lends five initialized context words; result exclusively owns its header and cloned nullable E4 children"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 98,
      "column": 5,
      "reason": "value transfers its 40-byte header and two independently owned nullable text children"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 114,
      "column": 9,
      "reason": "the optional code box borrows one initialized exact-Int word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 120,
      "column": 5,
      "reason": "nullable option boxes lend one initialized payload word; value owns cloned texts and the complete new context"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 131,
      "column": 5,
      "reason": "value borrows an initialized context header"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 135,
      "column": 5,
      "reason": "value borrows a context; some lends the writable pointer output for a present field"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 147,
      "column": 5,
      "reason": "value borrows a context; some lends the writable exact-Int word output for a present field"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 155,
      "column": 5,
      "reason": "value borrows an initialized error header"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 159,
      "column": 5,
      "reason": "value borrows a complete error header; the selected payload must match its canonical tag"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 170,
      "column": 5,
      "reason": "context transfers its initialized five words and text owners into the fresh error; only the context header is then freed"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 184,
      "column": 5,
      "reason": "value borrows an initialized ResourceLimit error"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 199,
      "column": 5,
      "reason": "value exclusively owns seven initialized words; ResourceLimit has no text children"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 8,
      "column": 5,
      "reason": "value exclusively owns four initialized words; inputs lend exact Int owners and a valid E4 message"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 18,
      "column": 5,
      "reason": "value lends the complete immutable carrier and all three owned children"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 25,
      "column": 5,
      "reason": "value transfers its 32-byte header, two independently retained Int words and owned E4 message"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 34,
      "column": 5,
      "reason": "value lends the immutable carrier; return retains its exact Int field"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 38,
      "column": 5,
      "reason": "value lends the initialized presence word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 42,
      "column": 5,
      "reason": "value lends the immutable carrier; return retains its exact Int field"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 46,
      "column": 5,
      "reason": "value lends the immutable carrier; return deep-clones its owned E4 message"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 5,
      "column": 5,
      "reason": "value lends a complete initialized network header"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 8,
      "column": 5,
      "reason": "value exclusively owns the network header or its mutex"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 38,
      "column": 5,
      "reason": "value owns sixteen octets; bytes lends length initialized U8 elements"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 50,
      "column": 5,
      "reason": "value lends IP24; data owns length initialized native U8 elements; output receives ownership"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 83,
      "column": 5,
      "reason": "bytes lends width initialized little-endian bytes (width<=8)"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 93,
      "column": 5,
      "reason": "bytes owns width writable little-endian bytes (width<=8)"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNetPoller.jet",
      "line": 60,
      "column": 5,
      "reason": "the root lock owns a packed 12B epoll_event; state retains the native fd through this synchronous call"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNetPoller.jet",
      "line": 86,
      "column": 9,
      "reason": "events owns 64 packed epoll_event entries; epoll_wait retains no buffer pointer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNetResolver.jet",
      "line": 104,
      "column": 5,
      "reason": "host lends length E4 bytes; outputs lend their writable pointer/count words"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostNetResolver.jet",
      "line": 124,
      "column": 5,
      "reason": "name owns a trailing NUL byte"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostPipe.jet",
      "line": 128,
      "column": 13,
      "reason": "the FD72 lock protects this initialized refill-buffer byte"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 5,
      "column": 5,
      "reason": "the audited caller supplies the Linux syscall's valid word arguments"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 66,
      "column": 5,
      "reason": "the caller lends the C-ABI tuple's Bool byte and two writable Int words"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 143,
      "column": 5,
      "reason": "some lends a writable C-ABI Option success word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 153,
      "column": 5,
      "reason": "status lends an eight-byte out slot; wait4 writes its low four bytes and retains no pointer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 165,
      "column": 5,
      "reason": "read_fd lends eight writable bytes to pipe's two u32 results; split both facts into the caller's separate word slots"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 13,
      "column": 9,
      "reason": "the FD72 lock prevents native descriptor close/reuse throughout this nonblocking syscall"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 54,
      "column": 13,
      "reason": "accept4 retains no address pointers and atomically sets CLOEXEC/NONBLOCK on the accepted socket"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 61,
      "column": 13,
      "reason": "private scratch lends sockaddr bytes and its exact 4B socklen; accept4 retains neither pointer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 96,
      "column": 9,
      "reason": "private scratch lends one native int and its 4B socklen; getsockopt retains neither pointer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 111,
      "column": 9,
      "reason": "private scratch lends exactly one native int; setsockopt retains no pointer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 135,
      "column": 5,
      "reason": "data lends length native U8 elements; output owns length compact bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 149,
      "column": 9,
      "reason": "the held FD72 lock lends its live nonblocking socket; packed lends exactly length bytes; no pointer survives recvfrom/sendto"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 154,
      "column": 13,
      "reason": "recvfrom initialized result compact bytes; data owns the corresponding mutable native U8 slots"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 185,
      "column": 9,
      "reason": "the FD72 lock retains the socket; private scratch lends sockaddr bytes; packed lends length bytes; sendto retains no pointers"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 202,
      "column": 9,
      "reason": "private scratch lends sockaddr/socklen and packed lends length mutable bytes; MSG_TRUNC returns the complete datagram length while writing at most length bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 211,
      "column": 17,
      "reason": "recvfrom initialized used compact bytes; the caller owns used mutable native U8 elements"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 235,
      "column": 5,
      "reason": "path lends its immutable E4 bytes for std-compatible sockaddr_un validation"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 257,
      "column": 13,
      "reason": "scratch owns the sockaddr_un pathname and its final zero byte"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 224,
      "column": 17,
      "reason": "Linux TCGETS initialized the 36-byte termios buffer; cfmakeraw sets VMIN=1, VTIME=0 in its control-character array"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 291,
      "column": 13,
      "reason": "rt_sigaction borrows four initialized stack words; no allocator, mutex or TLS is touched after fork"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 709,
      "column": 9,
      "reason": "ioctl borrows one stack word of source-validated winsize dimensions; no pointer escapes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 725,
      "column": 5,
      "reason": "source lends a NUL-terminated raw path including non-Unicode bytes and a writable 128-byte stat fact row"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 736,
      "column": 5,
      "reason": "the source-approved native NUL path and Jet-owned PATH_MAX buffer are live across the mechanical realpath call"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 741,
      "column": 9,
      "reason": "libc __errno_location returns this calling thread's live errno int; read only its initialized four bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 752,
      "column": 5,
      "reason": "successful realpath NUL-terminates its result within the PATH_MAX byte buffer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 22,
      "column": 5,
      "reason": "bytes borrows one live NUL-terminated startup string"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 45,
      "column": 5,
      "reason": "startup lends the immutable, null-terminated envp pointer array"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 58,
      "column": 5,
      "reason": "rows exclusively owns raw_count index records; startup lends every pointed-to NUL string"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 79,
      "column": 5,
      "reason": "storage exclusively owns total_bytes; each source row lends its NUL string for this capture call, and published rows point only into the owned block"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 111,
      "column": 5,
      "reason": "bytes lends one live NUL-terminated startup string"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 125,
      "column": 5,
      "reason": "startup lends length bytes; data exclusively owns length initialized packed bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 138,
      "column": 5,
      "reason": "output lends a 24-byte native List<U8> header and receives ownership of data"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 152,
      "column": 5,
      "reason": "row borrows a complete immutable startup index record"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 167,
      "column": 5,
      "reason": "row borrows its immutable record; data_out lends one writable pointer word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 176,
      "column": 5,
      "reason": "row borrows its immutable record; data_out lends one writable pointer word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 192,
      "column": 5,
      "reason": "startup lends argc initialized argv pointers and their immutable NUL strings"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 211,
      "column": 5,
      "reason": "the startup index and key bytes are immutable borrowed data"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 21,
      "column": 5,
      "reason": "value borrows a complete immutable startup NUL string; output lends one writable U64 word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 48,
      "column": 5,
      "reason": "value borrows length immutable captured startup bytes; no pointer escapes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 82,
      "column": 5,
      "reason": "the held Host lock lends 16 writable bytes of its existing ioctl scratch; the canonical lookup retains no key pointer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 92,
      "column": 5,
      "reason": "output exclusively borrows the paired initialized empty Source out-record; no owner is overwritten"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 115,
      "column": 5,
      "reason": "Host startup already initialized its process-lifetime mapping; the held lock owns ioctl scratch32..47 and FIRST raw lookup only borrows the immutable captured index"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 147,
      "column": 13,
      "reason": "the paired Source owner receives the sole native U8 buffer with exact LEN/CAP/DATA; no raw pointer is cast into a List value"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStdio.jet",
      "line": 13,
      "column": 5,
      "reason": "text lends the E4 header and readable bytes for this synchronous write"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStdio.jet",
      "line": 20,
      "column": 5,
      "reason": "text lends the E4 header and readable bytes; the LF enters the same stream buffer without a temporary String"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStdio.jet",
      "line": 41,
      "column": 5,
      "reason": "text lends the E4 header and readable bytes for this synchronous write"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStdio.jet",
      "line": 48,
      "column": 5,
      "reason": "text lends the E4 header and readable bytes; writev borrows a stack LF, never an allocated line String"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStdio.jet",
      "line": 71,
      "column": 5,
      "reason": "the caller lends the tuple's exact-Int word and Bool byte outputs"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStdio.jet",
      "line": 78,
      "column": 5,
      "reason": "text lends an E4 header and its bytes for the synchronous buffered write"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostStdio.jet",
      "line": 96,
      "column": 5,
      "reason": "text lends an E4 header and its bytes; writev borrows its own stack LF"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 27,
      "column": 5,
      "reason": "clock_gettime synchronously writes the calling thread's two scratch words"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 56,
      "column": 5,
      "reason": "before_out lends the tuple's one writable Bool byte"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 148,
      "column": 5,
      "reason": "path lends length bytes; std file open rejects embedded NUL"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 156,
      "column": 5,
      "reason": "name owns its trailing NUL byte"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 189,
      "column": 13,
      "reason": "bytes lends used initialized bytes; data owns used native U8 elements"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/HyperLogLog.jet",
      "line": 332,
      "column": 9,
      "reason": "one native HLL mutex protects this packed register byte"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 10,
      "column": 5,
      "reason": "borrow an initialized word in a live exact-Int carrier"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 16,
      "column": 5,
      "reason": "write an initialized word in a uniquely owned exact-Int workspace"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 22,
      "column": 5,
      "reason": "borrow four initialized bytes in a live limb buffer; no adjacent limb is read"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 28,
      "column": 5,
      "reason": "write four bytes in a uniquely owned limb buffer; value is an unsigned 32-bit limb"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 300,
      "column": 9,
      "reason": "the caller holds an existing owner; atomic retain cannot race final-owner retirement"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 321,
      "column": 9,
      "reason": "the caller consumes one owner; atomic decrement publishes refs0 before shared-domain retirement"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 326,
      "column": 13,
      "reason": "the final owner has reached zero; the remaining shared hazard domain decides immediate free or retirement"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/IntHazard.jet",
      "line": 90,
      "column": 5,
      "reason": "the native numeric forwarder lends a pure reload callback and its live closure context for this synchronous call"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 106,
      "column": 5,
      "reason": "entry is the loader/object's checked executable System V void(void) address, not a closure header"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/MemStorage.jet",
      "line": 22,
      "column": 5,
      "reason": "address borrows an initialized word in a native allocator header, descriptor, drop record, or Fixed metadata span"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/MemStorage.jet",
      "line": 25,
      "column": 5,
      "reason": "address exclusively owns one aligned writable allocator or Fixed metadata word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/MemStorage.jet",
      "line": 78,
      "column": 5,
      "reason": "code is the Lower-generated address-based lazy.drop thunk for the initialized value at pointer; its SysV result is Unit and discarded, and no allocator lock is held"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/NativeLayoutABI.jet",
      "line": 11,
      "column": 5,
      "reason": "thread-confined live Layout storage lends this initialized aligned word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/NativeLayoutABI.jet",
      "line": 14,
      "column": 5,
      "reason": "thread-confined native Layout storage owns this aligned word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/ObserveLive.jet",
      "line": 135,
      "column": 5,
      "reason": "vetted Observe Linux file/clock syscalls borrow native buffers only for their duration"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Parallel.jet",
      "line": 31,
      "column": 9,
      "reason": "checked System V (env,start,end)->Unit callback; env remains borrowed and each range has exclusive output slots"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/ScalarReflectGlue.jet",
      "line": 4,
      "column": 5,
      "reason": "borrows an initialized aligned word in a live scalar/reflection carrier"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/ScalarReflectGlue.jet",
      "line": 7,
      "column": 5,
      "reason": "owns a writable aligned word in a native scalar/reflection carrier"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 120,
      "column": 5,
      "reason": "the cold OS leaf borrows its initialized stack word; a successful nonzero base is this attached thread's live Jet TCB"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 719,
      "column": 9,
      "reason": "sched_getaffinity writes only the owned mask; pid0 names this thread"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 945,
      "column": 9,
      "reason": "sched_yield retains no memory and yields this OS worker"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 11,
      "column": 5,
      "reason": "address names the caller-owned live Secret buffer; effectful asm stores wipe every byte including spare capacity before release"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 24,
      "column": 5,
      "reason": "bytes is the exclusive moved native List<U8> header; secret is fresh aligned24B storage and adopts its sole buffer without copying secret material"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 42,
      "column": 5,
      "reason": "secret is a live exclusively borrowed native24B Secret header; len/cap/data follow the native List layout and all byte owners remain inside its buffer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 6,
      "column": 5,
      "reason": "borrows an aligned live synchronization word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 10,
      "column": 5,
      "reason": "publishes an aligned live synchronization word with x86 release ordering"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 16,
      "column": 5,
      "reason": "atomic exchange on an aligned live synchronization word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 22,
      "column": 5,
      "reason": "atomic fetch-add on an aligned live synchronization word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 28,
      "column": 5,
      "reason": "atomic compare-exchange on an aligned live synchronization word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 34,
      "column": 5,
      "reason": "futex wake borrows a live aligned word and retains no pointer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 42,
      "column": 5,
      "reason": "futex borrows live state; timed wait uses this attached thread's reserved TCB scratch until the syscall returns"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 54,
      "column": 5,
      "reason": "clock_gettime writes two words of thread-owned TCB scratch; no pointer escapes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/TaskGroup.jet",
      "line": 250,
      "column": 9,
      "reason": "the canonical task selector yields without consuming any child result"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Test.jet",
      "line": 124,
      "column": 9,
      "reason": "the OS leaf borrows at most eight initialized bytes from its own stack word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/TestSuite.jet",
      "line": 79,
      "column": 5,
      "reason": "the pinned named-fn box code takes its box and writable inline 16B result storage"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Text.jet",
      "line": 10,
      "column": 5,
      "reason": "address borrows an initialized word in a live text or iterator header"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Text.jet",
      "line": 16,
      "column": 5,
      "reason": "address is an exclusively owned writable header word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Text.jet",
      "line": 20,
      "column": 5,
      "reason": "address borrows an initialized byte inside the caller's live buffer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Text.jet",
      "line": 26,
      "column": 5,
      "reason": "address is one exclusively owned writable byte and value is a byte"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/TextGlue.jet",
      "line": 4,
      "column": 5,
      "reason": "borrows an initialized aligned word in a live native text diagnostic"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/TextGlue.jet",
      "line": 7,
      "column": 5,
      "reason": "owns a writable aligned word in a fresh native text diagnostic"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 50,
      "column": 5,
      "reason": "every caller has attached before running Jet TLS code"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 54,
      "column": 5,
      "reason": "GS is reserved for Jet and base is a live self-addressed TCB or zero at detach"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 61,
      "column": 5,
      "reason": "gettid identifies this thread without reading its inherited GS"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 243,
      "column": 9,
      "reason": "the registry owns this live closure header whose code accepts header and one I64 value"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 328,
      "column": 5,
      "reason": "the caller transferred a System V I64->I64 code address and one argument to this thread"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 378,
      "column": 5,
      "reason": "fresh raw clone stack mapping with a PROT_NONE first-page guard; child may call only Jet/syscall code, never musl/Rust with inherited FS"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 395,
      "column": 5,
      "reason": "guarded stack is seeded with Jet-only entry/arg; TID word is live until join; flags share VM, FS, FILES, SIGHAND, THREAD, SYSVSEM and enable parent-set/child-clear TID"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 410,
      "column": 9,
      "reason": "kernel CHILD_CLEARTID clears this live word and wakes a non-private futex; recheck handles exit-before-wait and EINTR"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/TimeGlue.jet",
      "line": 4,
      "column": 5,
      "reason": "borrows an initialized aligned word inside a native time carrier"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/TimeGlue.jet",
      "line": 7,
      "column": 5,
      "reason": "owns a writable aligned word inside a fresh native time carrier"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 9,
      "column": 5,
      "reason": "the caller borrows an initialized word of a live JetText or static require descriptor"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 15,
      "column": 5,
      "reason": "the caller borrows exactly one initialized byte of a live UTF-8 carrier"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 21,
      "column": 5,
      "reason": "the caller owns eight writable bytes in a scratch mapping or allocated JetText storage"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 27,
      "column": 5,
      "reason": "address is one writable byte in the allocated debug-text buffer; value is a byte"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 33,
      "column": 5,
      "reason": "audited runtime syscall: write borrows live text, mmap creates scratch, munmap releases it, exit_group ends the process"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 69,
      "column": 9,
      "reason": "the OS leaf borrows a stack word; generated calls have 1..8 valid bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/UiCarriersEnums.jet",
      "line": 5,
      "column": 5,
      "reason": "address borrows an initialized native UI carrier or E4 header word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/UiCarriersEnums.jet",
      "line": 10,
      "column": 5,
      "reason": "address exclusively owns one writable native UI carrier word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/Unicode.jet",
      "line": 88,
      "column": 5,
      "reason": "some is the caller's writable aligned Option<Int> payload slot, written only on success"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/World.jet",
      "line": 76,
      "column": 5,
      "reason": "registered provider code remains live at this world/lease boundary; output lends one writable word"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/XMLGlue.jet",
      "line": 5,
      "column": 5,
      "reason": "borrows an initialized aligned word in a live XML carrier or optional input"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Source/XMLGlue.jet",
      "line": 8,
      "column": 5,
      "reason": "owns a writable aligned word in a fresh XML carrier or caller output"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/AllocTests.jet",
      "line": 9,
      "column": 5,
      "reason": "a failed allocator witness terminates this isolated test process"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/AllocTests.jet",
      "line": 25,
      "column": 5,
      "reason": "both allocations own count bytes; touching their endpoints must not alias each other or their metadata"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/AllocTests.jet",
      "line": 69,
      "column": 5,
      "reason": "the test is the only heap user and the class has a retained empty slab"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/AllocTests.jet",
      "line": 76,
      "column": 9,
      "reason": "pointer's private header identifies its still-live slab, which must be the retained mapping"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/AllocTests.jet",
      "line": 80,
      "column": 9,
      "reason": "no other thread touches the retained slab or its state cell"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/AllocTests.jet",
      "line": 96,
      "column": 9,
      "reason": "the test owns the full payload and can read its allocator header"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/AllocTests.jet",
      "line": 109,
      "column": 9,
      "reason": "the chain word remains readable until its payload is freed"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/AllocTests.jet",
      "line": 115,
      "column": 5,
      "reason": "after all payloads are freed, exactly one empty slab is retained; any second empty slab was unlinked and unmapped"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/AllocTests.jet",
      "line": 162,
      "column": 5,
      "reason": "the exact success line is two bounded stack words written to stdout"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/AllocationPolicyTests.jet",
      "line": 4,
      "column": 5,
      "reason": "the single-threaded fixture reads the process-lifetime tracked-node count after all its payloads were freed"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/EnvOverlayKernel.jet",
      "line": 6,
      "column": 5,
      "reason": "failed witness terminates its isolated real-runtime process"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SchedulerTests.jet",
      "line": 5,
      "column": 5,
      "reason": "an isolated failed scheduler witness exits with no further runtime cleanup"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SchedulerTests.jet",
      "line": 190,
      "column": 5,
      "reason": "fixture byte writes stay within owned 19/20-byte text buffers"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SchedulerTests.jet",
      "line": 194,
      "column": 5,
      "reason": "fixture byte writes stay within owned 19/20-byte text buffers"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SchedulerTests.jet",
      "line": 198,
      "column": 5,
      "reason": "fixture byte writes stay within owned 19/20-byte text buffers"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SchedulerTests.jet",
      "line": 202,
      "column": 5,
      "reason": "fixture byte writes stay within owned 19/20-byte text buffers"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SchedulerTests.jet",
      "line": 211,
      "column": 5,
      "reason": "fixture byte writes stay within an owned one-byte text buffer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SchedulerTests.jet",
      "line": 215,
      "column": 5,
      "reason": "fixture byte writes stay within an owned one-byte text buffer"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SecretTests.jet",
      "line": 6,
      "column": 5,
      "reason": "a failed isolated Secret witness terminates this test process"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SecretTests.jet",
      "line": 14,
      "column": 5,
      "reason": "header is fresh aligned24B storage owned by the test; data owns cap times the host U8 stride bytes or is zero for an empty List"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SecretTests.jet",
      "line": 23,
      "column": 5,
      "reason": "the pointer is a live small allocation; this isolated test is the sole allocator user and can inspect its retained slab metadata"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SecretTests.jet",
      "line": 36,
      "column": 5,
      "reason": "input and secret are distinct live headers; adoption leaves one live buffer owner and borrowed bytes alias only that parent's header"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SecretTests.jet",
      "line": 57,
      "column": 5,
      "reason": "the test owns data and checks the initialized U8 slots, including epoch padding, before any release"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SecretTests.jet",
      "line": 68,
      "column": 5,
      "reason": "secret still owns its live24B header after Close; the buffer was released, and no pointer into it is used"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SecretTests.jet",
      "line": 80,
      "column": 5,
      "reason": "reused is newly allocated live32B storage exclusively owned by this witness, not a read of freed storage"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SecretTests.jet",
      "line": 112,
      "column": 5,
      "reason": "data is this test's live List allocation; every capacity byte is dirtied before the wipe"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SecretTests.jet",
      "line": 120,
      "column": 5,
      "reason": "the same live allocation remains exclusively owned while this test checks all wiped bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SecretTests.jet",
      "line": 138,
      "column": 5,
      "reason": "this isolated native witness writes its17B success literal and exits without introducing any text or collection runtime dependency"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SentryInlineTests.jet",
      "line": 8,
      "column": 1,
      "reason": "the raw pointer deliberately escapes its frame for the sentry witness"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SentryInlineTests.jet",
      "line": 13,
      "column": 1,
      "reason": "an expired inline raw read must stop before any pointee byte is loaded"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/SentryTests.jet",
      "line": 206,
      "column": 5,
      "reason": "the checked Source callback uses the exact two-word native ABI"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/TextTests.jet",
      "line": 6,
      "column": 5,
      "reason": "a failed text runtime witness terminates only its isolated process"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/ThreadTests.jet",
      "line": 8,
      "column": 5,
      "reason": "a failed isolated runtime witness ends the process without allocation or libc"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/ThreadTests.jet",
      "line": 36,
      "column": 5,
      "reason": "gettid verifies identity without trusting the TCB under test"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/ThreadTests.jet",
      "line": 45,
      "column": 5,
      "reason": "ARCH_GET_GS writes a caller-owned word; the inherited pointer is compared, never dereferenced"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/ThreadTests.jet",
      "line": 204,
      "column": 13,
      "reason": "sched_yield widens both sides of the check-to-wait race without timing assumptions"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/ThreadTests.jet",
      "line": 284,
      "column": 5,
      "reason": "heap-free fixture success line is one eight-byte write"
    },
    {
      "package": "jet",
      "file": "Compiler/JetRuntime/Tests/UnicodeTests.jet",
      "line": 8,
      "column": 5,
      "reason": "a failed Unicode runtime witness ends only its isolated test process"
    },
    {
      "package": "jet",
      "file": "Core/auth/auth.jet",
      "line": 112,
      "column": 5,
      "reason": "auth PASETO v4.public verification uses the fixed Ed25519 primitive"
    },
    {
      "package": "jet",
      "file": "Core/crypto/crypto.jet",
      "line": 689,
      "column": 5,
      "reason": "the expert API explicitly authorizes authenticated legacy-v1 migration"
    },
    {
      "package": "jet",
      "file": "Core/crypto/crypto.jet",
      "line": 1000,
      "column": 5,
      "reason": "the expert API explicitly authorizes authenticated legacy-v1 migration"
    },
    {
      "package": "jet",
      "file": "Core/crypto/crypto.jet",
      "line": 1541,
      "column": 5,
      "reason": "crypto expert: Argon2id password hashing"
    },
    {
      "package": "jet",
      "file": "Core/crypto/crypto.jet",
      "line": 1568,
      "column": 5,
      "reason": "crypto expert: Argon2id password verification"
    },
    {
      "package": "jet",
      "file": "Core/crypto/jetc.jet",
      "line": 96,
      "column": 5,
      "reason": "Core JETC2 applies the fixed XChaCha20-Poly1305 suite and protocol-owned nonce/AAD domains"
    },
    {
      "package": "jet",
      "file": "Core/crypto/jetc.jet",
      "line": 104,
      "column": 5,
      "reason": "Core JETC2 authenticates a bounded record before releasing its plaintext"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 224,
      "column": 5,
      "reason": "vault typed load provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 231,
      "column": 5,
      "reason": "vault typed current provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 237,
      "column": 5,
      "reason": "vault typed status provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 244,
      "column": 5,
      "reason": "vault typed versions provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 251,
      "column": 5,
      "reason": "vault typed generation provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 258,
      "column": 5,
      "reason": "vault typed store provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 266,
      "column": 5,
      "reason": "vault key import: signing key material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 274,
      "column": 5,
      "reason": "vault key import: X25519 secret material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 282,
      "column": 5,
      "reason": "vault wrapped import provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 289,
      "column": 5,
      "reason": "vault typed rotation provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 296,
      "column": 5,
      "reason": "vault typed retirement provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 303,
      "column": 5,
      "reason": "vault typed revocation provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 310,
      "column": 5,
      "reason": "vault write authorization provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 317,
      "column": 5,
      "reason": "vault wrapped authorization provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 323,
      "column": 5,
      "reason": "vault typed generation commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 329,
      "column": 5,
      "reason": "vault typed store commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 335,
      "column": 5,
      "reason": "vault key import commit: signing key material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 341,
      "column": 5,
      "reason": "vault key import commit: X25519 secret material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 347,
      "column": 5,
      "reason": "vault wrapped import commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 353,
      "column": 5,
      "reason": "vault typed rotation commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 359,
      "column": 5,
      "reason": "vault typed retirement commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 365,
      "column": 5,
      "reason": "vault typed revocation commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 371,
      "column": 5,
      "reason": "vault passphrase export provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 378,
      "column": 5,
      "reason": "vault recipient export provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/data/data.jet",
      "line": 1266,
      "column": 9,
      "reason": "native path bytes originate from the host environment and are split at ASCII boundaries"
    },
    {
      "package": "jet",
      "file": "Core/data/data.jet",
      "line": 1876,
      "column": 5,
      "reason": "inspect existing private Decode carriers only inside checked Core data source"
    },
    {
      "package": "jet",
      "file": "Core/db/db.jet",
      "line": 43,
      "column": 9,
      "reason": "consume the unique SQLite queue handle through its existing host close leaf"
    },
    {
      "package": "jet",
      "file": "Core/db/db.jet",
      "line": 49,
      "column": 5,
      "reason": "open the existing SQLite bridge and immediately adopt its handle into a consuming Core resource"
    },
    {
      "package": "jet",
      "file": "Core/db/db.jet",
      "line": 56,
      "column": 5,
      "reason": "borrow the live unique queue handle for BEGIN"
    },
    {
      "package": "jet",
      "file": "Core/db/db.jet",
      "line": 59,
      "column": 5,
      "reason": "borrow the live unique queue handle for COMMIT"
    },
    {
      "package": "jet",
      "file": "Core/db/db.jet",
      "line": 62,
      "column": 5,
      "reason": "borrow the live unique queue handle for ROLLBACK"
    },
    {
      "package": "jet",
      "file": "Core/db/db.jet",
      "line": 65,
      "column": 5,
      "reason": "execute bound queue SQL on the exclusively borrowed SQLite handle; host only marshals its existing wire"
    },
    {
      "package": "jet",
      "file": "Core/db/db.jet",
      "line": 70,
      "column": 5,
      "reason": "query bound queue SQL on the exclusively borrowed SQLite handle; host only marshals its existing wire"
    },
    {
      "package": "jet",
      "file": "Core/email/email.jet",
      "line": 77,
      "column": 5,
      "reason": "DKIM decoding: borrow encoded bytes and keep partial output in a zeroizing Secret"
    },
    {
      "package": "jet",
      "file": "Core/email/email.jet",
      "line": 413,
      "column": 5,
      "reason": "DKIM known-answer fixture reads only synthetic key bytes"
    },
    {
      "package": "jet",
      "file": "Core/encoding/cbor.jet",
      "line": 95,
      "column": 5,
      "reason": "inspect private typed carriers without changing public Dyn variants"
    },
    {
      "package": "jet",
      "file": "Core/encoding/cbor.jet",
      "line": 157,
      "column": 5,
      "reason": "validated private carriers retain byte strings for typed Encode"
    },
    {
      "package": "jet",
      "file": "Core/encoding/cbor_stream.jet",
      "line": 209,
      "column": 9,
      "reason": "the retained key copies exactly the already validated text bytes"
    },
    {
      "package": "jet",
      "file": "Core/encoding/cbor_stream.jet",
      "line": 312,
      "column": 17,
      "reason": "the exact CBOR text allocation passed strict UTF-8 validation"
    },
    {
      "package": "jet",
      "file": "Core/encoding/csv.jet",
      "line": 123,
      "column": 9,
      "reason": "input is a String; original complete UTF-8 scalars and ASCII quote escapes entered the field"
    },
    {
      "package": "jet",
      "file": "Core/encoding/csv.jet",
      "line": 318,
      "column": 5,
      "reason": "the exact owned query bytes passed the shared strict UTF-8 validator"
    },
    {
      "package": "jet",
      "file": "Core/encoding/csv.jet",
      "line": 768,
      "column": 9,
      "reason": "each completed UTF-8 scalar was validated before its original bytes entered this field"
    },
    {
      "package": "jet",
      "file": "Core/encoding/json.jet",
      "line": 101,
      "column": 5,
      "reason": "preserve the shipped decoder's hidden stderr effect at its existing host boundary"
    },
    {
      "package": "jet",
      "file": "Core/encoding/json.jet",
      "line": 109,
      "column": 5,
      "reason": "preserve the shipped decoder's hidden wall-clock snapshot at its existing host boundary"
    },
    {
      "package": "jet",
      "file": "Core/encoding/json.jet",
      "line": 706,
      "column": 5,
      "reason": "preserve the native compiler-private Dyn carriers in Core JSON event projection"
    },
    {
      "package": "jet",
      "file": "Core/encoding/json.jet",
      "line": 870,
      "column": 5,
      "reason": "Core JSON rendering borrows native private payloads without exposing their constructors"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 12,
      "column": 9,
      "reason": "the Object variant was checked immediately before borrowing its ordered backing"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 20,
      "column": 5,
      "reason": "transfer the owned ordered entries allocation into the native Object carrier"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 26,
      "column": 5,
      "reason": "check the private Bytes variant immediately before borrowing its backing"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 33,
      "column": 5,
      "reason": "the shared parser validated this exact numeric lexeme"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 39,
      "column": 5,
      "reason": "retain decoded JSON text until the target Decode contract is known"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 300,
      "column": 9,
      "reason": "the shared UTF-8 validator just accepted this owned byte allocation"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 371,
      "column": 9,
      "reason": "the shared UTF-8 validator just accepted this owned ASCII token"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 376,
      "column": 13,
      "reason": "retain a validated number lexeme in the private event carrier until typed decoding"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 555,
      "column": 9,
      "reason": "transfer already validated UTF-8 storage to inspect its actual owned capacity"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 566,
      "column": 9,
      "reason": "the private numeric event is projected only into the matching typed tree carrier"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 836,
      "column": 5,
      "reason": "the shared UTF-8 validator just accepted this owned allocation"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 845,
      "column": 5,
      "reason": "the byte position was checked against this String's storage length"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 1405,
      "column": 5,
      "reason": "the shared UTF-8 validator accepted this exact owned formatter buffer"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 1416,
      "column": 5,
      "reason": "transfer the owned UTF-8 String buffer before re-spelling its shortest digits"
    },
    {
      "package": "jet",
      "file": "Core/encoding/stream.jet",
      "line": 1742,
      "column": 5,
      "reason": "owned validated UTF-8 allocation transfers in the encoding source seam"
    },
    {
      "package": "jet",
      "file": "Core/encoding/toml.jet",
      "line": 779,
      "column": 5,
      "reason": "inspect compiler-private Bytes and typed carriers; public Dyn variants are unchanged"
    },
    {
      "package": "jet",
      "file": "Core/encoding/xml_engine.jet",
      "line": 134,
      "column": 5,
      "reason": "all callers iterate or check the UTF-8 byte bounds before borrowing this byte"
    },
    {
      "package": "jet",
      "file": "Core/encoding/xml_engine.jet",
      "line": 173,
      "column": 5,
      "reason": "the caller selected a scalar-aligned window of validated UTF-8 bytes"
    },
    {
      "package": "jet",
      "file": "Core/encoding/xml_engine.jet",
      "line": 2218,
      "column": 5,
      "reason": "attribute whitespace compaction preserves the already validated UTF-8 byte sequence"
    },
    {
      "package": "jet",
      "file": "Core/encoding/yaml.jet",
      "line": 390,
      "column": 5,
      "reason": "inspect compiler-private Bytes and typed carriers; public Dyn variants are unchanged"
    },
    {
      "package": "jet",
      "file": "Core/files/files.jet",
      "line": 446,
      "column": 1,
      "reason": "the compiler store passes a live FileReader and Directory, with one NUL-free plain destination name; no source pathname is reopened"
    },
    {
      "package": "jet",
      "file": "Core/files/files.jet",
      "line": 452,
      "column": 1,
      "reason": "compiler store owns the explicit child descriptor slot and retains the held Directory until the process leaf duplicates its descriptor"
    },
    {
      "package": "jet",
      "file": "Core/http/server.jet",
      "line": 76,
      "column": 5,
      "reason": "move the normalized response head and one-shot body into the transport carrier"
    },
    {
      "package": "jet",
      "file": "Core/http/server.jet",
      "line": 139,
      "column": 5,
      "reason": "borrow the request's stored optional route-template metadata; no route selection or formatting policy"
    },
    {
      "package": "jet",
      "file": "Core/jobs/jobs.jet",
      "line": 130,
      "column": 5,
      "reason": "queue preflight reads final-component metadata without following a symlink; metadata failures are ignored exactly as the SQLite provider does"
    },
    {
      "package": "jet",
      "file": "Core/jobs/jobs.jet",
      "line": 135,
      "column": 5,
      "reason": "create the lexical parent with the existing raw recursive mkdir kernel before SQLite open"
    },
    {
      "package": "jet",
      "file": "Core/jobs/jobs.jet",
      "line": 394,
      "column": 13,
      "reason": "match the queue's existing raw wall-clock wait without advancing controlled scheduler time"
    },
    {
      "package": "jet",
      "file": "Core/jobs/jobs.jet",
      "line": 488,
      "column": 5,
      "reason": "publish already committed queue observations through the existing validated Devtools event/sink boundary"
    },
    {
      "package": "jet",
      "file": "Core/jobs/jobs.jet",
      "line": 503,
      "column": 5,
      "reason": "publish source-projected queue status through the existing validated Devtools event/sink boundary"
    },
    {
      "package": "jet",
      "file": "Core/mem/mem.jet",
      "line": 34,
      "column": 1,
      "reason": "`addr` is the address of a live, aligned `T` that stays valid for every use of the returned pointer"
    },
    {
      "package": "jet",
      "file": "Core/mem/mem.jet",
      "line": 38,
      "column": 1,
      "reason": "`pointer` names a live, aligned, readable `T` for the whole access"
    },
    {
      "package": "jet",
      "file": "Core/mem/mem.jet",
      "line": 42,
      "column": 1,
      "reason": "`pointer` names a live, aligned, writable `T` for the whole access"
    },
    {
      "package": "jet",
      "file": "Core/net/net.jet",
      "line": 137,
      "column": 5,
      "reason": "checked IPv4/IPv6 octets transferred into the native address carrier"
    },
    {
      "package": "jet",
      "file": "Core/net/net.jet",
      "line": 143,
      "column": 5,
      "reason": "read native address octets; no parsing or formatting policy in the carrier leaf"
    },
    {
      "package": "jet",
      "file": "Core/net/net.jet",
      "line": 149,
      "column": 5,
      "reason": "read the native IP address discriminant"
    },
    {
      "package": "jet",
      "file": "Core/net/net.jet",
      "line": 192,
      "column": 5,
      "reason": "validated IP octets, port and IPv6 numeric scope transferred into the socket carrier"
    },
    {
      "package": "jet",
      "file": "Core/net/net.jet",
      "line": 199,
      "column": 5,
      "reason": "read socket IP representation; formatting stays in Jet"
    },
    {
      "package": "jet",
      "file": "Core/net/net.jet",
      "line": 202,
      "column": 5,
      "reason": "read native socket port"
    },
    {
      "package": "jet",
      "file": "Core/net/net.jet",
      "line": 214,
      "column": 5,
      "reason": "read native IPv6 socket scope identifier"
    },
    {
      "package": "jet",
      "file": "Core/net/net.jet",
      "line": 413,
      "column": 5,
      "reason": "read the native UDP packet's socket-address carrier"
    },
    {
      "package": "jet",
      "file": "Core/process/process.jet",
      "line": 96,
      "column": 5,
      "reason": "transfer a fully initialized Jet process plan into the temporary native ProcessSpec carrier; paired environment columns have equal length"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 154,
      "column": 5,
      "reason": "one existing registry read snapshots exact and candidate facts; no second authority or resource carrier"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 264,
      "column": 5,
      "reason": "read the existing endpoint's tree, worker and generation fields"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 271,
      "column": 5,
      "reason": "Services3081: check started before the single delivery-field write"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 278,
      "column": 5,
      "reason": "Services1415: root policy write precedes each declared group policy write"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 287,
      "column": 5,
      "reason": "Services3220: tree's exclusive write borrow covers count then clear"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 296,
      "column": 5,
      "reason": "Services3231: started and record checks precede adapter then authority writes"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 316,
      "column": 5,
      "reason": "the source exclusive tree borrow covers the same complete mutation as Rust &mut; no lock or interleaving window"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 347,
      "column": 5,
      "reason": "Services4437: authority lookup, durable load, first record, then checked framing"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 361,
      "column": 5,
      "reason": "Services4493: replay existing in-memory records without issuing a store read"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 396,
      "column": 5,
      "reason": "Services6729: copy the existing optional last-upgrade field"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 402,
      "column": 5,
      "reason": "Services1153: plain field projection; rollback store remains omitted from all text views"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 410,
      "column": 5,
      "reason": "Services1265: read the existing topology fields, preserve handler insertion order"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 428,
      "column": 5,
      "reason": "Services5867: durable replay before lookup; first matching run wins"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 443,
      "column": 5,
      "reason": "Services5879: durable replay before lookup; missing or nonterminal run is Policy"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 460,
      "column": 5,
      "reason": "Services6978: project existing runtime facts; source owns counts and text"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 541,
      "column": 5,
      "reason": "Allocation only: source supplies every Services.rs initializer field at 2d9953adf:1293-1319, in native evaluation order"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 616,
      "column": 5,
      "reason": "read the final path component's OS metadata kind without opening an identity handle"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 642,
      "column": 5,
      "reason": "read the checked service tree's generation field without mutating or copying its scheduler carrier"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 648,
      "column": 5,
      "reason": "read event-log storage length on the checked service tree"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 654,
      "column": 5,
      "reason": "read dead-letter storage length on the checked service tree"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 856,
      "column": 5,
      "reason": "Services5439-5507: synchronize before validation; exact version replay precedes active-version rejection"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 893,
      "column": 5,
      "reason": "Services5656-5705: exclusive tree borrow keeps replay and append cursor decisions indivisible"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 924,
      "column": 5,
      "reason": "Services5708-5789: first recorded definition wins; replay precedes terminal checks"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 977,
      "column": 5,
      "reason": "Services5792-5867: replay consumes recorded attempts, never derives another attempt from replayed state"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1012,
      "column": 5,
      "reason": "Services5870-5950: two replay entries advance together; identical terminal completions are idempotent"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1188,
      "column": 5,
      "reason": "Services1546: exclusive tree write borrow preserves validation, reparent, append ordering"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1234,
      "column": 5,
      "reason": "Services7044: counter write precedes reachable-worker selection and canonical failure policy"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1250,
      "column": 5,
      "reason": "Services4575: source owns adapter/authority/payload/count checks and record framing; store append before field push"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1270,
      "column": 5,
      "reason": "Services4525: host record-write leaf retains existing mutex through store write and field assignment; no guard exposed"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1302,
      "column": 5,
      "reason": "Services2184: identity and pinned-generation checks precede authority validation"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1328,
      "column": 5,
      "reason": "Services6081: exact directory lookup, generation, proof, authority, running, draining order"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1352,
      "column": 5,
      "reason": "Services2558: source owns routing, message checks, worker gate, capacity error text, post-send ordering"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1451,
      "column": 5,
      "reason": "ServiceAuthority2504: one existing locked durable read, then the owned entry's state"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1466,
      "column": 5,
      "reason": "ServiceAuthority2517: same locked read; receipt fields and signature are source policy"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1480,
      "column": 5,
      "reason": "ServiceAuthority2535-2647: one locked read, then exact record filtering, error order and signature validation"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1529,
      "column": 5,
      "reason": "ServiceAuthority2650: validate/read first, then an exact zero-retention runtime and existing retry operation"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1540,
      "column": 5,
      "reason": "ServiceAuthority1297: identity checks, existing two registry reads/captured Arc, conditional depth check, one receive"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1629,
      "column": 5,
      "reason": "Services2864: local queued-message/idempotency matching stays source; existing cryptographic ID leaf"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1649,
      "column": 5,
      "reason": "Services3127: no enclosing lock; source owns local deduplication/admission/rollback orchestration"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1722,
      "column": 5,
      "reason": "Services2629: no enclosing guard; existing atomic authority operations retain their locks"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1814,
      "column": 5,
      "reason": "Services2693-2762: retain authority requeue then mailbox-restore ordering and both failure texts"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 1875,
      "column": 5,
      "reason": "Services1464-1544: checked function item is passed through; source owns validation and every seed"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 2017,
      "column": 5,
      "reason": "Services2917-3078: exclusive tree borrow; each write follows the native order"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 2132,
      "column": 5,
      "reason": "Services3080-3100: pinned validation, group policy, one now, then weak name/tree match"
    },
    {
      "package": "jet",
      "file": "Core/service/service.jet",
      "line": 2153,
      "column": 5,
      "reason": "Services2847-2862: validated endpoint, then deliberately weak name/tree match, channel depth"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 371,
      "column": 1,
      "reason": "posix umask: the caller owns the process file-creation mask and restores it when other code relies on it"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 417,
      "column": 5,
      "reason": "posix getpgid: inspect a process group"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 423,
      "column": 5,
      "reason": "posix getsid: inspect a process session"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 428,
      "column": 1,
      "reason": "posix getpriority: the caller vouches that `which` names this process or one it controls"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 435,
      "column": 1,
      "reason": "posix setpriority: the caller owns the scheduling priority of the process it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 439,
      "column": 1,
      "reason": "posix utime: the caller owns the file whose timestamps it rewrites"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 446,
      "column": 1,
      "reason": "posix stop: the caller accepts that the process ends here after the shared cleanup boundary"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 456,
      "column": 1,
      "reason": "posix pipe: the caller owns both returned descriptors and closes each exactly once"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 462,
      "column": 1,
      "reason": "posix close_fd: the caller owns `fd` and nothing uses it after this call"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 470,
      "column": 1,
      "reason": "posix mkfifo: the caller owns `path` and removes the named pipe it creates"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 475,
      "column": 1,
      "reason": "posix fork: the caller handles both the parent and the child return and reaps the child"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 479,
      "column": 1,
      "reason": "posix setsid: the caller owns the process session topology it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 483,
      "column": 1,
      "reason": "posix wait: the caller owns the child processes it reaps"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 489,
      "column": 1,
      "reason": "posix waitpid: the caller owns the child process it reaps"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 495,
      "column": 1,
      "reason": "posix setuid: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 499,
      "column": 1,
      "reason": "posix setgid: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 503,
      "column": 1,
      "reason": "posix setpgrp: the caller owns the process group topology it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 507,
      "column": 1,
      "reason": "posix setpgid: the caller owns the process group topology it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 511,
      "column": 1,
      "reason": "posix kill: the caller owns or is entitled to signal the target process"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 515,
      "column": 1,
      "reason": "posix initgroups: the caller changes supplementary groups deliberately"
    },
    {
      "package": "jet",
      "file": "Core/testing/testing.jet",
      "line": 354,
      "column": 5,
      "reason": "the caller bounded this byte position by the String storage length"
    },
    {
      "package": "jet",
      "file": "Core/testing/testing.jet",
      "line": 375,
      "column": 5,
      "reason": "all appended bytes came from complete UTF-8 strings or ASCII"
    },
    {
      "package": "jet",
      "file": "Core/tui/tui.jet",
      "line": 101,
      "column": 5,
      "reason": "Core TUI reads owned encoded OS environment bytes from the current host; no UTF-8 assumption"
    },
    {
      "package": "jet",
      "file": "Core/tui/tui.jet",
      "line": 127,
      "column": 5,
      "reason": "Core TUI samples the same stdout terminal fact consumed by terminal style/progress"
    },
    {
      "package": "jet",
      "file": "Core/tui/tui.jet",
      "line": 653,
      "column": 5,
      "reason": "Core TUI test: normalized style words and explicit absent UI slots"
    },
    {
      "package": "jet",
      "file": "Core/ui/host.jet",
      "line": 154,
      "column": 5,
      "reason": "Core UI capability facts come from the current retained provider, not a fabricated default"
    },
    {
      "package": "jet",
      "file": "Core/ui/host.jet",
      "line": 160,
      "column": 5,
      "reason": "Core UI dialog crosses the current provider boundary; its typed cancellation and denial are preserved"
    },
    {
      "package": "jet",
      "file": "Core/ui/host.jet",
      "line": 166,
      "column": 5,
      "reason": "Core UI save dialog crosses the current provider boundary with the original typed request"
    },
    {
      "package": "jet",
      "file": "Core/ui/host.jet",
      "line": 180,
      "column": 5,
      "reason": "Core UI dialog request transfer: validated grant and explicit absent directory, filter list, and multiple-selection slot"
    },
    {
      "package": "jet",
      "file": "Core/ui/host/accessibility.jet",
      "line": 7,
      "column": 5,
      "reason": "Core accessibility attaches metadata to the canonical node through the current retained provider"
    },
    {
      "package": "jet",
      "file": "Core/ui/host/accessibility.jet",
      "line": 16,
      "column": 5,
      "reason": "Core accessibility projects metadata from the canonical node, never a second provider tree"
    },
    {
      "package": "jet",
      "file": "Core/ui/host/clipboard.jet",
      "line": 6,
      "column": 5,
      "reason": "Core clipboard reads through the current retained provider; the raw host owns platform access"
    },
    {
      "package": "jet",
      "file": "Core/ui/host/clipboard.jet",
      "line": 12,
      "column": 5,
      "reason": "Core clipboard transfers the exact UTF-8 text to the current retained provider"
    },
    {
      "package": "jet",
      "file": "Core/ui/host/drag_drop.jet",
      "line": 33,
      "column": 5,
      "reason": "Core drag/drop consumes one typed event from the current retained provider queue"
    },
    {
      "package": "jet",
      "file": "Core/ui/host/ime.jet",
      "line": 25,
      "column": 5,
      "reason": "Core IME consumes one typed event from the current retained provider queue"
    },
    {
      "package": "jet",
      "file": "Core/ui/host/shortcuts.jet",
      "line": 11,
      "column": 5,
      "reason": "Core shortcut registration uses the current retained provider's canonical action registry"
    },
    {
      "package": "jet",
      "file": "Core/ui/host/shortcuts.jet",
      "line": 17,
      "column": 5,
      "reason": "Core shortcut dispatch uses the current retained provider's canonical action registry"
    },
    {
      "package": "jet",
      "file": "Core/ui/ui.jet",
      "line": 126,
      "column": 5,
      "reason": "Core creates the real retained GTK renderer through the registered host allocation boundary"
    },
    {
      "package": "jet",
      "file": "Core/ui/ui.jet",
      "line": 131,
      "column": 5,
      "reason": "Core creates the real retained terminal renderer through the registered host allocation boundary"
    },
    {
      "package": "jet",
      "file": "Core/ui/ui.jet",
      "line": 136,
      "column": 5,
      "reason": "Core creates the real retained null renderer through the registered host allocation boundary"
    },
    {
      "package": "jet",
      "file": "Core/ui/ui.jet",
      "line": 147,
      "column": 5,
      "reason": "Core UI accessibility slot transfer: explicitly present validated metadata"
    },
    {
      "package": "jet",
      "file": "Core/ui/ui.jet",
      "line": 164,
      "column": 5,
      "reason": "Core UI shortcut slot transfer: explicitly present validated shortcut"
    },
    {
      "package": "jet",
      "file": "Core/ui/ui.jet",
      "line": 191,
      "column": 5,
      "reason": "Core UI host slot transfer: audited kind, role, explicit absent slots, and normalized style words"
    },
    {
      "package": "jet",
      "file": "Core/ui/ui.jet",
      "line": 197,
      "column": 5,
      "reason": "Core UI viewport transfer: source supplies a valid device and bounded positive dimensions"
    },
    {
      "package": "jet",
      "file": "Core/vendor/sqlite3/fixture/run.jet",
      "line": 17,
      "column": 5,
      "reason": "exclusive out-slots hold SQLite pointers; zero words mean null, SQL is NUL-terminated for each borrowed call, and each live pointer is finalized/closed once"
    },
    {
      "package": "jet",
      "file": "Jetpack/Foundation/Source/TreeHash.jet",
      "line": 724,
      "column": 5,
      "reason": "source-store linkat uses the live held reader and directory, a validated plain name, and no source pathname; raw syscall facts are classified here"
    },
    {
      "package": "jet",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 58,
      "column": 5,
      "reason": "export a fresh Core signing seed only to descriptor-relative owner-only key publication; the Core key owner is wiped on every outcome"
    },
    {
      "package": "jet",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 113,
      "column": 5,
      "reason": "import the protected seed into Core signing without a second raw-secret copy; close the temporary Core Secret on success and failure"
    },
    {
      "package": "jet",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 137,
      "column": 5,
      "reason": "package author signatures require Core's strict Ed25519 verification; no raw verifier or helper process"
    },
    {
      "package": "jet",
      "file": "Jetpack/Publication/Tests/Index.jet",
      "line": 266,
      "column": 5,
      "reason": "the test owns the planted lock file it ages"
    },
    {
      "package": "jet",
      "file": "Jetpack/Publication/Tests/Sign.jet",
      "line": 164,
      "column": 5,
      "reason": "original signing secret-erasure acceptance requires the Core volatile in-place wipe provider (#3597)"
    },
    {
      "package": "jet",
      "file": "Jetpack/Store/Tests/ClosureGraph.jet",
      "line": 59,
      "column": 13,
      "reason": "the test owns the journal file it just wrote"
    },
    {
      "package": "jet",
      "file": "Jetpack/Store/Tests/Seal.jet",
      "line": 31,
      "column": 5,
      "reason": "the store test owns the fixture file whose mtime it pins"
    },
    {
      "package": "jet",
      "file": "Tools/agent-eval/domain-foundations/area-embedded/pkg/mmio_fixed.jet",
      "line": 4,
      "column": 5,
      "reason": "board.sensor_v1 GPIO register 0x40000100 is a volatile MMIO word"
    },
    {
      "package": "jet",
      "file": "Tools/agent-eval/domain-foundations/area-embedded/pkg/run.jet",
      "line": 111,
      "column": 5,
      "reason": "host replay uses a stack cell as a stand-in for the board register"
    },
    {
      "package": "jet",
      "file": "Tools/testkit/StepsControl.jet",
      "line": 134,
      "column": 5,
      "reason": "kill(pid, 0) observes the case-owned child without sending a signal"
    },
    {
      "package": "jet",
      "file": "Tools/testkit/StepsSessions.jet",
      "line": 116,
      "column": 5,
      "reason": "the case explicitly authorizes signalling its own spawned child"
    },
    {
      "package": "jetpack",
      "file": "crates/jetpack/batteries/embedded/embedded.jet",
      "line": 181,
      "column": 9,
      "reason": "host replay uses a stack cell for the CMSDK UART0 control register and never dereferences the target address"
    },
    {
      "package": "tests",
      "file": "tests/cases/cffi_native_matrix/fixtures/required_native_c_abi_matrix/default/matrix.jet",
      "line": 44,
      "column": 5,
      "reason": "live non-null out slot; read only after status zero"
    },
    {
      "package": "tests",
      "file": "tests/cases/cffi_native_matrix/fixtures/required_native_c_abi_matrix/sysv64/matrix.jet",
      "line": 44,
      "column": 5,
      "reason": "live non-null out slot; read only after status zero"
    },
    {
      "package": "tests",
      "file": "tests/cases/cli_parts_core/fixtures/gate_policy/main.jet",
      "line": 2,
      "column": 5,
      "reason": "org refusal"
    },
    {
      "package": "tests",
      "file": "tests/cases/cli_parts_inspect/fixtures/foreign/app/main.jet",
      "line": 3,
      "column": 5,
      "reason": "calls the audited dependency"
    },
    {
      "package": "tests",
      "file": "tests/cases/cli_parts_inspect/fixtures/foreign/dep/dep.jet",
      "line": 2,
      "column": 1,
      "reason": "intentionally invalid address"
    },
    {
      "package": "tests",
      "file": "tests/cases/cli_parts_inspect/fixtures/release/hardened/main.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "tests",
      "file": "tests/cases/cli_parts_inspect/fixtures/release/release/main.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "tests",
      "file": "tests/cases/cli_parts_inspect/fixtures/team/unsafe/main.jet",
      "line": 2,
      "column": 5,
      "reason": "audit"
    },
    {
      "package": "tests",
      "file": "tests/cases/cli_parts_inspect/fixtures/test_hardened/hardened/main.jet",
      "line": 4,
      "column": 5,
      "reason": "test deliberately reads after arena reset"
    },
    {
      "package": "tests",
      "file": "tests/cases/cli_parts_inspect/fixtures/test_hardened/release/main.jet",
      "line": 4,
      "column": 5,
      "reason": "test deliberately reads after arena reset"
    },
    {
      "package": "tests",
      "file": "tests/cases/comptime_cli_parity/fixtures/tier_parity_examples_run_through_aot_jit_and_interpreter/tooling_checked_text_type/checked_text_type.jet",
      "line": 20,
      "column": 5,
      "reason": "checked text example output"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/argon2id_rfc_and_expert_known_answers_kill_argon2i_mutation.fixture/run.jet",
      "line": 37,
      "column": 5,
      "reason": "original public expert Argon2id KAT"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/expert_aes256gcm_matches_nist_cavs_vector.fixture/run.jet",
      "line": 37,
      "column": 5,
      "reason": "original published known-answer vector"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/expert_surface_matches_reference_vectors_and_bounds.fixture/run.jet",
      "line": 37,
      "column": 5,
      "reason": "original expert interoperability vectors"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/expert_xchacha20poly1305_matches_cfrg_vector.fixture/run.jet",
      "line": 37,
      "column": 5,
      "reason": "original published known-answer vector"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/jetc_v1_expert_open_accepts_only_the_pinned_grammar.fixture/run.jet",
      "line": 40,
      "column": 5,
      "reason": "original pinned JETC v1 migration grammar"
    },
    {
      "package": "tests",
      "file": "tests/cases/gate_ledger/fixtures/gates/gates.jet",
      "line": 2,
      "column": 5,
      "reason": "test unsafe reason"
    },
    {
      "package": "tests",
      "file": "tests/cases/output_callable/runnable_contracts_and_selection_fail_in_sema.fixture/probe3/main.jet",
      "line": 2,
      "column": 1,
      "reason": "raw boundary"
    },
    {
      "package": "tests",
      "file": "tests/cases/pin/fixtures/library_wires_self_ref_under_unsafe_and_exposes_safe_pin_api/fixture.jet",
      "line": 10,
      "column": 5,
      "reason": "node storage is fixed for the returned pin; self_addr names this place"
    },
    {
      "package": "tests",
      "file": "tests/cases/policy_scope/unsafe_obligations_are_checked_before_codegen.fixture/run.jet",
      "line": 5,
      "column": 5,
      "reason": "local pointer"
    },
    {
      "package": "tests",
      "file": "tests/cases/single_use/drop_erases_no_unsafe_in_codegen.fixture/source-1.jet",
      "line": 14,
      "column": 5,
      "reason": "gone"
    },
    {
      "package": "tests",
      "file": "tests/cases/single_use/drop_inside_unsafe_block_satisfies_single_use.fixture/source-1.jet",
      "line": 15,
      "column": 5,
      "reason": "the resource is already gone; nothing to release"
    },
    {
      "package": "tests",
      "file": "tests/cases/single_use/drop_inside_unsafe_fn_satisfies_single_use.fixture/source-1.jet",
      "line": 13,
      "column": 1,
      "reason": "voids a freshly-acquired lock whose resource is already gone"
    },
    {
      "package": "tests",
      "file": "tests/cases/single_use/drop_inside_unsafe_fn_satisfies_single_use.fixture/source-1.jet",
      "line": 19,
      "column": 5,
      "reason": "calling the audited voider"
    },
    {
      "package": "tests",
      "file": "tests/cases/single_use/reuse_after_drop_is_e0121.fixture/source-1.jet",
      "line": 15,
      "column": 5,
      "reason": "done with it"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_collections_and_methods/sentry_typed_int_store_keeps_exact_carrier.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "the typed pointer refers to the live local cell for these stores"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_control_and_data/explicit_process_exit_cleanup_golden_matches_all_execution_tiers.fixture/process_exit_cleanup.jet",
      "line": 20,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_custom/fixtures/from_addr_sentry_is_tier_parity/fixture.jet",
      "line": 6,
      "column": 5,
      "reason": "value is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_alignment.jet",
      "line": 4,
      "column": 5,
      "reason": "the tuple storage is live but this raw Int is misaligned"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_expired_recursive_stack.jet",
      "line": 11,
      "column": 5,
      "reason": "the recursive address outlives its owning Jet frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_expired_stack.jet",
      "line": 8,
      "column": 5,
      "reason": "the returned address outlives its owning Jet frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_provenance.jet",
      "line": 6,
      "column": 5,
      "reason": "external address must be a live allocation"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_quarantine.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_custom/fixtures/sentry_source_policy_off_is_tier_parity/fixture.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_place_window_lifetimes/borrowed_window_stays_caller_owned.fixture/main.jet",
      "line": 5,
      "column": 5,
      "reason": "the borrowed value is live in the caller"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_place_window_lifetimes/heap_element_window_stays_allocator_owned.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "the list element remains heap-owned"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_place_window_lifetimes/stack_window_uses_expiring_sentry_address.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "cell is live on this stack frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/ffi_capability_edge_uses_shared_sentry_prelude/main.jet",
      "line": 7,
      "column": 5,
      "reason": "the foreign call may write through the borrowed value"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/paren/main.jet",
      "line": 2,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be a live, valid Int"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/paren/main.jet",
      "line": 10,
      "column": 5,
      "reason": "addr is the address of `cell`, a live Int on this stack frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/plain/main.jet",
      "line": 2,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be a live, valid Int"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/plain/main.jet",
      "line": 10,
      "column": 5,
      "reason": "addr is the address of `cell`, a live Int on this stack frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/sentry_kernel_is_shared_by_aot_and_runtime_adapters/main.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/unsafe_fn_block_and_ptr_ops/main.jet",
      "line": 2,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be a live, valid Int"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/unsafe_fn_block_and_ptr_ops/main.jet",
      "line": 10,
      "column": 5,
      "reason": "addr is the address of `cell`, a live Int on this stack frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/unsafe_tier_emit_is_byte_exact/main.jet",
      "line": 2,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be valid"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/unsafe_tier_emit_is_byte_exact/main.jet",
      "line": 10,
      "column": 5,
      "reason": "safe: cell is live"
    },
    {
      "package": "tests",
      "file": "tests/cases/tir_unsafe_and_runtime/fixtures/volatile_write_emit_is_byte_exact/main.jet",
      "line": 2,
      "column": 1,
      "reason": "UART TX register is mapped by the target machine"
    },
    {
      "package": "tests",
      "file": "tests/cases/web_build/module_local_dev_is_validated_as_web_runtime.fixture/main.jet",
      "line": 4,
      "column": 9,
      "reason": "exercise the web TIR gate"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/aes/nist_gcm.jet",
      "line": 11,
      "column": 5,
      "reason": "published KAT or frozen seed AES-GCM fixture"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/aes/seed_differential.jet",
      "line": 11,
      "column": 5,
      "reason": "published KAT or frozen seed AES-GCM fixture"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 13,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 17,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 30,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 34,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 47,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 51,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 64,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 68,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 81,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 85,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 96,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 107,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 118,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 129,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 140,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 151,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 162,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 173,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 184,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 195,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 206,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 217,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 228,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 239,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 250,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 261,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 272,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 283,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 294,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 305,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 321,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 332,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 348,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 359,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 375,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 386,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 402,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 413,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 429,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 440,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 456,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 467,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 483,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 494,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 510,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 521,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 537,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 548,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 564,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 575,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 591,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 602,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 618,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 629,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 645,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 656,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 672,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 683,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 699,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 710,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 726,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 737,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 753,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 764,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 780,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 791,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 807,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 818,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 834,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 845,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 861,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 872,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 888,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 899,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 915,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 926,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 942,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 953,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 969,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 980,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 996,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1007,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1023,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1034,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1050,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1061,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1077,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1088,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1104,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1115,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1131,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1142,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1158,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1169,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1185,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1196,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1212,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1223,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1239,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1250,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1266,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1277,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1293,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1304,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1320,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1331,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1342,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1353,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1364,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1375,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1386,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1397,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1408,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1424,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1440,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1456,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1474,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1478,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1491,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1495,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1508,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1512,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1525,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1529,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1542,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1546,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1559,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1563,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1576,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1580,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1593,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1597,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1610,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1614,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1627,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1631,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1644,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1648,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1661,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1665,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1678,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1682,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1695,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1699,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1712,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1716,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1729,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1733,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1746,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1750,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1763,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1767,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1780,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1784,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1797,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1801,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1814,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1818,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1831,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1835,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1848,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1852,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1865,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1869,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1882,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1886,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1899,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1903,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1916,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1920,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1933,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1937,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1950,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1954,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1967,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1971,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1984,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1988,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2001,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2005,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2018,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2022,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2035,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2039,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2052,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2056,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2069,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2073,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2086,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2090,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2103,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2107,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2120,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2124,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2137,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2141,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2154,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2158,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2171,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2175,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2188,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2192,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2205,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2209,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2220,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2236,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_x25519.jet",
      "line": 5,
      "column": 5,
      "reason": "published/frozen X25519 test inputs"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/aes256gcm_open.jet",
      "line": 8,
      "column": 5,
      "reason": "fixed NIST AES-256-GCM vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/aes256gcm_seal.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed NIST AES-256-GCM vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/argon2id.jet",
      "line": 8,
      "column": 5,
      "reason": "fixed Argon2id known-answer vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/ed25519_sign.jet",
      "line": 6,
      "column": 5,
      "reason": "fixed RFC 8032 Ed25519 vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/ed25519_verify_strict.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed RFC 8032 Ed25519 vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/hkdf_sha256_raw.jet",
      "line": 8,
      "column": 5,
      "reason": "fixed RFC 5869 HKDF vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/migrate_v1.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed JETC v1 source failure"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/open_v1.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed JETC v1 AES vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/secret_bytes.jet",
      "line": 7,
      "column": 5,
      "reason": "inspect fixed secret bytes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/shared_secret_bytes.jet",
      "line": 9,
      "column": 5,
      "reason": "inspect shared-secret length"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/signing_key_bytes.jet",
      "line": 7,
      "column": 5,
      "reason": "inspect generated signing-key bytes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/x25519_raw.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed RFC 7748 X25519 vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/x25519_secret_bytes.jet",
      "line": 7,
      "column": 5,
      "reason": "inspect generated X25519 secret bytes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/xchacha20poly1305_open.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed CFRG XChaCha20-Poly1305 vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/xchacha20poly1305_seal.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed CFRG XChaCha20-Poly1305 vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/vault/commit_import_signing.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed signing key into the vault"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/vault/commit_import_x25519.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed X25519 secret into the vault"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/vault/prepare_import_signing.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed signing-key vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/vault/prepare_import_x25519.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed X25519 secret vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/x25519.jet",
      "line": 9,
      "column": 5,
      "reason": "inspect shared-secret length"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/mem/volatile_read.jet",
      "line": 6,
      "column": 5,
      "reason": "value is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/mem/volatile_write.jet",
      "line": 6,
      "column": 5,
      "reason": "value is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/atexit.jet",
      "line": 5,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/close_fd.jet",
      "line": 6,
      "column": 9,
      "reason": "close an invalid descriptor without changing process state"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/fork.jet",
      "line": 5,
      "column": 5,
      "reason": "fork and let the child return immediately"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/getpriority.jet",
      "line": 5,
      "column": 5,
      "reason": "read the current process priority"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/initgroups.jet",
      "line": 5,
      "column": 5,
      "reason": "probe initgroups with a stable missing user"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/kill.jet",
      "line": 5,
      "column": 5,
      "reason": "check the process group without sending a signal"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/mkfifo.jet",
      "line": 5,
      "column": 5,
      "reason": "probe FIFO creation with an empty path"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/pipe.jet",
      "line": 5,
      "column": 5,
      "reason": "open a pipe and observe its descriptor pair"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setgid.jet",
      "line": 5,
      "column": 5,
      "reason": "probe setgid with an invalid identifier"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setpgid.jet",
      "line": 5,
      "column": 5,
      "reason": "set the current process into its existing process group"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setpgrp.jet",
      "line": 5,
      "column": 5,
      "reason": "set the current process group"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setpriority.jet",
      "line": 5,
      "column": 5,
      "reason": "lower the current process priority for this short-lived witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setsid.jet",
      "line": 5,
      "column": 5,
      "reason": "create a session when the host permits it"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setuid.jet",
      "line": 5,
      "column": 5,
      "reason": "probe setuid with an invalid identifier"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/stop.jet",
      "line": 6,
      "column": 9,
      "reason": "stop this dedicated witness successfully"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/umask.jet",
      "line": 5,
      "column": 5,
      "reason": "set a deterministic mask for this short-lived witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/utime.jet",
      "line": 5,
      "column": 5,
      "reason": "probe timestamp update with an empty path"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/wait.jet",
      "line": 5,
      "column": 5,
      "reason": "wait for a child that this witness does not have"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/waitpid.jet",
      "line": 5,
      "column": 5,
      "reason": "poll for a child that this witness does not have"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/ffi_stack4_i64.jet",
      "line": 2,
      "column": 1,
      "reason": "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/ffi_stack4_i64.jet",
      "line": 49,
      "column": 5,
      "reason": "Linux getpid syscall 39 ignores all argument registers and retains no stack pointer"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/fmt_import_c_bindings.jet",
      "line": 7,
      "column": 1,
      "reason": "audited C boundary"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/fmt_import_c_bindings.jet",
      "line": 10,
      "column": 1,
      "reason": "audited C boundary"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/mir_explicit_cleanup_order.jet",
      "line": 31,
      "column": 5,
      "reason": "exercise the explicit process-stop cleanup boundary"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/unsafe_obligations/run.jet",
      "line": 5,
      "column": 5,
      "reason": "local pointer"
    },
    {
      "package": "tests",
      "file": "tests/fuzz/sema/differential/ex_crypto_crypto_migration.jet",
      "line": 26,
      "column": 5,
      "reason": "AES-256-GCM protocol interoperability"
    },
    {
      "package": "tests",
      "file": "tests/fuzz/sema/valid/ex_lowlevel_lowlevel.jet",
      "line": 10,
      "column": 5,
      "reason": "`cell` is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/fuzz/sema/valid/ex_lowlevel_pointer_cast_deref.jet",
      "line": 11,
      "column": 5,
      "reason": "flag is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/run/core_crypto_foundations/crypto_aead_tampering_is_rejected_across_tiers.jet",
      "line": 16,
      "column": 5,
      "reason": "fixed AEAD tamper vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/audited_gate_ladder_forbidden/run.jet",
      "line": 3,
      "column": 5,
      "reason": "the organization policy refuses this audited escape"
    },
    {
      "package": "tests",
      "file": "tests/ui/cffi_out_pointer_requires_unsafe.jet",
      "line": 12,
      "column": 1,
      "reason": "the local slot remains live for the complete C call"
    },
    {
      "package": "tests",
      "file": "tests/ui/checked_text_raw_requires_unsafe.jet",
      "line": 22,
      "column": 5,
      "reason": "audited literal"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed password-hash vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy_type_precedence.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed password-hash vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy_valid.jet",
      "line": 7,
      "column": 5,
      "reason": "protocol-selected password policy"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy_valid.jet",
      "line": 15,
      "column": 5,
      "reason": "fixed password-hash vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_material_length.jet",
      "line": 12,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_material_length_type_precedence.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_material_length_valid.jet",
      "line": 5,
      "column": 5,
      "reason": "protocol-selected interop material"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_material_length_valid.jet",
      "line": 17,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_callback_bound_precedence.jet",
      "line": 13,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_correct.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_effect_bound_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_effect_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_effect_prohibition_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_effect_unknown_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_length.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_region_authority_precedence.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_trait_dispatch_precedence.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_type_precedence.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_hkdf_output_length.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_hkdf_output_type_precedence.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_hkdf_output_valid.jet",
      "line": 6,
      "column": 5,
      "reason": "protocol-selected output length"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_hkdf_output_valid.jet",
      "line": 14,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_duplicate_clobber.jet",
      "line": 3,
      "column": 1,
      "reason": "each discarded register is named once"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_float_signature.jet",
      "line": 3,
      "column": 1,
      "reason": "register-only increment"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_missing_return_anchor.jet",
      "line": 3,
      "column": 1,
      "reason": "cycle counter"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_no_mem.jet",
      "line": 1,
      "column": 1,
      "reason": "register-only increment"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_return_clobber.jet",
      "line": 3,
      "column": 1,
      "reason": "the result register is overwritten by the body"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_unknown_clobber.jet",
      "line": 3,
      "column": 1,
      "reason": "the body declares only audited registers"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_body_not_string.jet",
      "line": 1,
      "column": 1,
      "reason": "demo"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_inline_c_mismatch.jet",
      "line": 1,
      "column": 1,
      "reason": "deliberate mismatch fixture"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_unknown_language.jet",
      "line": 1,
      "column": 1,
      "reason": "demo"
    },
    {
      "package": "tests",
      "file": "tests/ui/gates_load_package_ceiling/run.jet",
      "line": 2,
      "column": 5,
      "reason": "audited load-only gate"
    },
    {
      "package": "tests",
      "file": "tests/ui/lowlevel_e3102.jet",
      "line": 2,
      "column": 5,
      "reason": "addr is valid"
    },
    {
      "package": "tests",
      "file": "tests/ui/lowlevel_e3103.jet",
      "line": 3,
      "column": 1,
      "reason": "reads through caller-provided raw pointer"
    },
    {
      "package": "tests",
      "file": "tests/ui/lowlevel_volatile_write_without_mem.jet",
      "line": 2,
      "column": 5,
      "reason": "addr is valid"
    },
    {
      "package": "tests",
      "file": "tests/ui/lowlevel_volatile_write_wrong_ptr.jet",
      "line": 4,
      "column": 5,
      "reason": "plain ints are not raw pointers"
    },
    {
      "package": "tests",
      "file": "tests/ui/lowlevel_volatile_write_wrong_value.jet",
      "line": 6,
      "column": 5,
      "reason": "slot is live and pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/ui/marker_group_forbidden.jet",
      "line": 2,
      "column": 21,
      "reason": "audit"
    },
    {
      "package": "tests",
      "file": "tests/ui/secrets_expiry/expiring_secret_loan_rejects_move_variadic_and_drop_paths_3.jet",
      "line": 11,
      "column": 9,
      "reason": "attempt to discard a loan"
    },
    {
      "package": "tests",
      "file": "tests/ui/unsafe_forbidden/run.jet",
      "line": 2,
      "column": 5,
      "reason": "not allowed"
    },
    {
      "package": "tests",
      "file": "tests/ui/unsafe_obligation_missing.jet",
      "line": 5,
      "column": 5,
      "reason": "local pointer"
    },
    {
      "package": "tests",
      "file": "tests/ui/unsafe_obligation_no_bleed.jet",
      "line": 6,
      "column": 5,
      "reason": "two independent pointer operations"
    },
    {
      "package": "tests",
      "file": "tests/ui/unsafe_per_site/run.jet",
      "line": 2,
      "column": 5,
      "reason": "selection required"
    }
  ]
}
-->

### Counts

| crate/package | regions |
| --- | ---: |
| Examples | 38 |
| jet | 589 |
| jetpack | 1 |
| tests | 355 |
| **total** | **983** |

### Regions

| crate/package | file | line | reason |
| --- | --- | ---: | --- |
| Examples | Examples/features/crypto/crypto_migration.jet | 31:5 | "AES-256-GCM protocol interoperability" |
| Examples | Examples/features/crypto/crypto_suite.jet | 36:5 | "known-answer check reads the RFC 7748 shared secret" |
| Examples | Examples/features/crypto/expert_projection.jet | 30:5 | "expert cryptography is an explicit opt-in" |
| Examples | Examples/features/crypto/primitive_matrix.jet | 96:5 | "known-answer check reads the RFC 7748 shared secret" |
| Examples | Examples/features/crypto/primitive_matrix.jet | 158:5 | "expert known-answer vectors need raw key bytes" |
| Examples | Examples/features/crypto/random_api_split.jet | 33:5 | "compare the typed and raw HKDF rungs" |
| Examples | Examples/features/crypto/vault_keys.jet | 41:5 | "restore audited raw signing-key material" |
| Examples | Examples/features/effects/audited_gate_ladder.jet | 6:5 | "the audited block has no low-level operation" |
| Examples | Examples/features/effects/single_use_discard.jet | 35:5 | "event cancelled; the ticket admits to nothing, so voiding it is correct" |
| Examples | Examples/features/io/cleanup_matrix.jet | 33:5 | "the stop runs the shared cleanup boundary" |
| Examples | Examples/features/io/cleanup_matrix.jet | 78:5 | "the exit callbacks have no captured state" |
| Examples | Examples/features/io/os_process_control.jet | 30:5 | "POSIX process and pipe control for the core.sys surface" |
| Examples | Examples/features/io/os_stop_cleanup.jet | 24:5 | "the stop callback uses the shared cleanup boundary" |
| Examples | Examples/features/io/process_exit_cleanup.jet | 20:5 | "the exit callback has no captured state" |
| Examples | Examples/features/io/system_privileged_entry.jet | 10:5 | "run only inside a disposable user namespace with a dedicated test identity" |
| Examples | Examples/features/io/system_projection.jet | 14:5 | "the clock file was created above in this test-owned directory" |
| Examples | Examples/features/io/system_projection.jet | 20:5 | "fork and reap both children before returning" |
| Examples | Examples/features/lowlevel/inline_asm.jet | 8:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Examples | Examples/features/lowlevel/inline_asm.jet | 19:5 | "call the audited register-only assembly contract" |
| Examples | Examples/features/lowlevel/inline_c.jet | 6:1 | "the scalar ABI contract matches the C definition" |
| Examples | Examples/features/lowlevel/inline_c.jet | 13:1 | "the scalar ABI contract deliberately returns a mismatched result" |
| Examples | Examples/features/lowlevel/inline_c.jet | 19:5 | "call the audited inline C contract" |
| Examples | Examples/features/lowlevel/inline_c.jet | 26:9 | "call the audited inline C contract with bounded Int values" |
| Examples | Examples/features/lowlevel/inline_c.jet | 32:9 | "call the audited inline C contract with a deliberately wrong C result" |
| Examples | Examples/features/lowlevel/lowlevel.jet | 8:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| Examples | Examples/features/lowlevel/mmio_board_write.jet | 9:5 | "stand-in MMIO cell stays on this stack frame" |
| Examples | Examples/features/lowlevel/opaque_handles/run.jet | 34:5 | "fixed headers define handle close ownership, exclusive buffers, and bounded output lengths" |
| Examples | Examples/features/lowlevel/pointer_cast_deref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
| Examples | Examples/features/lowlevel/unsafe_obligations.jet | 9:5 | "cell stays live and the pointer remains local" |
| Examples | Examples/features/memory/expiring_secret.jet | 20:5 | "vault key import: expiring secret lifetime" |
| Examples | Examples/features/memory/pin.jet | 43:5 | "node storage is fixed for the returned pin; self_addr names this place" |
| Examples | Examples/features/memory/rawptr.jet | 7:5 | "`cell` is a live Int on this stack frame; the pointer never escapes" |
| Examples | Examples/features/memory/uninit.jet | 12:5 | "uninitialized plain-data storage is filled before every read" |
| Examples | Examples/features/memory/unsafe_sentries.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| Examples | Examples/features/memory/unsafe_sentries_package_off/run.jet | 7:5 | "the local cell is live for this package-policy proof" |
| Examples | Examples/features/memory/unsafe_sentries_provenance.jet | 6:5 | "external address must be a live allocation" |
| Examples | Examples/features/memory/unsafe_sentries_source_off.jet | 7:5 | "the local cell is live for this raw read" |
| Examples | Examples/features/tooling/checked_text_type.jet | 20:5 | "checked text example output" |
| jet | Compiler/JetBackend/Source/Image/Calls.jet | 74:9 | "each argument cell is inside this call's writable mapping" |
| jet | Compiler/JetBackend/Source/Image/Calls.jet | 79:5 | "loaded.entry is the selector-generated pointer-to-cells adapter; all cells remain mapped through its return" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 48:5 | "function is the resolved (ptr,len)-&gt;word runtime constructor; buffer remains readable until it returns" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 117:5 | "word is a live String handle and both functions borrow it through the canonical runtime C ABI" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 124:9 | "the runtime's borrowed UTF-8 projection holds length readable bytes until the owning String is dropped" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 144:13 | "word is a live exact-Int carrier and stringify returns a fresh owning String" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 148:13 | "the temporary String is owned here and decoding has copied its bytes" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 186:5 | "word transfers one live String owner to the runtime's canonical consuming drop" |
| jet | Compiler/JetBackend/Source/Image/CoreSharedParent.jet | 264:5 | "the admitted canonical control surrounds the producer's checked no-argument System V getter and its owned Shared result; the Core image remains process-pinned" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 165:9 | "the slot is an aligned word inside the writable mapping, checked above" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 171:9 | "the relocation is an aligned word inside the writable mapping, checked above" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 191:5 | "the admitted control uses begin(fuel,depth,session), status(), end(); startup is the producer's checked no-argument Unit static-fixup boundary" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 212:5 | "`main` is the runtime's `int32_t jet_rt_main(void (*)(void))` and `entry` is the image's checked no-argument entry" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 234:5 | "the slot is an aligned word inside the mapping, now writable" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 247:9 | "this loaded image's checked zero-argument Unit finalizer remains executable; no native owners or returning frames survive" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 180:5 | "state is the compiler-process-lifetime fourteen-word execution resource" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 184:5 | "state is the compiler-process-lifetime fourteen-word execution resource" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 207:5 | "terminated paths and admitted identity and five writable resource words remain live through posix_spawn" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 232:5 | "the raw host leaf returns the compiler-process-lifetime resource words; requests serialize access" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 238:5 | "logical identities share the process-owned execution counter and mutex" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 241:5 | "release the private execution mutex acquired above" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 252:5 | "wait writes only the owned status word, after this child's socket was closed" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 262:5 | "close is serialized with every request on this process-owned host" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 267:5 | "release the private execution mutex acquired above" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 272:5 | "logical close serializes with turns and shared-child teardown" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 274:5 | "release the execution mutex acquired for this logical close" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 318:5 | "all compiler threads share this one process-owned execution resource" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 320:5 | "release the private execution mutex acquired above" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 492:5 | "this is the canonical runtime's checked single-word C ABI" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 696:13 | "each initialized argument is inside this request's mapped cells" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 702:9 | "execution_begin takes three u64 words: fuel, depth, is_comptime" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 707:9 | "entry is the selector-generated pointer-to-cells adapter for this checked request; images and runtime stay pinned" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 717:17 | "successful result storage belongs to this exact pinned image's generated typed release glue" |
| jet | Compiler/JetBackend/Source/Image/SessionHost.jet | 723:21 | "the successful checked adapter initialized its result cell" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 39:1 | "a raw Linux x86-64 system call; the caller passes the call number and arguments that call expects" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 85:5 | "the caller owns `count` writable bytes at `address`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 92:5 | "the caller owns `count` readable bytes at `address`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 100:5 | "an anonymous mapping aliases no existing memory" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 107:5 | "the caller owns the mapping it changes" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 113:5 | "the caller owns the mapping and nothing uses it any more" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 120:5 | "exit_group ends every thread of the process" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 125:1 | "stores one word at an address the caller owns and has mapped writable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 132:1 | "reads one word at an address the caller owns and has mapped readable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 139:1 | "reads one byte at an address the caller owns and has mapped readable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 146:1 | "stores one byte at an address the caller owns and has mapped writable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 175:9 | "the caller owns the destination words" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 185:1 | "calls machine code; the caller vouches for the code and its System V signature" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 198:1 | "calls machine code; the caller vouches for the code and its two-word System V signature" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 213:1 | "calls machine code; the caller holds the mapping and its checked zero-argument Unit System V signature" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 223:1 | "calls machine code; the caller holds the mapping and its checked three-word Unit System V signature" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 240:1 | "calls machine code; the caller holds the mapping and its checked one-word Unit System V signature" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 252:1 | "calls machine code; the caller holds the mapping and its checked two-word Unit System V signature" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 267:1 | "dlopen reads the NUL-terminated path at `path`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 275:1 | "dlsym reads the NUL-terminated name at `name`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 298:5 | "`name` is a NUL-terminated copy of `path`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 310:5 | "`name` is a NUL-terminated copy of `symbol`; dlsym only reads the library's symbol table" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 319:1 | "a raw Linux x86-64 syscall; the caller supplies the syscall's valid machine-word arguments" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 354:1 | "reads one aligned machine word in live allocator-owned mapped memory" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 361:1 | "stores one aligned machine word in live allocator-owned mapped memory; x86 TSO makes this a release store" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 370:1 | "atomically compares and exchanges one aligned live machine word; every non-atomic access to protected data holds its lock" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 385:1 | "x86 pause only hints that this thread is waiting in a spin loop" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 392:1 | "reads one byte from a live readable mapped address supplied by the caller" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 399:1 | "writes one byte to a live writable mapped address supplied by the caller" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 413:1 | "writes at most eight bytes from a stack-local word while that word is live; the balanced pushes and pops do not expose stack storage after return" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 438:1 | "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 485:1 | "the calling thread is attached and GS points at its live Jet TCB" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 494:1 | "ARCH_GET_GS writes one initialized aligned stack word only during this syscall; no TLS or heap is read, and the push/pop is balanced" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 509:1 | "atomically adds to one aligned live word and returns its previous value" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 522:1 | "atomically exchanges one aligned live word; xchg has an implicit full barrier" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 535:1 | "calls executable System V code with one raw word argument" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 548:1 | "calls a Jet closure's code with the live closure header and one raw word argument" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 566:1 | "raw clone child runs only Jet/syscall code, never Rust or musl; supplied stack contains entry and argument" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 607:1 | "calls checked System V Unit code; the caller lends one live raw machine-word argument" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 619:1 | "calls checked System V Unit code; the caller vouches for both raw machine-word arguments" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 633:1 | "calls checked System V Unit code; the caller vouches for all three raw machine-word arguments" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 649:1 | "calls a checked native runtime callback with one machine-word argument" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 662:1 | "calls a checked native runtime callback with two machine-word arguments" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 677:1 | "calls a checked native runtime callback with three machine-word arguments" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 694:1 | "calls a checked native runtime callback with four machine-word arguments" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 716:1 | "source and directory are live owned descriptors, name is terminated; linkat borrows a stack-local empty path and retains no pointer" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 738:1 | "ioctl reads one stack-local machine word with the supplied audited request and live fd; it retains no pointer and all pushes are balanced" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 762:1 | "object is a live fd for fstat or a terminated path for stat/lstat; output lends 128 writable bytes; the initialized stack stat frame never escapes" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 851:1 | "the audited caller lends the stat scalar row and valid syscall object" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 853:5 | "packed Linux 32-bit fields become independent portable machine words without any allocation" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 865:1 | "calls a checked System V void(void) process entry that remains executable for the entire call" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 876:1 | "directory and path identify a source-approved statx object; output lends three writable words; the zeroed 256-byte statx stack buffer never escapes" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 974:5 | "address names an initialized canonical native word owned by the pinned request or retained slot" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 981:5 | "address names an initialized canonical native byte owned by the pinned request or retained slot" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 988:5 | "width and address range are checked; the caller retains the initialized slot's physical permit" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 1024:5 | "the caller owns exactly size writable bytes under the native payload permit; width and value were checked above" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 1042:9 | "both addresses have the checked canonical alloc(size,align) / free(pointer,size,align) ABI" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 1058:5 | "the allocation receipt pairs this owner with its original free address and exact size/alignment" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 1066:5 | "the admitted runtime signature or pinned clone/drop role was checked before invoking its fixed word ABI" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 1074:1 | "calls machine code; the caller vouches for its three-word System V signature" |
| jet | Compiler/JetBackend/Source/OS/SessionOutput.jet | 7:5 | "fd is this host's owned anonymous output file; no request is running" |
| jet | Compiler/JetBackend/Source/OS/SessionOutput.jet | 16:5 | "the child's completed request no longer writes this anonymous file" |
| jet | Compiler/JetBackend/Source/OS/SessionOutput.jet | 36:13 | "index is within the bytes just read into this mapping" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 6:1 | "returns the process-lifetime execution resource words; the caller serializes access" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 16:1 | "serializes access to this process's private execution resource" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 24:1 | "releases the private execution mutex acquired by this thread" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 32:1 | "posix_spawn creates a fresh process, not a forked copy of compiler/runtime locks; paths and output are caller-owned" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 77:1 | "waits only for this owned child; status points at one writable word" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 96:5 | "sendto borrows the caller-owned readable bytes; MSG_NOSIGNAL is 0x4000 on Linux" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 101:5 | "fd belongs to this private execution host" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 149:9 | "header has32 writable bytes; a chunk count has at most5 decimal digits" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 158:9 | "both decimal digits are inside the caller-owned header scratch" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 165:5 | "the newline follows at most5 digits in32 writable header bytes" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 213:9 | "one byte was read into this owned mapping" |
| jet | Compiler/JetBackend/Source/OS/SessionTransport.jet | 229:13 | "index is within the bytes just read into this mapping" |
| jet | Compiler/JetBackend/Tests/CallImages.jet | 138:5 | "the separate observer mapping remains owned, readable and writable until the final assertion" |
| jet | Compiler/JetBackend/Tests/CallableStatics.jet | 77:5 | "the loaded image owns both observer pointers and their writable static cells through unload; fini is the checked zero-argument Unit function" |
| jet | Compiler/JetBackend/Tests/SessionHostFixtures.jet | 151:9 | "half-close only this owned socket's write side; leave read open for HostError" |
| jet | Compiler/JetBackend/Tests/SessionHostFixtures.jet | 291:5 | "the checked fixture is a no-argument System V function returning the word 7; its mapping is retained through the call" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 69:5 | "anonymous private mmap owns fresh writable bytes and cannot alias a live heap block; a failed syscall produces no allocation" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 83:5 | "the caller has removed this mapping from all heap lists and no payload remains live" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 98:5 | "root is the aligned lock word in the process-lifetime state mapping; CAS acquires all protected slab metadata" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 104:5 | "the calling thread holds the heap lock; x86 release store publishes its completed metadata updates" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 111:5 | "slab and its linked neighbours are live heap-owned headers protected by the held heap lock" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 129:5 | "head and slab are live heap-owned metadata protected by the held heap lock" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 145:5 | "the fresh 64KiB mapping holds every aligned free-block link; the heap lock excludes all list readers" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 162:5 | "the held heap lock protects the class list, block links and slab live count; the chosen block has at least size+16+align-1 bytes" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 195:5 | "pointer's two header words and the aligned payload are wholly within this fresh page-rounded mapping" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 220:5 | "the caller holds the policy lock and cell is a counter in alloc_root" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 228:5 | "the aligned mode word is read atomically on x86; system allocation needs no policy lock" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 234:5 | "the policy lock exclusively owns the mode, cap and accounting words" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 263:5 | "the policy lock exclusively owns live bytes; restoration does not erase outstanding tracked blocks" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 277:5 | "the held policy lock serializes all native allocator configuration and reservations" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 294:5 | "the policy lock serializes restoration with native reserve and free; tracked blocks remain in the side table" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 308:5 | "the policy lock protects this bounded fact word in alloc_root" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 323:5 | "the private raw-heap node is fully initialized before linking under the policy/tracker lock" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 336:5 | "the process-lifetime tracker count is an aligned x86 word; a zero count avoids any lookup or lock" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 343:5 | "the policy/tracker lock exclusively owns all side-table links and count" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 426:9 | "the lowering returns this still-live direct allocation with exactly its original size and alignment" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 435:5 | "the lowering frees exactly once with the allocation's original layout; its slab remains live until this last payload is returned under the heap lock" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 464:5 | "remaining is in 1..=8, so write borrows only the initialized stack word; a short write advances within it" |
| jet | Compiler/JetRuntime/Source/Alloc.jet | 504:5 | "the stack-word syscalls unblock SIGABRT and, if necessary, restore its default disposition before raising it again; no heap pointer is needed" |
| jet | Compiler/JetRuntime/Source/Atomic.jet | 14:5 | "place is caller-owned aligned8 private unpublished storage for one atomic word" |
| jet | Compiler/JetRuntime/Source/Atomic.jet | 19:5 | "place borrows one aligned8 live atomic word; x86 aligned MOV is the SeqCst load mapping" |
| jet | Compiler/JetRuntime/Source/Atomic.jet | 24:5 | "place borrows one aligned8 live atomic word; XCHG is the full-barrier SeqCst store mapping" |
| jet | Compiler/JetRuntime/Source/Atomic.jet | 29:5 | "place borrows one aligned8 live atomic word; LOCK CMPXCHG orders success and failure SeqCst" |
| jet | Compiler/JetRuntime/Source/Atomic.jet | 34:5 | "place borrows one aligned8 live atomic word; LOCK XADD returns the previous wrapping64 value SeqCst" |
| jet | Compiler/JetRuntime/Source/ClosureRefs.jet | 9:5 | "the caller's owned reference keeps the aligned closure header live; locked compare-exchange atomically publishes one more owner" |
| jet | Compiler/JetRuntime/Source/ClosureRefs.jet | 20:5 | "consumes one reference to an aligned live closure header; locked compare-exchange provides release ordering and last-owner acquire before its release helper" |
| jet | Compiler/JetRuntime/Source/Collections.jet | 9:5 | "data lends index's initialized native U8 element; low byte is meaningful in either epoch" |
| jet | Compiler/JetRuntime/Source/Collections.jet | 14:5 | "data owns index's complete native U8 slot; initialize word-epoch padding before low-byte store" |
| jet | Compiler/JetRuntime/Source/Collections.jet | 72:5 | "checked native value-returning callback; its raw argument stays valid for the synchronous call" |
| jet | Compiler/JetRuntime/Source/Collections.jet | 78:5 | "checked native value-returning callback; both raw arguments stay valid for the synchronous call" |
| jet | Compiler/JetRuntime/Source/Collections.jet | 84:5 | "checked unary closure adapter with borrowed environment, input and output slots" |
| jet | Compiler/JetRuntime/Source/Collections.jet | 90:5 | "checked binary closure adapter with live environment, inputs and output slots" |
| jet | Compiler/JetRuntime/Source/Collections.jet | 96:5 | "checked native Unit callback; its raw argument stays valid for the synchronous call" |
| jet | Compiler/JetRuntime/Source/Collections.jet | 102:5 | "checked Unit clone callback; source remains live and target lends writable output storage" |
| jet | Compiler/JetRuntime/Source/CompilerFS.jet | 155:17 | "the prefix remains terminated and scratch lends 128 writable portable stat-fact bytes" |
| jet | Compiler/JetRuntime/Source/CompilerMapped.jet | 113:13 | "the owned readable file lends its descriptor; mmap creates exactly length read-only private bytes; the map owner alone unmaps them" |
| jet | Compiler/JetRuntime/Source/CountMinSketch.jet | 9:5 | "caller holds this native CMS mutex and owns the initialized four-byte counter" |
| jet | Compiler/JetRuntime/Source/CryptoEntropy.jet | 49:13 | "the test/fault harness owns the synchronous callback and context until reset; the callback borrows only this writable suffix" |
| jet | Compiler/JetRuntime/Source/CryptoEntropy.jet | 54:5 | "Linux getrandom flags0 writes only the exclusively borrowed writable span and retains no address; kernel negative errno is preserved" |
| jet | Compiler/JetRuntime/Source/DataEvent.jet | 5:5 | "address borrows an initialized word in a live native DataEvent or List" |
| jet | Compiler/JetRuntime/Source/DataEvent.jet | 8:5 | "address owns a writable word in a native DataEvent" |
| jet | Compiler/JetRuntime/Source/DataLimits.jet | 6:5 | "address borrows an initialized word in a live native limits carrier" |
| jet | Compiler/JetRuntime/Source/DataLimits.jet | 9:5 | "address owns a writable word in a native limits carrier or getter out slot" |
| jet | Compiler/JetRuntime/Source/Dyn.jet | 7:5 | "address borrows an initialized word in a live native Dyn or List" |
| jet | Compiler/JetRuntime/Source/Dyn.jet | 10:5 | "address owns a writable word in a native Dyn or List" |
| jet | Compiler/JetRuntime/Source/Deadline.jet | 65:5 | "clock_gettime writes two thread-owned TCB words, not retained after the syscall" |
| jet | Compiler/JetRuntime/Source/Display.jet | 8:5 | "borrows an initialized aligned word in an E4 header, exact-Int node or owned formatter workspace" |
| jet | Compiler/JetRuntime/Source/Display.jet | 14:5 | "writes an aligned word in an exclusively owned formatter workspace or fresh E4 header" |
| jet | Compiler/JetRuntime/Source/Display.jet | 20:5 | "reads one initialized byte in an owned formatter digit buffer or borrowed exact-Int limb" |
| jet | Compiler/JetRuntime/Source/Display.jet | 26:5 | "writes one byte in an exclusively owned formatter buffer; value is in 0..255" |
| jet | Compiler/JetRuntime/Source/EncodingErrors.jet | 6:5 | "borrows an initialized aligned word in a live encoding carrier or option input" |
| jet | Compiler/JetRuntime/Source/EncodingErrors.jet | 9:5 | "owns a writable aligned word in a fresh encoding carrier or caller output" |
| jet | Compiler/JetRuntime/Source/GeometryGlue.jet | 5:5 | "borrows an initialized aligned word inside a live native geometry carrier" |
| jet | Compiler/JetRuntime/Source/GeometryGlue.jet | 8:5 | "owns a writable aligned word in a fresh native geometry carrier" |
| jet | Compiler/JetRuntime/Source/HTTPBody.jet | 40:5 | "borrows an initialized word in the live body, chunks, closure, List or E4 allocation" |
| jet | Compiler/JetRuntime/Source/HTTPBody.jet | 46:5 | "owns the word, or holds the body's transition lock for this state field" |
| jet | Compiler/JetRuntime/Source/HTTPBody.jet | 52:5 | "atomic refcount, take/close flag or transition lock in a live body allocation" |
| jet | Compiler/JetRuntime/Source/HTTPBody.jet | 103:5 | "the last closure reference calls its generated release(box), which drops captures and frees the box" |
| jet | Compiler/JetRuntime/Source/HTTPBody.jet | 213:5 | "the claimed closer code has the audited code(box) ABI, with no transition lock held" |
| jet | Compiler/JetRuntime/Source/HTTPBody.jet | 326:17 | "reader code(box, descriptor) borrows exactly count writable U8 bytes; no body lock is held" |
| jet | Compiler/JetRuntime/Source/HTTPBody.jet | 379:17 | "the source holds count initialized UTF8 or raw reader bytes" |
| jet | Compiler/JetRuntime/Source/Host.jet | 14:5 | "runtime startup owns a fresh anonymous mapping before any program thread exists" |
| jet | Compiler/JetRuntime/Source/Host.jet | 28:5 | "rt_sigaction synchronously borrows the four-word SIG_IGN action; no signal handler or restorer runs for ignored SIGPIPE" |
| jet | Compiler/JetRuntime/Source/Host.jet | 45:5 | "the caller lends length readable bytes for the complete synchronous write" |
| jet | Compiler/JetRuntime/Source/Host.jet | 60:5 | "the output lock exclusively owns the initialized buffer and length" |
| jet | Compiler/JetRuntime/Source/Host.jet | 88:5 | "the caller holds the output lock; each byte stays within the 8192-byte mapping-owned buffer" |
| jet | Compiler/JetRuntime/Source/Host.jet | 108:5 | "the caller lends length bytes; the output lock owns the destination buffer" |
| jet | Compiler/JetRuntime/Source/Host.jet | 153:5 | "bytes lends length readable bytes; writev borrows its own LF until return" |
| jet | Compiler/JetRuntime/Source/Host.jet | 170:5 | "the held host lock lends enough scratch for Linux TCGETS" |
| jet | Compiler/JetRuntime/Source/Host.jet | 193:5 | "value is a borrowed E4 JetCText header; stdout copies its bytes before returning" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 8:5 | "the caller borrows a live FD72 or runtime scratch word" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 14:5 | "the caller owns the FD72 lock or exclusive runtime scratch word" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 54:5 | "the fresh path allocation includes its terminator byte" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 264:9 | "the caller lends 128 writable bytes for portable stat facts" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 352:5 | "the terminated borrowed path and 128-byte stat fact output are live for the syscall" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 403:5 | "the caller lends one writable Bool byte" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 418:9 | "the held directory and terminated source-approved single name stay live through openat" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 440:9 | "the held directory and terminated final name live through O_PATH&#124;O_NOFOLLOW&#124;O_CLOEXEC openat; no read access or link traversal" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 448:9 | "openat returned an exclusively owned descriptor and the caller lends StatFacts128; fstat copies initialized scalar facts" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 482:5 | "both duplicated descriptors and the terminated name live through the held-inode linkat syscall" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 504:9 | "the descriptor is held live, output begins with the borrowed empty pathname, statx writes only its private stack frame" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 515:5 | "the terminated source-approved path and three writable birth-time output words stay live through statx" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 569:5 | "both terminated borrowed names and the caller's optional compact byte output remain live only through lgetxattr" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 598:9 | "the held directory, terminated plain name and writable byte range are borrowed only through readlinkat" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 624:5 | "held directory duplicates and terminated names prevent any descriptor-close/reuse race during renameat2" |
| jet | Compiler/JetRuntime/Source/HostFiles.jet | 644:9 | "the held fd and stack-local source-validated winsize word live only through ioctl" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 21:5 | "value transfers one 56-byte error header and its nullable owned E4 text children" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 32:5 | "value borrows the complete error header; result exclusively owns 56 fresh bytes" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 65:5 | "value exclusively owns a new initialized IOError with two owned text children; error lends the caller's pointer slot" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 82:5 | "value lends five initialized context words; result exclusively owns its header and cloned nullable E4 children" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 98:5 | "value transfers its 40-byte header and two independently owned nullable text children" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 114:9 | "the optional code box borrows one initialized exact-Int word" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 120:5 | "nullable option boxes lend one initialized payload word; value owns cloned texts and the complete new context" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 131:5 | "value borrows an initialized context header" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 135:5 | "value borrows a context; some lends the writable pointer output for a present field" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 147:5 | "value borrows a context; some lends the writable exact-Int word output for a present field" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 155:5 | "value borrows an initialized error header" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 159:5 | "value borrows a complete error header; the selected payload must match its canonical tag" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 170:5 | "context transfers its initialized five words and text owners into the fresh error; only the context header is then freed" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 184:5 | "value borrows an initialized ResourceLimit error" |
| jet | Compiler/JetRuntime/Source/HostIO.jet | 199:5 | "value exclusively owns seven initialized words; ResourceLimit has no text children" |
| jet | Compiler/JetRuntime/Source/HostNet.jet | 8:5 | "value exclusively owns four initialized words; inputs lend exact Int owners and a valid E4 message" |
| jet | Compiler/JetRuntime/Source/HostNet.jet | 18:5 | "value lends the complete immutable carrier and all three owned children" |
| jet | Compiler/JetRuntime/Source/HostNet.jet | 25:5 | "value transfers its 32-byte header, two independently retained Int words and owned E4 message" |
| jet | Compiler/JetRuntime/Source/HostNet.jet | 34:5 | "value lends the immutable carrier; return retains its exact Int field" |
| jet | Compiler/JetRuntime/Source/HostNet.jet | 38:5 | "value lends the initialized presence word" |
| jet | Compiler/JetRuntime/Source/HostNet.jet | 42:5 | "value lends the immutable carrier; return retains its exact Int field" |
| jet | Compiler/JetRuntime/Source/HostNet.jet | 46:5 | "value lends the immutable carrier; return deep-clones its owned E4 message" |
| jet | Compiler/JetRuntime/Source/HostNetAddress.jet | 5:5 | "value lends a complete initialized network header" |
| jet | Compiler/JetRuntime/Source/HostNetAddress.jet | 8:5 | "value exclusively owns the network header or its mutex" |
| jet | Compiler/JetRuntime/Source/HostNetAddress.jet | 38:5 | "value owns sixteen octets; bytes lends length initialized U8 elements" |
| jet | Compiler/JetRuntime/Source/HostNetAddress.jet | 50:5 | "value lends IP24; data owns length initialized native U8 elements; output receives ownership" |
| jet | Compiler/JetRuntime/Source/HostNetAddress.jet | 83:5 | "bytes lends width initialized little-endian bytes (width&lt;=8)" |
| jet | Compiler/JetRuntime/Source/HostNetAddress.jet | 93:5 | "bytes owns width writable little-endian bytes (width&lt;=8)" |
| jet | Compiler/JetRuntime/Source/HostNetPoller.jet | 60:5 | "the root lock owns a packed 12B epoll_event; state retains the native fd through this synchronous call" |
| jet | Compiler/JetRuntime/Source/HostNetPoller.jet | 86:9 | "events owns 64 packed epoll_event entries; epoll_wait retains no buffer pointer" |
| jet | Compiler/JetRuntime/Source/HostNetResolver.jet | 104:5 | "host lends length E4 bytes; outputs lend their writable pointer/count words" |
| jet | Compiler/JetRuntime/Source/HostNetResolver.jet | 124:5 | "name owns a trailing NUL byte" |
| jet | Compiler/JetRuntime/Source/HostPipe.jet | 128:13 | "the FD72 lock protects this initialized refill-buffer byte" |
| jet | Compiler/JetRuntime/Source/HostProcess.jet | 5:5 | "the audited caller supplies the Linux syscall's valid word arguments" |
| jet | Compiler/JetRuntime/Source/HostProcess.jet | 66:5 | "the caller lends the C-ABI tuple's Bool byte and two writable Int words" |
| jet | Compiler/JetRuntime/Source/HostProcess.jet | 143:5 | "some lends a writable C-ABI Option success word" |
| jet | Compiler/JetRuntime/Source/HostProcess.jet | 153:5 | "status lends an eight-byte out slot; wait4 writes its low four bytes and retains no pointer" |
| jet | Compiler/JetRuntime/Source/HostProcess.jet | 165:5 | "read_fd lends eight writable bytes to pipe's two u32 results; split both facts into the caller's separate word slots" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 13:9 | "the FD72 lock prevents native descriptor close/reuse throughout this nonblocking syscall" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 54:13 | "accept4 retains no address pointers and atomically sets CLOEXEC/NONBLOCK on the accepted socket" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 61:13 | "private scratch lends sockaddr bytes and its exact 4B socklen; accept4 retains neither pointer" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 96:9 | "private scratch lends one native int and its 4B socklen; getsockopt retains neither pointer" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 111:9 | "private scratch lends exactly one native int; setsockopt retains no pointer" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 135:5 | "data lends length native U8 elements; output owns length compact bytes" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 149:9 | "the held FD72 lock lends its live nonblocking socket; packed lends exactly length bytes; no pointer survives recvfrom/sendto" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 154:13 | "recvfrom initialized result compact bytes; data owns the corresponding mutable native U8 slots" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 185:9 | "the FD72 lock retains the socket; private scratch lends sockaddr bytes; packed lends length bytes; sendto retains no pointers" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 202:9 | "private scratch lends sockaddr/socklen and packed lends length mutable bytes; MSG_TRUNC returns the complete datagram length while writing at most length bytes" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 211:17 | "recvfrom initialized used compact bytes; the caller owns used mutable native U8 elements" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 235:5 | "path lends its immutable E4 bytes for std-compatible sockaddr_un validation" |
| jet | Compiler/JetRuntime/Source/HostSockets.jet | 257:13 | "scratch owns the sockaddr_un pathname and its final zero byte" |
| jet | Compiler/JetRuntime/Source/HostSpawn.jet | 224:17 | "Linux TCGETS initialized the 36-byte termios buffer; cfmakeraw sets VMIN=1, VTIME=0 in its control-character array" |
| jet | Compiler/JetRuntime/Source/HostSpawn.jet | 291:13 | "rt_sigaction borrows four initialized stack words; no allocator, mutex or TLS is touched after fork" |
| jet | Compiler/JetRuntime/Source/HostSpawn.jet | 709:9 | "ioctl borrows one stack word of source-validated winsize dimensions; no pointer escapes" |
| jet | Compiler/JetRuntime/Source/HostSpawn.jet | 725:5 | "source lends a NUL-terminated raw path including non-Unicode bytes and a writable 128-byte stat fact row" |
| jet | Compiler/JetRuntime/Source/HostSpawn.jet | 736:5 | "the source-approved native NUL path and Jet-owned PATH_MAX buffer are live across the mechanical realpath call" |
| jet | Compiler/JetRuntime/Source/HostSpawn.jet | 741:9 | "libc __errno_location returns this calling thread's live errno int; read only its initialized four bytes" |
| jet | Compiler/JetRuntime/Source/HostSpawn.jet | 752:5 | "successful realpath NUL-terminates its result within the PATH_MAX byte buffer" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 22:5 | "bytes borrows one live NUL-terminated startup string" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 45:5 | "startup lends the immutable, null-terminated envp pointer array" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 58:5 | "rows exclusively owns raw_count index records; startup lends every pointed-to NUL string" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 79:5 | "storage exclusively owns total_bytes; each source row lends its NUL string for this capture call, and published rows point only into the owned block" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 111:5 | "bytes lends one live NUL-terminated startup string" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 125:5 | "startup lends length bytes; data exclusively owns length initialized packed bytes" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 138:5 | "output lends a 24-byte native List&lt;U8&gt; header and receives ownership of data" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 152:5 | "row borrows a complete immutable startup index record" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 167:5 | "row borrows its immutable record; data_out lends one writable pointer word" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 176:5 | "row borrows its immutable record; data_out lends one writable pointer word" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 192:5 | "startup lends argc initialized argv pointers and their immutable NUL strings" |
| jet | Compiler/JetRuntime/Source/HostStartup.jet | 211:5 | "the startup index and key bytes are immutable borrowed data" |
| jet | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 21:5 | "value borrows a complete immutable startup NUL string; output lends one writable U64 word" |
| jet | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 48:5 | "value borrows length immutable captured startup bytes; no pointer escapes" |
| jet | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 82:5 | "the held Host lock lends 16 writable bytes of its existing ioctl scratch; the canonical lookup retains no key pointer" |
| jet | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 92:5 | "output exclusively borrows the paired initialized empty Source out-record; no owner is overwritten" |
| jet | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 115:5 | "Host startup already initialized its process-lifetime mapping; the held lock owns ioctl scratch32..47 and FIRST raw lookup only borrows the immutable captured index" |
| jet | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 147:13 | "the paired Source owner receives the sole native U8 buffer with exact LEN/CAP/DATA; no raw pointer is cast into a List value" |
| jet | Compiler/JetRuntime/Source/HostStdio.jet | 13:5 | "text lends the E4 header and readable bytes for this synchronous write" |
| jet | Compiler/JetRuntime/Source/HostStdio.jet | 20:5 | "text lends the E4 header and readable bytes; the LF enters the same stream buffer without a temporary String" |
| jet | Compiler/JetRuntime/Source/HostStdio.jet | 41:5 | "text lends the E4 header and readable bytes for this synchronous write" |
| jet | Compiler/JetRuntime/Source/HostStdio.jet | 48:5 | "text lends the E4 header and readable bytes; writev borrows a stack LF, never an allocated line String" |
| jet | Compiler/JetRuntime/Source/HostStdio.jet | 71:5 | "the caller lends the tuple's exact-Int word and Bool byte outputs" |
| jet | Compiler/JetRuntime/Source/HostStdio.jet | 78:5 | "text lends an E4 header and its bytes for the synchronous buffered write" |
| jet | Compiler/JetRuntime/Source/HostStdio.jet | 96:5 | "text lends an E4 header and its bytes; writev borrows its own stack LF" |
| jet | Compiler/JetRuntime/Source/HostTime.jet | 27:5 | "clock_gettime synchronously writes the calling thread's two scratch words" |
| jet | Compiler/JetRuntime/Source/HostTime.jet | 56:5 | "before_out lends the tuple's one writable Bool byte" |
| jet | Compiler/JetRuntime/Source/HostTime.jet | 148:5 | "path lends length bytes; std file open rejects embedded NUL" |
| jet | Compiler/JetRuntime/Source/HostTime.jet | 156:5 | "name owns its trailing NUL byte" |
| jet | Compiler/JetRuntime/Source/HostTime.jet | 189:13 | "bytes lends used initialized bytes; data owns used native U8 elements" |
| jet | Compiler/JetRuntime/Source/HyperLogLog.jet | 332:9 | "one native HLL mutex protects this packed register byte" |
| jet | Compiler/JetRuntime/Source/Int.jet | 10:5 | "borrow an initialized word in a live exact-Int carrier" |
| jet | Compiler/JetRuntime/Source/Int.jet | 16:5 | "write an initialized word in a uniquely owned exact-Int workspace" |
| jet | Compiler/JetRuntime/Source/Int.jet | 22:5 | "borrow four initialized bytes in a live limb buffer; no adjacent limb is read" |
| jet | Compiler/JetRuntime/Source/Int.jet | 28:5 | "write four bytes in a uniquely owned limb buffer; value is an unsigned 32-bit limb" |
| jet | Compiler/JetRuntime/Source/Int.jet | 300:9 | "the caller holds an existing owner; atomic retain cannot race final-owner retirement" |
| jet | Compiler/JetRuntime/Source/Int.jet | 321:9 | "the caller consumes one owner; atomic decrement publishes refs0 before shared-domain retirement" |
| jet | Compiler/JetRuntime/Source/Int.jet | 326:13 | "the final owner has reached zero; the remaining shared hazard domain decides immediate free or retirement" |
| jet | Compiler/JetRuntime/Source/IntHazard.jet | 90:5 | "the native numeric forwarder lends a pure reload callback and its live closure context for this synchronous call" |
| jet | Compiler/JetRuntime/Source/Main.jet | 106:5 | "entry is the loader/object's checked executable System V void(void) address, not a closure header" |
| jet | Compiler/JetRuntime/Source/MemStorage.jet | 22:5 | "address borrows an initialized word in a native allocator header, descriptor, drop record, or Fixed metadata span" |
| jet | Compiler/JetRuntime/Source/MemStorage.jet | 25:5 | "address exclusively owns one aligned writable allocator or Fixed metadata word" |
| jet | Compiler/JetRuntime/Source/MemStorage.jet | 78:5 | "code is the Lower-generated address-based lazy.drop thunk for the initialized value at pointer; its SysV result is Unit and discarded, and no allocator lock is held" |
| jet | Compiler/JetRuntime/Source/NativeLayoutABI.jet | 11:5 | "thread-confined live Layout storage lends this initialized aligned word" |
| jet | Compiler/JetRuntime/Source/NativeLayoutABI.jet | 14:5 | "thread-confined native Layout storage owns this aligned word" |
| jet | Compiler/JetRuntime/Source/ObserveLive.jet | 135:5 | "vetted Observe Linux file/clock syscalls borrow native buffers only for their duration" |
| jet | Compiler/JetRuntime/Source/Parallel.jet | 31:9 | "checked System V (env,start,end)-&gt;Unit callback; env remains borrowed and each range has exclusive output slots" |
| jet | Compiler/JetRuntime/Source/ScalarReflectGlue.jet | 4:5 | "borrows an initialized aligned word in a live scalar/reflection carrier" |
| jet | Compiler/JetRuntime/Source/ScalarReflectGlue.jet | 7:5 | "owns a writable aligned word in a native scalar/reflection carrier" |
| jet | Compiler/JetRuntime/Source/Scheduler.jet | 120:5 | "the cold OS leaf borrows its initialized stack word; a successful nonzero base is this attached thread's live Jet TCB" |
| jet | Compiler/JetRuntime/Source/Scheduler.jet | 719:9 | "sched_getaffinity writes only the owned mask; pid0 names this thread" |
| jet | Compiler/JetRuntime/Source/Scheduler.jet | 945:9 | "sched_yield retains no memory and yields this OS worker" |
| jet | Compiler/JetRuntime/Source/Secret.jet | 11:5 | "address names the caller-owned live Secret buffer; effectful asm stores wipe every byte including spare capacity before release" |
| jet | Compiler/JetRuntime/Source/Secret.jet | 24:5 | "bytes is the exclusive moved native List&lt;U8&gt; header; secret is fresh aligned24B storage and adopts its sole buffer without copying secret material" |
| jet | Compiler/JetRuntime/Source/Secret.jet | 42:5 | "secret is a live exclusively borrowed native24B Secret header; len/cap/data follow the native List layout and all byte owners remain inside its buffer" |
| jet | Compiler/JetRuntime/Source/Sync.jet | 6:5 | "borrows an aligned live synchronization word" |
| jet | Compiler/JetRuntime/Source/Sync.jet | 10:5 | "publishes an aligned live synchronization word with x86 release ordering" |
| jet | Compiler/JetRuntime/Source/Sync.jet | 16:5 | "atomic exchange on an aligned live synchronization word" |
| jet | Compiler/JetRuntime/Source/Sync.jet | 22:5 | "atomic fetch-add on an aligned live synchronization word" |
| jet | Compiler/JetRuntime/Source/Sync.jet | 28:5 | "atomic compare-exchange on an aligned live synchronization word" |
| jet | Compiler/JetRuntime/Source/Sync.jet | 34:5 | "futex wake borrows a live aligned word and retains no pointer" |
| jet | Compiler/JetRuntime/Source/Sync.jet | 42:5 | "futex borrows live state; timed wait uses this attached thread's reserved TCB scratch until the syscall returns" |
| jet | Compiler/JetRuntime/Source/Sync.jet | 54:5 | "clock_gettime writes two words of thread-owned TCB scratch; no pointer escapes" |
| jet | Compiler/JetRuntime/Source/TaskGroup.jet | 250:9 | "the canonical task selector yields without consuming any child result" |
| jet | Compiler/JetRuntime/Source/Test.jet | 124:9 | "the OS leaf borrows at most eight initialized bytes from its own stack word" |
| jet | Compiler/JetRuntime/Source/TestSuite.jet | 79:5 | "the pinned named-fn box code takes its box and writable inline 16B result storage" |
| jet | Compiler/JetRuntime/Source/Text.jet | 10:5 | "address borrows an initialized word in a live text or iterator header" |
| jet | Compiler/JetRuntime/Source/Text.jet | 16:5 | "address is an exclusively owned writable header word" |
| jet | Compiler/JetRuntime/Source/Text.jet | 20:5 | "address borrows an initialized byte inside the caller's live buffer" |
| jet | Compiler/JetRuntime/Source/Text.jet | 26:5 | "address is one exclusively owned writable byte and value is a byte" |
| jet | Compiler/JetRuntime/Source/TextGlue.jet | 4:5 | "borrows an initialized aligned word in a live native text diagnostic" |
| jet | Compiler/JetRuntime/Source/TextGlue.jet | 7:5 | "owns a writable aligned word in a fresh native text diagnostic" |
| jet | Compiler/JetRuntime/Source/Thread.jet | 50:5 | "every caller has attached before running Jet TLS code" |
| jet | Compiler/JetRuntime/Source/Thread.jet | 54:5 | "GS is reserved for Jet and base is a live self-addressed TCB or zero at detach" |
| jet | Compiler/JetRuntime/Source/Thread.jet | 61:5 | "gettid identifies this thread without reading its inherited GS" |
| jet | Compiler/JetRuntime/Source/Thread.jet | 243:9 | "the registry owns this live closure header whose code accepts header and one I64 value" |
| jet | Compiler/JetRuntime/Source/Thread.jet | 328:5 | "the caller transferred a System V I64-&gt;I64 code address and one argument to this thread" |
| jet | Compiler/JetRuntime/Source/Thread.jet | 378:5 | "fresh raw clone stack mapping with a PROT_NONE first-page guard; child may call only Jet/syscall code, never musl/Rust with inherited FS" |
| jet | Compiler/JetRuntime/Source/Thread.jet | 395:5 | "guarded stack is seeded with Jet-only entry/arg; TID word is live until join; flags share VM, FS, FILES, SIGHAND, THREAD, SYSVSEM and enable parent-set/child-clear TID" |
| jet | Compiler/JetRuntime/Source/Thread.jet | 410:9 | "kernel CHILD_CLEARTID clears this live word and wakes a non-private futex; recheck handles exit-before-wait and EINTR" |
| jet | Compiler/JetRuntime/Source/TimeGlue.jet | 4:5 | "borrows an initialized aligned word inside a native time carrier" |
| jet | Compiler/JetRuntime/Source/TimeGlue.jet | 7:5 | "owns a writable aligned word inside a fresh native time carrier" |
| jet | Compiler/JetRuntime/Source/Trap.jet | 9:5 | "the caller borrows an initialized word of a live JetText or static require descriptor" |
| jet | Compiler/JetRuntime/Source/Trap.jet | 15:5 | "the caller borrows exactly one initialized byte of a live UTF-8 carrier" |
| jet | Compiler/JetRuntime/Source/Trap.jet | 21:5 | "the caller owns eight writable bytes in a scratch mapping or allocated JetText storage" |
| jet | Compiler/JetRuntime/Source/Trap.jet | 27:5 | "address is one writable byte in the allocated debug-text buffer; value is a byte" |
| jet | Compiler/JetRuntime/Source/Trap.jet | 33:5 | "audited runtime syscall: write borrows live text, mmap creates scratch, munmap releases it, exit_group ends the process" |
| jet | Compiler/JetRuntime/Source/Trap.jet | 69:9 | "the OS leaf borrows a stack word; generated calls have 1..8 valid bytes" |
| jet | Compiler/JetRuntime/Source/UiCarriersEnums.jet | 5:5 | "address borrows an initialized native UI carrier or E4 header word" |
| jet | Compiler/JetRuntime/Source/UiCarriersEnums.jet | 10:5 | "address exclusively owns one writable native UI carrier word" |
| jet | Compiler/JetRuntime/Source/Unicode.jet | 88:5 | "some is the caller's writable aligned Option&lt;Int&gt; payload slot, written only on success" |
| jet | Compiler/JetRuntime/Source/World.jet | 76:5 | "registered provider code remains live at this world/lease boundary; output lends one writable word" |
| jet | Compiler/JetRuntime/Source/XMLGlue.jet | 5:5 | "borrows an initialized aligned word in a live XML carrier or optional input" |
| jet | Compiler/JetRuntime/Source/XMLGlue.jet | 8:5 | "owns a writable aligned word in a fresh XML carrier or caller output" |
| jet | Compiler/JetRuntime/Tests/AllocTests.jet | 9:5 | "a failed allocator witness terminates this isolated test process" |
| jet | Compiler/JetRuntime/Tests/AllocTests.jet | 25:5 | "both allocations own count bytes; touching their endpoints must not alias each other or their metadata" |
| jet | Compiler/JetRuntime/Tests/AllocTests.jet | 69:5 | "the test is the only heap user and the class has a retained empty slab" |
| jet | Compiler/JetRuntime/Tests/AllocTests.jet | 76:9 | "pointer's private header identifies its still-live slab, which must be the retained mapping" |
| jet | Compiler/JetRuntime/Tests/AllocTests.jet | 80:9 | "no other thread touches the retained slab or its state cell" |
| jet | Compiler/JetRuntime/Tests/AllocTests.jet | 96:9 | "the test owns the full payload and can read its allocator header" |
| jet | Compiler/JetRuntime/Tests/AllocTests.jet | 109:9 | "the chain word remains readable until its payload is freed" |
| jet | Compiler/JetRuntime/Tests/AllocTests.jet | 115:5 | "after all payloads are freed, exactly one empty slab is retained; any second empty slab was unlinked and unmapped" |
| jet | Compiler/JetRuntime/Tests/AllocTests.jet | 162:5 | "the exact success line is two bounded stack words written to stdout" |
| jet | Compiler/JetRuntime/Tests/AllocationPolicyTests.jet | 4:5 | "the single-threaded fixture reads the process-lifetime tracked-node count after all its payloads were freed" |
| jet | Compiler/JetRuntime/Tests/EnvOverlayKernel.jet | 6:5 | "failed witness terminates its isolated real-runtime process" |
| jet | Compiler/JetRuntime/Tests/SchedulerTests.jet | 5:5 | "an isolated failed scheduler witness exits with no further runtime cleanup" |
| jet | Compiler/JetRuntime/Tests/SchedulerTests.jet | 190:5 | "fixture byte writes stay within owned 19/20-byte text buffers" |
| jet | Compiler/JetRuntime/Tests/SchedulerTests.jet | 194:5 | "fixture byte writes stay within owned 19/20-byte text buffers" |
| jet | Compiler/JetRuntime/Tests/SchedulerTests.jet | 198:5 | "fixture byte writes stay within owned 19/20-byte text buffers" |
| jet | Compiler/JetRuntime/Tests/SchedulerTests.jet | 202:5 | "fixture byte writes stay within owned 19/20-byte text buffers" |
| jet | Compiler/JetRuntime/Tests/SchedulerTests.jet | 211:5 | "fixture byte writes stay within an owned one-byte text buffer" |
| jet | Compiler/JetRuntime/Tests/SchedulerTests.jet | 215:5 | "fixture byte writes stay within an owned one-byte text buffer" |
| jet | Compiler/JetRuntime/Tests/SecretTests.jet | 6:5 | "a failed isolated Secret witness terminates this test process" |
| jet | Compiler/JetRuntime/Tests/SecretTests.jet | 14:5 | "header is fresh aligned24B storage owned by the test; data owns cap times the host U8 stride bytes or is zero for an empty List" |
| jet | Compiler/JetRuntime/Tests/SecretTests.jet | 23:5 | "the pointer is a live small allocation; this isolated test is the sole allocator user and can inspect its retained slab metadata" |
| jet | Compiler/JetRuntime/Tests/SecretTests.jet | 36:5 | "input and secret are distinct live headers; adoption leaves one live buffer owner and borrowed bytes alias only that parent's header" |
| jet | Compiler/JetRuntime/Tests/SecretTests.jet | 57:5 | "the test owns data and checks the initialized U8 slots, including epoch padding, before any release" |
| jet | Compiler/JetRuntime/Tests/SecretTests.jet | 68:5 | "secret still owns its live24B header after Close; the buffer was released, and no pointer into it is used" |
| jet | Compiler/JetRuntime/Tests/SecretTests.jet | 80:5 | "reused is newly allocated live32B storage exclusively owned by this witness, not a read of freed storage" |
| jet | Compiler/JetRuntime/Tests/SecretTests.jet | 112:5 | "data is this test's live List allocation; every capacity byte is dirtied before the wipe" |
| jet | Compiler/JetRuntime/Tests/SecretTests.jet | 120:5 | "the same live allocation remains exclusively owned while this test checks all wiped bytes" |
| jet | Compiler/JetRuntime/Tests/SecretTests.jet | 138:5 | "this isolated native witness writes its17B success literal and exits without introducing any text or collection runtime dependency" |
| jet | Compiler/JetRuntime/Tests/SentryInlineTests.jet | 8:1 | "the raw pointer deliberately escapes its frame for the sentry witness" |
| jet | Compiler/JetRuntime/Tests/SentryInlineTests.jet | 13:1 | "an expired inline raw read must stop before any pointee byte is loaded" |
| jet | Compiler/JetRuntime/Tests/SentryTests.jet | 206:5 | "the checked Source callback uses the exact two-word native ABI" |
| jet | Compiler/JetRuntime/Tests/TextTests.jet | 6:5 | "a failed text runtime witness terminates only its isolated process" |
| jet | Compiler/JetRuntime/Tests/ThreadTests.jet | 8:5 | "a failed isolated runtime witness ends the process without allocation or libc" |
| jet | Compiler/JetRuntime/Tests/ThreadTests.jet | 36:5 | "gettid verifies identity without trusting the TCB under test" |
| jet | Compiler/JetRuntime/Tests/ThreadTests.jet | 45:5 | "ARCH_GET_GS writes a caller-owned word; the inherited pointer is compared, never dereferenced" |
| jet | Compiler/JetRuntime/Tests/ThreadTests.jet | 204:13 | "sched_yield widens both sides of the check-to-wait race without timing assumptions" |
| jet | Compiler/JetRuntime/Tests/ThreadTests.jet | 284:5 | "heap-free fixture success line is one eight-byte write" |
| jet | Compiler/JetRuntime/Tests/UnicodeTests.jet | 8:5 | "a failed Unicode runtime witness ends only its isolated test process" |
| jet | Core/auth/auth.jet | 112:5 | "auth PASETO v4.public verification uses the fixed Ed25519 primitive" |
| jet | Core/crypto/crypto.jet | 689:5 | "the expert API explicitly authorizes authenticated legacy-v1 migration" |
| jet | Core/crypto/crypto.jet | 1000:5 | "the expert API explicitly authorizes authenticated legacy-v1 migration" |
| jet | Core/crypto/crypto.jet | 1541:5 | "crypto expert: Argon2id password hashing" |
| jet | Core/crypto/crypto.jet | 1568:5 | "crypto expert: Argon2id password verification" |
| jet | Core/crypto/jetc.jet | 96:5 | "Core JETC2 applies the fixed XChaCha20-Poly1305 suite and protocol-owned nonce/AAD domains" |
| jet | Core/crypto/jetc.jet | 104:5 | "Core JETC2 authenticates a bounded record before releasing its plaintext" |
| jet | Core/crypto/vault.jet | 224:5 | "vault typed load provider boundary" |
| jet | Core/crypto/vault.jet | 231:5 | "vault typed current provider boundary" |
| jet | Core/crypto/vault.jet | 237:5 | "vault typed status provider boundary" |
| jet | Core/crypto/vault.jet | 244:5 | "vault typed versions provider boundary" |
| jet | Core/crypto/vault.jet | 251:5 | "vault typed generation provider boundary" |
| jet | Core/crypto/vault.jet | 258:5 | "vault typed store provider boundary" |
| jet | Core/crypto/vault.jet | 266:5 | "vault key import: signing key material" |
| jet | Core/crypto/vault.jet | 274:5 | "vault key import: X25519 secret material" |
| jet | Core/crypto/vault.jet | 282:5 | "vault wrapped import provider boundary" |
| jet | Core/crypto/vault.jet | 289:5 | "vault typed rotation provider boundary" |
| jet | Core/crypto/vault.jet | 296:5 | "vault typed retirement provider boundary" |
| jet | Core/crypto/vault.jet | 303:5 | "vault typed revocation provider boundary" |
| jet | Core/crypto/vault.jet | 310:5 | "vault write authorization provider boundary" |
| jet | Core/crypto/vault.jet | 317:5 | "vault wrapped authorization provider boundary" |
| jet | Core/crypto/vault.jet | 323:5 | "vault typed generation commit provider boundary" |
| jet | Core/crypto/vault.jet | 329:5 | "vault typed store commit provider boundary" |
| jet | Core/crypto/vault.jet | 335:5 | "vault key import commit: signing key material" |
| jet | Core/crypto/vault.jet | 341:5 | "vault key import commit: X25519 secret material" |
| jet | Core/crypto/vault.jet | 347:5 | "vault wrapped import commit provider boundary" |
| jet | Core/crypto/vault.jet | 353:5 | "vault typed rotation commit provider boundary" |
| jet | Core/crypto/vault.jet | 359:5 | "vault typed retirement commit provider boundary" |
| jet | Core/crypto/vault.jet | 365:5 | "vault typed revocation commit provider boundary" |
| jet | Core/crypto/vault.jet | 371:5 | "vault passphrase export provider boundary" |
| jet | Core/crypto/vault.jet | 378:5 | "vault recipient export provider boundary" |
| jet | Core/data/data.jet | 1266:9 | "native path bytes originate from the host environment and are split at ASCII boundaries" |
| jet | Core/data/data.jet | 1876:5 | "inspect existing private Decode carriers only inside checked Core data source" |
| jet | Core/db/db.jet | 43:9 | "consume the unique SQLite queue handle through its existing host close leaf" |
| jet | Core/db/db.jet | 49:5 | "open the existing SQLite bridge and immediately adopt its handle into a consuming Core resource" |
| jet | Core/db/db.jet | 56:5 | "borrow the live unique queue handle for BEGIN" |
| jet | Core/db/db.jet | 59:5 | "borrow the live unique queue handle for COMMIT" |
| jet | Core/db/db.jet | 62:5 | "borrow the live unique queue handle for ROLLBACK" |
| jet | Core/db/db.jet | 65:5 | "execute bound queue SQL on the exclusively borrowed SQLite handle; host only marshals its existing wire" |
| jet | Core/db/db.jet | 70:5 | "query bound queue SQL on the exclusively borrowed SQLite handle; host only marshals its existing wire" |
| jet | Core/email/email.jet | 77:5 | "DKIM decoding: borrow encoded bytes and keep partial output in a zeroizing Secret" |
| jet | Core/email/email.jet | 413:5 | "DKIM known-answer fixture reads only synthetic key bytes" |
| jet | Core/encoding/cbor.jet | 95:5 | "inspect private typed carriers without changing public Dyn variants" |
| jet | Core/encoding/cbor.jet | 157:5 | "validated private carriers retain byte strings for typed Encode" |
| jet | Core/encoding/cbor_stream.jet | 209:9 | "the retained key copies exactly the already validated text bytes" |
| jet | Core/encoding/cbor_stream.jet | 312:17 | "the exact CBOR text allocation passed strict UTF-8 validation" |
| jet | Core/encoding/csv.jet | 123:9 | "input is a String; original complete UTF-8 scalars and ASCII quote escapes entered the field" |
| jet | Core/encoding/csv.jet | 318:5 | "the exact owned query bytes passed the shared strict UTF-8 validator" |
| jet | Core/encoding/csv.jet | 768:9 | "each completed UTF-8 scalar was validated before its original bytes entered this field" |
| jet | Core/encoding/json.jet | 101:5 | "preserve the shipped decoder's hidden stderr effect at its existing host boundary" |
| jet | Core/encoding/json.jet | 109:5 | "preserve the shipped decoder's hidden wall-clock snapshot at its existing host boundary" |
| jet | Core/encoding/json.jet | 706:5 | "preserve the native compiler-private Dyn carriers in Core JSON event projection" |
| jet | Core/encoding/json.jet | 870:5 | "Core JSON rendering borrows native private payloads without exposing their constructors" |
| jet | Core/encoding/stream.jet | 12:9 | "the Object variant was checked immediately before borrowing its ordered backing" |
| jet | Core/encoding/stream.jet | 20:5 | "transfer the owned ordered entries allocation into the native Object carrier" |
| jet | Core/encoding/stream.jet | 26:5 | "check the private Bytes variant immediately before borrowing its backing" |
| jet | Core/encoding/stream.jet | 33:5 | "the shared parser validated this exact numeric lexeme" |
| jet | Core/encoding/stream.jet | 39:5 | "retain decoded JSON text until the target Decode contract is known" |
| jet | Core/encoding/stream.jet | 300:9 | "the shared UTF-8 validator just accepted this owned byte allocation" |
| jet | Core/encoding/stream.jet | 371:9 | "the shared UTF-8 validator just accepted this owned ASCII token" |
| jet | Core/encoding/stream.jet | 376:13 | "retain a validated number lexeme in the private event carrier until typed decoding" |
| jet | Core/encoding/stream.jet | 555:9 | "transfer already validated UTF-8 storage to inspect its actual owned capacity" |
| jet | Core/encoding/stream.jet | 566:9 | "the private numeric event is projected only into the matching typed tree carrier" |
| jet | Core/encoding/stream.jet | 836:5 | "the shared UTF-8 validator just accepted this owned allocation" |
| jet | Core/encoding/stream.jet | 845:5 | "the byte position was checked against this String's storage length" |
| jet | Core/encoding/stream.jet | 1405:5 | "the shared UTF-8 validator accepted this exact owned formatter buffer" |
| jet | Core/encoding/stream.jet | 1416:5 | "transfer the owned UTF-8 String buffer before re-spelling its shortest digits" |
| jet | Core/encoding/stream.jet | 1742:5 | "owned validated UTF-8 allocation transfers in the encoding source seam" |
| jet | Core/encoding/toml.jet | 779:5 | "inspect compiler-private Bytes and typed carriers; public Dyn variants are unchanged" |
| jet | Core/encoding/xml_engine.jet | 134:5 | "all callers iterate or check the UTF-8 byte bounds before borrowing this byte" |
| jet | Core/encoding/xml_engine.jet | 173:5 | "the caller selected a scalar-aligned window of validated UTF-8 bytes" |
| jet | Core/encoding/xml_engine.jet | 2218:5 | "attribute whitespace compaction preserves the already validated UTF-8 byte sequence" |
| jet | Core/encoding/yaml.jet | 390:5 | "inspect compiler-private Bytes and typed carriers; public Dyn variants are unchanged" |
| jet | Core/files/files.jet | 446:1 | "the compiler store passes a live FileReader and Directory, with one NUL-free plain destination name; no source pathname is reopened" |
| jet | Core/files/files.jet | 452:1 | "compiler store owns the explicit child descriptor slot and retains the held Directory until the process leaf duplicates its descriptor" |
| jet | Core/http/server.jet | 76:5 | "move the normalized response head and one-shot body into the transport carrier" |
| jet | Core/http/server.jet | 139:5 | "borrow the request's stored optional route-template metadata; no route selection or formatting policy" |
| jet | Core/jobs/jobs.jet | 130:5 | "queue preflight reads final-component metadata without following a symlink; metadata failures are ignored exactly as the SQLite provider does" |
| jet | Core/jobs/jobs.jet | 135:5 | "create the lexical parent with the existing raw recursive mkdir kernel before SQLite open" |
| jet | Core/jobs/jobs.jet | 394:13 | "match the queue's existing raw wall-clock wait without advancing controlled scheduler time" |
| jet | Core/jobs/jobs.jet | 488:5 | "publish already committed queue observations through the existing validated Devtools event/sink boundary" |
| jet | Core/jobs/jobs.jet | 503:5 | "publish source-projected queue status through the existing validated Devtools event/sink boundary" |
| jet | Core/mem/mem.jet | 34:1 | "`addr` is the address of a live, aligned `T` that stays valid for every use of the returned pointer" |
| jet | Core/mem/mem.jet | 38:1 | "`pointer` names a live, aligned, readable `T` for the whole access" |
| jet | Core/mem/mem.jet | 42:1 | "`pointer` names a live, aligned, writable `T` for the whole access" |
| jet | Core/net/net.jet | 137:5 | "checked IPv4/IPv6 octets transferred into the native address carrier" |
| jet | Core/net/net.jet | 143:5 | "read native address octets; no parsing or formatting policy in the carrier leaf" |
| jet | Core/net/net.jet | 149:5 | "read the native IP address discriminant" |
| jet | Core/net/net.jet | 192:5 | "validated IP octets, port and IPv6 numeric scope transferred into the socket carrier" |
| jet | Core/net/net.jet | 199:5 | "read socket IP representation; formatting stays in Jet" |
| jet | Core/net/net.jet | 202:5 | "read native socket port" |
| jet | Core/net/net.jet | 214:5 | "read native IPv6 socket scope identifier" |
| jet | Core/net/net.jet | 413:5 | "read the native UDP packet's socket-address carrier" |
| jet | Core/process/process.jet | 96:5 | "transfer a fully initialized Jet process plan into the temporary native ProcessSpec carrier; paired environment columns have equal length" |
| jet | Core/service/service.jet | 154:5 | "one existing registry read snapshots exact and candidate facts; no second authority or resource carrier" |
| jet | Core/service/service.jet | 264:5 | "read the existing endpoint's tree, worker and generation fields" |
| jet | Core/service/service.jet | 271:5 | "Services3081: check started before the single delivery-field write" |
| jet | Core/service/service.jet | 278:5 | "Services1415: root policy write precedes each declared group policy write" |
| jet | Core/service/service.jet | 287:5 | "Services3220: tree's exclusive write borrow covers count then clear" |
| jet | Core/service/service.jet | 296:5 | "Services3231: started and record checks precede adapter then authority writes" |
| jet | Core/service/service.jet | 316:5 | "the source exclusive tree borrow covers the same complete mutation as Rust &amp;mut; no lock or interleaving window" |
| jet | Core/service/service.jet | 347:5 | "Services4437: authority lookup, durable load, first record, then checked framing" |
| jet | Core/service/service.jet | 361:5 | "Services4493: replay existing in-memory records without issuing a store read" |
| jet | Core/service/service.jet | 396:5 | "Services6729: copy the existing optional last-upgrade field" |
| jet | Core/service/service.jet | 402:5 | "Services1153: plain field projection; rollback store remains omitted from all text views" |
| jet | Core/service/service.jet | 410:5 | "Services1265: read the existing topology fields, preserve handler insertion order" |
| jet | Core/service/service.jet | 428:5 | "Services5867: durable replay before lookup; first matching run wins" |
| jet | Core/service/service.jet | 443:5 | "Services5879: durable replay before lookup; missing or nonterminal run is Policy" |
| jet | Core/service/service.jet | 460:5 | "Services6978: project existing runtime facts; source owns counts and text" |
| jet | Core/service/service.jet | 541:5 | "Allocation only: source supplies every Services.rs initializer field at 2d9953adf:1293-1319, in native evaluation order" |
| jet | Core/service/service.jet | 616:5 | "read the final path component's OS metadata kind without opening an identity handle" |
| jet | Core/service/service.jet | 642:5 | "read the checked service tree's generation field without mutating or copying its scheduler carrier" |
| jet | Core/service/service.jet | 648:5 | "read event-log storage length on the checked service tree" |
| jet | Core/service/service.jet | 654:5 | "read dead-letter storage length on the checked service tree" |
| jet | Core/service/service.jet | 856:5 | "Services5439-5507: synchronize before validation; exact version replay precedes active-version rejection" |
| jet | Core/service/service.jet | 893:5 | "Services5656-5705: exclusive tree borrow keeps replay and append cursor decisions indivisible" |
| jet | Core/service/service.jet | 924:5 | "Services5708-5789: first recorded definition wins; replay precedes terminal checks" |
| jet | Core/service/service.jet | 977:5 | "Services5792-5867: replay consumes recorded attempts, never derives another attempt from replayed state" |
| jet | Core/service/service.jet | 1012:5 | "Services5870-5950: two replay entries advance together; identical terminal completions are idempotent" |
| jet | Core/service/service.jet | 1188:5 | "Services1546: exclusive tree write borrow preserves validation, reparent, append ordering" |
| jet | Core/service/service.jet | 1234:5 | "Services7044: counter write precedes reachable-worker selection and canonical failure policy" |
| jet | Core/service/service.jet | 1250:5 | "Services4575: source owns adapter/authority/payload/count checks and record framing; store append before field push" |
| jet | Core/service/service.jet | 1270:5 | "Services4525: host record-write leaf retains existing mutex through store write and field assignment; no guard exposed" |
| jet | Core/service/service.jet | 1302:5 | "Services2184: identity and pinned-generation checks precede authority validation" |
| jet | Core/service/service.jet | 1328:5 | "Services6081: exact directory lookup, generation, proof, authority, running, draining order" |
| jet | Core/service/service.jet | 1352:5 | "Services2558: source owns routing, message checks, worker gate, capacity error text, post-send ordering" |
| jet | Core/service/service.jet | 1451:5 | "ServiceAuthority2504: one existing locked durable read, then the owned entry's state" |
| jet | Core/service/service.jet | 1466:5 | "ServiceAuthority2517: same locked read; receipt fields and signature are source policy" |
| jet | Core/service/service.jet | 1480:5 | "ServiceAuthority2535-2647: one locked read, then exact record filtering, error order and signature validation" |
| jet | Core/service/service.jet | 1529:5 | "ServiceAuthority2650: validate/read first, then an exact zero-retention runtime and existing retry operation" |
| jet | Core/service/service.jet | 1540:5 | "ServiceAuthority1297: identity checks, existing two registry reads/captured Arc, conditional depth check, one receive" |
| jet | Core/service/service.jet | 1629:5 | "Services2864: local queued-message/idempotency matching stays source; existing cryptographic ID leaf" |
| jet | Core/service/service.jet | 1649:5 | "Services3127: no enclosing lock; source owns local deduplication/admission/rollback orchestration" |
| jet | Core/service/service.jet | 1722:5 | "Services2629: no enclosing guard; existing atomic authority operations retain their locks" |
| jet | Core/service/service.jet | 1814:5 | "Services2693-2762: retain authority requeue then mailbox-restore ordering and both failure texts" |
| jet | Core/service/service.jet | 1875:5 | "Services1464-1544: checked function item is passed through; source owns validation and every seed" |
| jet | Core/service/service.jet | 2017:5 | "Services2917-3078: exclusive tree borrow; each write follows the native order" |
| jet | Core/service/service.jet | 2132:5 | "Services3080-3100: pinned validation, group policy, one now, then weak name/tree match" |
| jet | Core/service/service.jet | 2153:5 | "Services2847-2862: validated endpoint, then deliberately weak name/tree match, channel depth" |
| jet | Core/sys/sys.jet | 371:1 | "posix umask: the caller owns the process file-creation mask and restores it when other code relies on it" |
| jet | Core/sys/sys.jet | 417:5 | "posix getpgid: inspect a process group" |
| jet | Core/sys/sys.jet | 423:5 | "posix getsid: inspect a process session" |
| jet | Core/sys/sys.jet | 428:1 | "posix getpriority: the caller vouches that `which` names this process or one it controls" |
| jet | Core/sys/sys.jet | 435:1 | "posix setpriority: the caller owns the scheduling priority of the process it changes" |
| jet | Core/sys/sys.jet | 439:1 | "posix utime: the caller owns the file whose timestamps it rewrites" |
| jet | Core/sys/sys.jet | 446:1 | "posix stop: the caller accepts that the process ends here after the shared cleanup boundary" |
| jet | Core/sys/sys.jet | 456:1 | "posix pipe: the caller owns both returned descriptors and closes each exactly once" |
| jet | Core/sys/sys.jet | 462:1 | "posix close_fd: the caller owns `fd` and nothing uses it after this call" |
| jet | Core/sys/sys.jet | 470:1 | "posix mkfifo: the caller owns `path` and removes the named pipe it creates" |
| jet | Core/sys/sys.jet | 475:1 | "posix fork: the caller handles both the parent and the child return and reaps the child" |
| jet | Core/sys/sys.jet | 479:1 | "posix setsid: the caller owns the process session topology it changes" |
| jet | Core/sys/sys.jet | 483:1 | "posix wait: the caller owns the child processes it reaps" |
| jet | Core/sys/sys.jet | 489:1 | "posix waitpid: the caller owns the child process it reaps" |
| jet | Core/sys/sys.jet | 495:1 | "posix setuid: the caller drops or changes process credentials deliberately" |
| jet | Core/sys/sys.jet | 499:1 | "posix setgid: the caller drops or changes process credentials deliberately" |
| jet | Core/sys/sys.jet | 503:1 | "posix setpgrp: the caller owns the process group topology it changes" |
| jet | Core/sys/sys.jet | 507:1 | "posix setpgid: the caller owns the process group topology it changes" |
| jet | Core/sys/sys.jet | 511:1 | "posix kill: the caller owns or is entitled to signal the target process" |
| jet | Core/sys/sys.jet | 515:1 | "posix initgroups: the caller changes supplementary groups deliberately" |
| jet | Core/testing/testing.jet | 354:5 | "the caller bounded this byte position by the String storage length" |
| jet | Core/testing/testing.jet | 375:5 | "all appended bytes came from complete UTF-8 strings or ASCII" |
| jet | Core/tui/tui.jet | 101:5 | "Core TUI reads owned encoded OS environment bytes from the current host; no UTF-8 assumption" |
| jet | Core/tui/tui.jet | 127:5 | "Core TUI samples the same stdout terminal fact consumed by terminal style/progress" |
| jet | Core/tui/tui.jet | 653:5 | "Core TUI test: normalized style words and explicit absent UI slots" |
| jet | Core/ui/host.jet | 154:5 | "Core UI capability facts come from the current retained provider, not a fabricated default" |
| jet | Core/ui/host.jet | 160:5 | "Core UI dialog crosses the current provider boundary; its typed cancellation and denial are preserved" |
| jet | Core/ui/host.jet | 166:5 | "Core UI save dialog crosses the current provider boundary with the original typed request" |
| jet | Core/ui/host.jet | 180:5 | "Core UI dialog request transfer: validated grant and explicit absent directory, filter list, and multiple-selection slot" |
| jet | Core/ui/host/accessibility.jet | 7:5 | "Core accessibility attaches metadata to the canonical node through the current retained provider" |
| jet | Core/ui/host/accessibility.jet | 16:5 | "Core accessibility projects metadata from the canonical node, never a second provider tree" |
| jet | Core/ui/host/clipboard.jet | 6:5 | "Core clipboard reads through the current retained provider; the raw host owns platform access" |
| jet | Core/ui/host/clipboard.jet | 12:5 | "Core clipboard transfers the exact UTF-8 text to the current retained provider" |
| jet | Core/ui/host/drag_drop.jet | 33:5 | "Core drag/drop consumes one typed event from the current retained provider queue" |
| jet | Core/ui/host/ime.jet | 25:5 | "Core IME consumes one typed event from the current retained provider queue" |
| jet | Core/ui/host/shortcuts.jet | 11:5 | "Core shortcut registration uses the current retained provider's canonical action registry" |
| jet | Core/ui/host/shortcuts.jet | 17:5 | "Core shortcut dispatch uses the current retained provider's canonical action registry" |
| jet | Core/ui/ui.jet | 126:5 | "Core creates the real retained GTK renderer through the registered host allocation boundary" |
| jet | Core/ui/ui.jet | 131:5 | "Core creates the real retained terminal renderer through the registered host allocation boundary" |
| jet | Core/ui/ui.jet | 136:5 | "Core creates the real retained null renderer through the registered host allocation boundary" |
| jet | Core/ui/ui.jet | 147:5 | "Core UI accessibility slot transfer: explicitly present validated metadata" |
| jet | Core/ui/ui.jet | 164:5 | "Core UI shortcut slot transfer: explicitly present validated shortcut" |
| jet | Core/ui/ui.jet | 191:5 | "Core UI host slot transfer: audited kind, role, explicit absent slots, and normalized style words" |
| jet | Core/ui/ui.jet | 197:5 | "Core UI viewport transfer: source supplies a valid device and bounded positive dimensions" |
| jet | Core/vendor/sqlite3/fixture/run.jet | 17:5 | "exclusive out-slots hold SQLite pointers; zero words mean null, SQL is NUL-terminated for each borrowed call, and each live pointer is finalized/closed once" |
| jet | Jetpack/Foundation/Source/TreeHash.jet | 724:5 | "source-store linkat uses the live held reader and directory, a validated plain name, and no source pathname; raw syscall facts are classified here" |
| jet | Jetpack/Publication/Source/Sign.jet | 58:5 | "export a fresh Core signing seed only to descriptor-relative owner-only key publication; the Core key owner is wiped on every outcome" |
| jet | Jetpack/Publication/Source/Sign.jet | 113:5 | "import the protected seed into Core signing without a second raw-secret copy; close the temporary Core Secret on success and failure" |
| jet | Jetpack/Publication/Source/Sign.jet | 137:5 | "package author signatures require Core's strict Ed25519 verification; no raw verifier or helper process" |
| jet | Jetpack/Publication/Tests/Index.jet | 266:5 | "the test owns the planted lock file it ages" |
| jet | Jetpack/Publication/Tests/Sign.jet | 164:5 | "original signing secret-erasure acceptance requires the Core volatile in-place wipe provider (#3597)" |
| jet | Jetpack/Store/Tests/ClosureGraph.jet | 59:13 | "the test owns the journal file it just wrote" |
| jet | Jetpack/Store/Tests/Seal.jet | 31:5 | "the store test owns the fixture file whose mtime it pins" |
| jet | Tools/agent-eval/domain-foundations/area-embedded/pkg/mmio_fixed.jet | 4:5 | "board.sensor_v1 GPIO register 0x40000100 is a volatile MMIO word" |
| jet | Tools/agent-eval/domain-foundations/area-embedded/pkg/run.jet | 111:5 | "host replay uses a stack cell as a stand-in for the board register" |
| jet | Tools/testkit/StepsControl.jet | 134:5 | "kill(pid, 0) observes the case-owned child without sending a signal" |
| jet | Tools/testkit/StepsSessions.jet | 116:5 | "the case explicitly authorizes signalling its own spawned child" |
| jetpack | crates/jetpack/batteries/embedded/embedded.jet | 181:9 | "host replay uses a stack cell for the CMSDK UART0 control register and never dereferences the target address" |
| tests | tests/cases/cffi_native_matrix/fixtures/required_native_c_abi_matrix/default/matrix.jet | 44:5 | "live non-null out slot; read only after status zero" |
| tests | tests/cases/cffi_native_matrix/fixtures/required_native_c_abi_matrix/sysv64/matrix.jet | 44:5 | "live non-null out slot; read only after status zero" |
| tests | tests/cases/cli_parts_core/fixtures/gate_policy/main.jet | 2:5 | "org refusal" |
| tests | tests/cases/cli_parts_inspect/fixtures/foreign/app/main.jet | 3:5 | "calls the audited dependency" |
| tests | tests/cases/cli_parts_inspect/fixtures/foreign/dep/dep.jet | 2:1 | "intentionally invalid address" |
| tests | tests/cases/cli_parts_inspect/fixtures/release/hardened/main.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| tests | tests/cases/cli_parts_inspect/fixtures/release/release/main.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| tests | tests/cases/cli_parts_inspect/fixtures/team/unsafe/main.jet | 2:5 | "audit" |
| tests | tests/cases/cli_parts_inspect/fixtures/test_hardened/hardened/main.jet | 4:5 | "test deliberately reads after arena reset" |
| tests | tests/cases/cli_parts_inspect/fixtures/test_hardened/release/main.jet | 4:5 | "test deliberately reads after arena reset" |
| tests | tests/cases/comptime_cli_parity/fixtures/tier_parity_examples_run_through_aot_jit_and_interpreter/tooling_checked_text_type/checked_text_type.jet | 20:5 | "checked text example output" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/argon2id_rfc_and_expert_known_answers_kill_argon2i_mutation.fixture/run.jet | 37:5 | "original public expert Argon2id KAT" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/expert_aes256gcm_matches_nist_cavs_vector.fixture/run.jet | 37:5 | "original published known-answer vector" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/expert_surface_matches_reference_vectors_and_bounds.fixture/run.jet | 37:5 | "original expert interoperability vectors" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/expert_xchacha20poly1305_matches_cfrg_vector.fixture/run.jet | 37:5 | "original published known-answer vector" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/jetc_v1_expert_open_accepts_only_the_pinned_grammar.fixture/run.jet | 40:5 | "original pinned JETC v1 migration grammar" |
| tests | tests/cases/gate_ledger/fixtures/gates/gates.jet | 2:5 | "test unsafe reason" |
| tests | tests/cases/output_callable/runnable_contracts_and_selection_fail_in_sema.fixture/probe3/main.jet | 2:1 | "raw boundary" |
| tests | tests/cases/pin/fixtures/library_wires_self_ref_under_unsafe_and_exposes_safe_pin_api/fixture.jet | 10:5 | "node storage is fixed for the returned pin; self_addr names this place" |
| tests | tests/cases/policy_scope/unsafe_obligations_are_checked_before_codegen.fixture/run.jet | 5:5 | "local pointer" |
| tests | tests/cases/single_use/drop_erases_no_unsafe_in_codegen.fixture/source-1.jet | 14:5 | "gone" |
| tests | tests/cases/single_use/drop_inside_unsafe_block_satisfies_single_use.fixture/source-1.jet | 15:5 | "the resource is already gone; nothing to release" |
| tests | tests/cases/single_use/drop_inside_unsafe_fn_satisfies_single_use.fixture/source-1.jet | 13:1 | "voids a freshly-acquired lock whose resource is already gone" |
| tests | tests/cases/single_use/drop_inside_unsafe_fn_satisfies_single_use.fixture/source-1.jet | 19:5 | "calling the audited voider" |
| tests | tests/cases/single_use/reuse_after_drop_is_e0121.fixture/source-1.jet | 15:5 | "done with it" |
| tests | tests/cases/tir_collections_and_methods/sentry_typed_int_store_keeps_exact_carrier.fixture/main.jet | 6:5 | "the typed pointer refers to the live local cell for these stores" |
| tests | tests/cases/tir_control_and_data/explicit_process_exit_cleanup_golden_matches_all_execution_tiers.fixture/process_exit_cleanup.jet | 20:5 | "the exit callback has no captured state" |
| tests | tests/cases/tir_custom/fixtures/from_addr_sentry_is_tier_parity/fixture.jet | 6:5 | "value is live on this stack frame and the pointer never escapes" |
| tests | tests/cases/tir_custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_alignment.jet | 4:5 | "the tuple storage is live but this raw Int is misaligned" |
| tests | tests/cases/tir_custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_expired_recursive_stack.jet | 11:5 | "the recursive address outlives its owning Jet frame" |
| tests | tests/cases/tir_custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_expired_stack.jet | 8:5 | "the returned address outlives its owning Jet frame" |
| tests | tests/cases/tir_custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_provenance.jet | 6:5 | "external address must be a live allocation" |
| tests | tests/cases/tir_custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_quarantine.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| tests | tests/cases/tir_custom/fixtures/sentry_source_policy_off_is_tier_parity/fixture.jet | 7:5 | "the local cell is live for this raw read" |
| tests | tests/cases/tir_place_window_lifetimes/borrowed_window_stays_caller_owned.fixture/main.jet | 5:5 | "the borrowed value is live in the caller" |
| tests | tests/cases/tir_place_window_lifetimes/heap_element_window_stays_allocator_owned.fixture/main.jet | 6:5 | "the list element remains heap-owned" |
| tests | tests/cases/tir_place_window_lifetimes/stack_window_uses_expiring_sentry_address.fixture/main.jet | 6:5 | "cell is live on this stack frame" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/ffi_capability_edge_uses_shared_sentry_prelude/main.jet | 7:5 | "the foreign call may write through the borrowed value" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/paren/main.jet | 2:1 | "reads through a raw pointer; addr must be a live, valid Int" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/paren/main.jet | 10:5 | "addr is the address of `cell`, a live Int on this stack frame" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/plain/main.jet | 2:1 | "reads through a raw pointer; addr must be a live, valid Int" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/plain/main.jet | 10:5 | "addr is the address of `cell`, a live Int on this stack frame" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/sentry_kernel_is_shared_by_aot_and_runtime_adapters/main.jet | 7:5 | "the local cell is live for this raw read" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/unsafe_fn_block_and_ptr_ops/main.jet | 2:1 | "reads through a raw pointer; addr must be a live, valid Int" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/unsafe_fn_block_and_ptr_ops/main.jet | 10:5 | "addr is the address of `cell`, a live Int on this stack frame" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/unsafe_tier_emit_is_byte_exact/main.jet | 2:1 | "reads through a raw pointer; addr must be valid" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/unsafe_tier_emit_is_byte_exact/main.jet | 10:5 | "safe: cell is live" |
| tests | tests/cases/tir_unsafe_and_runtime/fixtures/volatile_write_emit_is_byte_exact/main.jet | 2:1 | "UART TX register is mapped by the target machine" |
| tests | tests/cases/web_build/module_local_dev_is_validated_as_web_runtime.fixture/main.jet | 4:9 | "exercise the web TIR gate" |
| tests | tests/conformance/corpus/core/crypto/aes/nist_gcm.jet | 11:5 | "published KAT or frozen seed AES-GCM fixture" |
| tests | tests/conformance/corpus/core/crypto/aes/seed_differential.jet | 11:5 | "published KAT or frozen seed AES-GCM fixture" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 13:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 17:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 30:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 34:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 47:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 51:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 64:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 68:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 81:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 85:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 96:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 107:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 118:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 129:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 140:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 151:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 162:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 173:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 184:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 195:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 206:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 217:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 228:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 239:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 250:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 261:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 272:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 283:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 294:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 305:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 321:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 332:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 348:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 359:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 375:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 386:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 402:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 413:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 429:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 440:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 456:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 467:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 483:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 494:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 510:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 521:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 537:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 548:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 564:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 575:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 591:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 602:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 618:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 629:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 645:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 656:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 672:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 683:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 699:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 710:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 726:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 737:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 753:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 764:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 780:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 791:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 807:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 818:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 834:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 845:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 861:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 872:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 888:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 899:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 915:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 926:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 942:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 953:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 969:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 980:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 996:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1007:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1023:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1034:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1050:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1061:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1077:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1088:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1104:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1115:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1131:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1142:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1158:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1169:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1185:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1196:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1212:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1223:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1239:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1250:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1266:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1277:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1293:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1304:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1320:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1331:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1342:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1353:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1364:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1375:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1386:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1397:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1408:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1424:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1440:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1456:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1474:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1478:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1491:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1495:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1508:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1512:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1525:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1529:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1542:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1546:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1559:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1563:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1576:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1580:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1593:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1597:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1610:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1614:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1627:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1631:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1644:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1648:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1661:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1665:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1678:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1682:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1695:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1699:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1712:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1716:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1729:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1733:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1746:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1750:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1763:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1767:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1780:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1784:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1797:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1801:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1814:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1818:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1831:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1835:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1848:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1852:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1865:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1869:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1882:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1886:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1899:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1903:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1916:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1920:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1933:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1937:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1950:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1954:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1967:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1971:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1984:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1988:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2001:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2005:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2018:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2022:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2035:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2039:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2052:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2056:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2069:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2073:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2086:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2090:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2103:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2107:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2120:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2124:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2137:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2141:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2154:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2158:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2171:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2175:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2188:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2192:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2205:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2209:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2220:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2236:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_x25519.jet | 5:5 | "published/frozen X25519 test inputs" |
| tests | tests/conformance/corpus/core/crypto/expert/aes256gcm_open.jet | 8:5 | "fixed NIST AES-256-GCM vector" |
| tests | tests/conformance/corpus/core/crypto/expert/aes256gcm_seal.jet | 7:5 | "fixed NIST AES-256-GCM vector" |
| tests | tests/conformance/corpus/core/crypto/expert/argon2id.jet | 8:5 | "fixed Argon2id known-answer vector" |
| tests | tests/conformance/corpus/core/crypto/expert/ed25519_sign.jet | 6:5 | "fixed RFC 8032 Ed25519 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/ed25519_verify_strict.jet | 7:5 | "fixed RFC 8032 Ed25519 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/hkdf_sha256_raw.jet | 8:5 | "fixed RFC 5869 HKDF vector" |
| tests | tests/conformance/corpus/core/crypto/expert/migrate_v1.jet | 10:5 | "fixed JETC v1 source failure" |
| tests | tests/conformance/corpus/core/crypto/expert/open_v1.jet | 7:5 | "fixed JETC v1 AES vector" |
| tests | tests/conformance/corpus/core/crypto/expert/secret_bytes.jet | 7:5 | "inspect fixed secret bytes" |
| tests | tests/conformance/corpus/core/crypto/expert/shared_secret_bytes.jet | 9:5 | "inspect shared-secret length" |
| tests | tests/conformance/corpus/core/crypto/expert/signing_key_bytes.jet | 7:5 | "inspect generated signing-key bytes" |
| tests | tests/conformance/corpus/core/crypto/expert/x25519_raw.jet | 7:5 | "fixed RFC 7748 X25519 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/x25519_secret_bytes.jet | 7:5 | "inspect generated X25519 secret bytes" |
| tests | tests/conformance/corpus/core/crypto/expert/xchacha20poly1305_open.jet | 9:5 | "fixed CFRG XChaCha20-Poly1305 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/xchacha20poly1305_seal.jet | 9:5 | "fixed CFRG XChaCha20-Poly1305 vector" |
| tests | tests/conformance/corpus/core/crypto/vault/commit_import_signing.jet | 5:5 | "restore a fixed signing key into the vault" |
| tests | tests/conformance/corpus/core/crypto/vault/commit_import_x25519.jet | 5:5 | "restore a fixed X25519 secret into the vault" |
| tests | tests/conformance/corpus/core/crypto/vault/prepare_import_signing.jet | 5:5 | "restore a fixed signing-key vector" |
| tests | tests/conformance/corpus/core/crypto/vault/prepare_import_x25519.jet | 5:5 | "restore a fixed X25519 secret vector" |
| tests | tests/conformance/corpus/core/crypto/x25519.jet | 9:5 | "inspect shared-secret length" |
| tests | tests/conformance/corpus/core/mem/volatile_read.jet | 6:5 | "value is live on this stack frame and the pointer never escapes" |
| tests | tests/conformance/corpus/core/mem/volatile_write.jet | 6:5 | "value is live on this stack frame and the pointer never escapes" |
| tests | tests/conformance/corpus/core/sys/atexit.jet | 5:5 | "the exit callback has no captured state" |
| tests | tests/conformance/corpus/core/sys/close_fd.jet | 6:9 | "close an invalid descriptor without changing process state" |
| tests | tests/conformance/corpus/core/sys/fork.jet | 5:5 | "fork and let the child return immediately" |
| tests | tests/conformance/corpus/core/sys/getpriority.jet | 5:5 | "read the current process priority" |
| tests | tests/conformance/corpus/core/sys/initgroups.jet | 5:5 | "probe initgroups with a stable missing user" |
| tests | tests/conformance/corpus/core/sys/kill.jet | 5:5 | "check the process group without sending a signal" |
| tests | tests/conformance/corpus/core/sys/mkfifo.jet | 5:5 | "probe FIFO creation with an empty path" |
| tests | tests/conformance/corpus/core/sys/pipe.jet | 5:5 | "open a pipe and observe its descriptor pair" |
| tests | tests/conformance/corpus/core/sys/setgid.jet | 5:5 | "probe setgid with an invalid identifier" |
| tests | tests/conformance/corpus/core/sys/setpgid.jet | 5:5 | "set the current process into its existing process group" |
| tests | tests/conformance/corpus/core/sys/setpgrp.jet | 5:5 | "set the current process group" |
| tests | tests/conformance/corpus/core/sys/setpriority.jet | 5:5 | "lower the current process priority for this short-lived witness" |
| tests | tests/conformance/corpus/core/sys/setsid.jet | 5:5 | "create a session when the host permits it" |
| tests | tests/conformance/corpus/core/sys/setuid.jet | 5:5 | "probe setuid with an invalid identifier" |
| tests | tests/conformance/corpus/core/sys/stop.jet | 6:9 | "stop this dedicated witness successfully" |
| tests | tests/conformance/corpus/core/sys/umask.jet | 5:5 | "set a deterministic mask for this short-lived witness" |
| tests | tests/conformance/corpus/core/sys/utime.jet | 5:5 | "probe timestamp update with an empty path" |
| tests | tests/conformance/corpus/core/sys/wait.jet | 5:5 | "wait for a child that this witness does not have" |
| tests | tests/conformance/corpus/core/sys/waitpid.jet | 5:5 | "poll for a child that this witness does not have" |
| tests | tests/fixtures/ffi_stack4_i64.jet | 2:1 | "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return" |
| tests | tests/fixtures/ffi_stack4_i64.jet | 49:5 | "Linux getpid syscall 39 ignores all argument registers and retains no stack pointer" |
| tests | tests/fixtures/fmt_import_c_bindings.jet | 7:1 | "audited C boundary" |
| tests | tests/fixtures/fmt_import_c_bindings.jet | 10:1 | "audited C boundary" |
| tests | tests/fixtures/mir_explicit_cleanup_order.jet | 31:5 | "exercise the explicit process-stop cleanup boundary" |
| tests | tests/fixtures/unsafe_obligations/run.jet | 5:5 | "local pointer" |
| tests | tests/fuzz/sema/differential/ex_crypto_crypto_migration.jet | 26:5 | "AES-256-GCM protocol interoperability" |
| tests | tests/fuzz/sema/valid/ex_lowlevel_lowlevel.jet | 10:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| tests | tests/fuzz/sema/valid/ex_lowlevel_pointer_cast_deref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
| tests | tests/run/core_crypto_foundations/crypto_aead_tampering_is_rejected_across_tiers.jet | 16:5 | "fixed AEAD tamper vector" |
| tests | tests/ui/audited_gate_ladder_forbidden/run.jet | 3:5 | "the organization policy refuses this audited escape" |
| tests | tests/ui/cffi_out_pointer_requires_unsafe.jet | 12:1 | "the local slot remains live for the complete C call" |
| tests | tests/ui/checked_text_raw_requires_unsafe.jet | 22:5 | "audited literal" |
| tests | tests/ui/crypto_argon2id_literal_policy.jet | 7:5 | "fixed password-hash vectors" |
| tests | tests/ui/crypto_argon2id_literal_policy_type_precedence.jet | 7:5 | "fixed password-hash vector" |
| tests | tests/ui/crypto_argon2id_literal_policy_valid.jet | 7:5 | "protocol-selected password policy" |
| tests | tests/ui/crypto_argon2id_literal_policy_valid.jet | 15:5 | "fixed password-hash vectors" |
| tests | tests/ui/crypto_expert_material_length.jet | 12:5 | "fixed interop vectors" |
| tests | tests/ui/crypto_expert_material_length_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_material_length_valid.jet | 5:5 | "protocol-selected interop material" |
| tests | tests/ui/crypto_expert_material_length_valid.jet | 17:5 | "fixed interop vectors" |
| tests | tests/ui/crypto_expert_nonce_callback_bound_precedence.jet | 13:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_correct.jet | 5:5 | "fixed interop vectors" |
| tests | tests/ui/crypto_expert_nonce_effect_bound_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_effect_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_effect_prohibition_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_effect_unknown_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_length.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_region_authority_precedence.jet | 9:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_trait_dispatch_precedence.jet | 9:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_length.jet | 7:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_valid.jet | 6:5 | "protocol-selected output length" |
| tests | tests/ui/crypto_hkdf_output_valid.jet | 14:5 | "fixed interop vectors" |
| tests | tests/ui/ffi_asm_duplicate_clobber.jet | 3:1 | "each discarded register is named once" |
| tests | tests/ui/ffi_asm_float_signature.jet | 3:1 | "register-only increment" |
| tests | tests/ui/ffi_asm_missing_return_anchor.jet | 3:1 | "cycle counter" |
| tests | tests/ui/ffi_asm_no_mem.jet | 1:1 | "register-only increment" |
| tests | tests/ui/ffi_asm_return_clobber.jet | 3:1 | "the result register is overwritten by the body" |
| tests | tests/ui/ffi_asm_unknown_clobber.jet | 3:1 | "the body declares only audited registers" |
| tests | tests/ui/ffi_body_not_string.jet | 1:1 | "demo" |
| tests | tests/ui/ffi_inline_c_mismatch.jet | 1:1 | "deliberate mismatch fixture" |
| tests | tests/ui/ffi_unknown_language.jet | 1:1 | "demo" |
| tests | tests/ui/gates_load_package_ceiling/run.jet | 2:5 | "audited load-only gate" |
| tests | tests/ui/lowlevel_e3102.jet | 2:5 | "addr is valid" |
| tests | tests/ui/lowlevel_e3103.jet | 3:1 | "reads through caller-provided raw pointer" |
| tests | tests/ui/lowlevel_volatile_write_without_mem.jet | 2:5 | "addr is valid" |
| tests | tests/ui/lowlevel_volatile_write_wrong_ptr.jet | 4:5 | "plain ints are not raw pointers" |
| tests | tests/ui/lowlevel_volatile_write_wrong_value.jet | 6:5 | "slot is live and pointer never escapes" |
| tests | tests/ui/marker_group_forbidden.jet | 2:21 | "audit" |
| tests | tests/ui/secrets_expiry/expiring_secret_loan_rejects_move_variadic_and_drop_paths_3.jet | 11:9 | "attempt to discard a loan" |
| tests | tests/ui/unsafe_forbidden/run.jet | 2:5 | "not allowed" |
| tests | tests/ui/unsafe_obligation_missing.jet | 5:5 | "local pointer" |
| tests | tests/ui/unsafe_obligation_no_bleed.jet | 6:5 | "two independent pointer operations" |
| tests | tests/ui/unsafe_per_site/run.jet | 2:5 | "selection required" |
<!-- unsafe-ratchet:end -->

## CoreLib unsafe audit

The ratchet checker also audits the CoreLib (`Core/**`), independently of
sema. Two rules hold there:

- Every raw operation sits inside an audited gate: postfix dereference
  `p.*`, `Ptr<T>.from_addr`, `core.mem.volatile_read`/`volatile_write`, and
  the POSIX controls sema gates with E3101 (`core.sys.fork`, `kill`, `setuid`,
  …). The scan ignores comments and string literals.
- A CoreLib function that opens an `#Unsafe` block without being an `#Unsafe`
  function discharges the obligation for every caller. Unless it is recorded
  below as a safe abstraction with its proof, it fails: a plain public `fn`
  around a gated operation would let safe code reach that operation with no
  gate of its own. A wrapper whose caller must vouch for the operation is
  written `#Unsafe("<caller contract>") fn …` instead, so every call needs its own
  `#Unsafe` region (E3103), whether it goes through the module alias or a
  direct import. `core.mem.from_addr`, `volatile_read`, `volatile_write` and
  the `core.sys` POSIX controls are such functions.

The registry below is written by reviewers, one row per safe abstraction:
its file, function, and the proof that no caller can break memory safety
through it. The checker never writes a proof; it fails on a missing row and on
a row whose function no longer opens a gate.
Default-value literals in function parameters are part of the signature, not
the function body; they do not exempt a wrapper from the proof requirement.

<!-- unsafe-abstractions:begin -->
```json
[
  {"file": "Core/auth/auth.jet", "function": "verify_paseto", "proof": "The strict Ed25519 verifier only reads `key`, the PAE-encoded message and the 64-byte signature, all owned byte lists sliced and length-checked above; it returns a Bool and exposes no key material."},
  {"file": "Core/crypto/crypto.jet", "function": "password_hash_with_salt", "proof": "Password and salt lengths are bounded before Argon2id runs with fixed parameters (m=65536, t=3, p=1, 32-byte output); the derived key stays in a Secret that only feeds the PHC encoding."},
  {"file": "Core/crypto/crypto.jet", "function": "password_verify", "proof": "Every PHC field is parsed and bounded (memory, iterations, lanes, salt and digest lengths) before Argon2id runs; the derived key is compared in constant time and never leaves the function."},
  {"file": "Core/crypto/jetc.jet", "function": "seal", "proof": "Expert AEAD seal is safe here because the nonce is unique per (key, record): the writer owns a fresh random key and 16-byte prefix (:151-195); each record's nonce = prefix ∥ LE64(index) (:62-65), with the header at index 0 (:189) and chunks at index 1.. strictly increasing (:194-210), bounded by MAX_RECORDS, and the final state cannot be reused. AAD is a fixed domain per record kind (:73-94). Key / nonce lengths are enforced by expert (expert.jet:499-509)."},
  {"file": "Core/crypto/jetc.jet", "function": "open", "proof": "Expert AEAD open is safe here because inputs are bounded before the call (headers :215-226, record shape / count :264-269); plaintext becomes a Secret only from an authenticated open result (:270-275); the coordinator writes only after authentication (crypto.jet:652-653) and requires the final record (:277-279) before publish (crypto.jet:656-657)."},
  {"file": "Core/crypto/crypto.jet", "function": "__migrate_v1", "proof": "Reachable only from expert.migrate_v1 (expert.jet:624, the sole caller), which the checker restricts to an importing core.crypto.expert caller inside #Unsafe (CheckerCoreLib/core_call.rs:2553-2577). The legacy buffer is bounded (≤1 GiB + 34, :666-675); the plaintext is a Secret only after authentication (:676-680); the staged v2 output is verified (:919-929) before publication."},
  {"file": "Core/crypto/vault.jet", "function": "load", "proof": "`validate_key` rejects a forged or foreign key reference before the typed provider runs; the provider returns the typed key value, never raw key bytes."},
  {"file": "Core/crypto/vault.jet", "function": "current", "proof": "`validate_name` rejects an invalid vault name before the typed provider runs; the result is a typed key reference, not key material."},
  {"file": "Core/crypto/vault.jet", "function": "status", "proof": "`validate_key` rejects a forged or foreign key reference before the provider runs; the result is a KeyStatus value."},
  {"file": "Core/crypto/vault.jet", "function": "versions", "proof": "`validate_name` rejects an invalid vault name before the provider runs; the result is a list of typed key references."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_generate", "proof": "`validate_name` runs first; the provider returns a MutationPlan that only `authorize_write` and a commit can act on."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_store", "proof": "`validate_name` runs first and the key moves (`^key`) into the provider, so the caller keeps no copy; the result is a MutationPlan."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_import_signing", "proof": "`validate_name` runs and the material must be exactly 32 bytes; the bytes move (`^bytes`) into the provider, so no raw copy remains with the caller."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_import_x25519", "proof": "`validate_name` runs and the material must be exactly 32 bytes; the bytes move (`^bytes`) into the provider, so no raw copy remains with the caller."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_import_wrapped", "proof": "`validate_name` runs and the wrapped blob is bounded to 16..8192 bytes before the provider decrypts it under `unlock`; the result is a WrappedImportPlan."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_rotate", "proof": "`validate_name` runs first; the provider returns a MutationPlan that only `authorize_write` and a commit can act on."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_retire", "proof": "`validate_key` and `validate_reason` run first; the provider returns a MutationPlan, not key material."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_revoke", "proof": "`validate_key` and `validate_reason` run first; the provider returns a MutationPlan, not key material."},
  {"file": "Core/crypto/vault.jet", "function": "authorize_write", "proof": "`validate_plan` and `validate_reason` run first; the plan is lent read-only and the result is a single-use VaultWrite token."},
  {"file": "Core/crypto/vault.jet", "function": "authorize_wrapped_import", "proof": "`validate_wrapped_plan` and `validate_reason` run first; the result is a single-use VaultWrite token."},
  {"file": "Core/crypto/vault.jet", "function": "commit_generate", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/crypto/vault.jet", "function": "commit_store", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/crypto/vault.jet", "function": "commit_import_signing", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/crypto/vault.jet", "function": "commit_import_x25519", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/crypto/vault.jet", "function": "commit_import_wrapped", "proof": "The write token and wrapped plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/crypto/vault.jet", "function": "commit_rotate", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a Rotation of typed key references."},
  {"file": "Core/crypto/vault.jet", "function": "commit_retire", "proof": "The write token and plan are validated and both move into the provider, so each is used once; nothing is returned but the error."},
  {"file": "Core/crypto/vault.jet", "function": "commit_revoke", "proof": "The write token and plan are validated and both move into the provider, so each is used once; nothing is returned but the error."},
  {"file": "Core/crypto/vault.jet", "function": "export_to_passphrase", "proof": "`validate_key` runs first and the passphrase is lent read-only; the result is passphrase-encrypted wrapped bytes, never the plain key."},
  {"file": "Core/crypto/vault.jet", "function": "export_to_recipients", "proof": "`validate_key` runs and the recipient list is bounded to 1..16; the result is recipient-encrypted wrapped bytes, never the plain key."},
  {"file":"Core/email/email.jet","function":"decode_dkim","proof":"The gate reads and writes Secret.bytes, not arbitrary memory. At email.jet:78-95 a nonempty length divisible by four bounds every quartet access and canonical padding/alphabet checks reject malformed input. The only decoded allocation is the fixed 32-byte Secret at :82; :96-104 guard every write with written < 32, never grow a secret-bearing List, and :107 rejects any non-32-byte result. Errors drop the zeroizing Secret; success moves it at :108. Callers supply only a borrowed Secret and cannot choose unchecked indices or receive a plain decoded byte list."},
  {"file":"Core/data/data.jet","function":"bridge_tool_on_path","proof":"__host_path_is_file invokes OsStr::from_encoded_bytes_unchecked (Top/DataStats.rs:11-13), so its input must retain native encoded-byte provenance. data.jet:1090 obtains PATH from the current host's encoded environment; :1092-1108 split only at ASCII colon/semicolon and remove Windows ASCII quotes. :1111-1117 append only ASCII separators and valid UTF-8 tool text, preserving that host encoding. The metadata leaf borrows the owned list and returns Bool/error, not a pointer; the caller cannot supply an unrelated byte allocation to this helper or retain a host borrow."},
  {"file":"Core/data/data.jet","function":"data_loader_render","proof":"The gate at data.jet:1729 only matches the compiler-private Dyn Number, TypedText and Bytes variants. Each payload is accessed after its matching discriminant at :1730, :1734 or :1738, then read through ordinary String/List operations into the caller's exclusively borrowed output; no unchecked memory, forged carrier or raw pointer is created. Other variants leave the gate for normal recursive rendering. Any typed Dyn input remains a valid enum; callers cannot select a mismatched native payload."},
  {"file":"Core/process/process.jet","function":"cmd","proof":"The __spec_fields leaf is layout transfer into ProcessSpec, not process execution (Top/ProcessSpec.rs:6-37). process.jet:92 takes all fields from default_fields(:419-428): paired environment columns are both empty; optional cwd, stdin, timeout, resource limits, terminal and policy are accompanied by their actual presence flags at :95-104, with valid typed placeholders. The sole present numeric limit is 67108864. argv is a typed owned String list; even empty argv is deferred to checked spawn, not indexed here. No descriptor or memory address is supplied or returned, and callers cannot alter the hidden initialized field set."},
  {"file":"Core/http/server.jet","function":"response_body","proof":"The __response_parts gate at server.jet:76 transfers typed Headers and one-shot Body ownership into the response without reading the body. :72-75 normalize every caller status to 100..599 before I64 conversion; all other transport fields at :78-79 are initialized typed constants, absent slots, or empty lists. The consuming body/headers parameters at :69 prevent the caller from reusing moved stream ownership. No pointer or foreign callback is accepted; invalid status cannot reach the unchecked head transfer."},
  {"file":"Core/http/server.jet","function":"access_log_template","proof":"The gate at server.jet:139 calls __route_template with a borrowed typed HTTPRequest. Its provider Top/HTTPServer.rs:6321-6322 only clones the request's stored Option<String>; it neither dereferences caller-supplied addresses nor mutates routing state, consumes the request, or exports a borrowed native reference. Every caller therefore receives owned optional text regardless of whether route metadata was present."},
{"file":"Core/db/db.jet","function":"close","proof":"db.jet:40 consumes the Close resource before passing its private raw field to __queue_raw_close(:42). The bridge uses a u64 registry key, not a memory address (jet-pkg-model/Prelude/DB.rs:66-67): removal drops the owned connection and absent keys return false. Thus even an already-absent handle cannot dereference freed storage; no connection borrow or raw pointer escapes the consuming close."},
{"file":"Core/db/db.jet","function":"__queue_open","proof":"__queue_raw_open accepts an ordinary String path and registers the owned SQLite connection under a fresh numeric key (jet-pkg-model/Prelude/DB.rs:40-48). db.jet:49 rejects the failure sentinel zero and :50 immediately adopts the key into the Close resource; no native pointer or connection reference is returned. Arbitrary caller paths produce a resource or None, not an unchecked memory capability."},
{"file":"Core/db/db.jet","function":"__queue_begin","proof":"The gate at db.jet:54 lends driver.raw to __queue_raw_begin without transferring the Close resource. The bridge fixes the statement to BEGIN and resolves the numeric key in DB_CONNS while its map borrow is live (jet-pkg-model/Prelude/DB.rs:95-100,115-117); missing keys return false. Callers cannot supply SQL or a pointer to this operation, and the native connection reference does not escape."},
{"file":"Core/db/db.jet","function":"__queue_commit","proof":"db.jet:57 lends only the typed driver's numeric key to __queue_raw_commit. The bridge selects the fixed COMMIT statement and performs the registry lookup under a live map borrow (jet-pkg-model/Prelude/DB.rs:95-100,119-121), returning false for an absent key. No raw address or native reference reaches the caller, including after failed transaction admission."},
{"file":"Core/db/db.jet","function":"__queue_rollback","proof":"db.jet:60 lends only the typed driver's numeric key to __queue_raw_rollback. Its bridge selects the fixed ROLLBACK statement and keeps the registry borrow through execution (jet-pkg-model/Prelude/DB.rs:95-100,123-125); absence is false rather than a dangling dereference. It exposes neither a native connection nor caller-selected memory."},
{"file":"Core/db/db.jet","function":"__queue_execute","proof":"db.jet:62-64 hold the exclusive __QueueDb borrow while __queue_raw_execute marshals typed String SQL and DBValue parameters. The bridge looks up the numeric key, rejects absence, owns decoded parameters and lends them only while the prepared statement executes (jet-pkg-model/Prelude/DB.rs:183-199). No caller address is interpreted and no statement or parameter reference escapes in the owned Int/DBError result."},
{"file":"Core/db/db.jet","function":"__queue_query","proof":"db.jet:67-69 hold the exclusive __QueueDb borrow through __queue_raw_query; SQL and parameters are typed owned values, not pointers. The bridge rejects an absent registry key and keeps connection, decoded parameters and statement alive through row reads (jet-pkg-model/Prelude/DB.rs:131-151), encoding owned results. Returned String/DBValue rows contain no references into SQLite storage, so callers cannot outlive a native row borrow."},
{"file":"Core/net/net.jet","function":"ip_addr","proof":"__ip_from_bytes expects exactly four or sixteen octets. net.jet:136 accepts only parse_ip_bytes success: parse_ipv4_bytes(:960-979) checks four decimal octets <=255, and the IPv6 parser(:982-1048) checks eight 16-bit words including compression before expanding to sixteen bytes. The exact owned list moves at :138; callers cannot substitute a differently sized list after validation."},
{"file":"Core/net/net.jet","function":"ip_to_string_impl","proof":"net.jet:143-144 call __ip_bytes on a borrowed typed IPAddr; Top/NetHTTP.rs:1944-1948 discriminant-matches its native IPv4/IPv6 enum and copies exactly its initialized octets into an owned list. Formatting only reads that list. No caller-selected index, raw pointer or borrowed native storage escapes."},
{"file":"Core/net/net.jet","function":"ip_is_ipv4","proof":"__ip_is_v4 at net.jet:149 only matches the valid native IPAddr discriminant (Top/NetHTTP.rs:1951-1952). Any typed address is admitted; no payload reinterpretation, mutation or memory borrow is returned, only Bool."},
{"file":"Core/net/net.jet","function":"socket_addr_parse","proof":"At net.jet:182-189 parsing produces exactly sixteen IPv6 bytes or four IPv4 bytes, with the IPv6 length checked at :183. parse_port(:846-856) bounds decimal port to 0..65535 and parse_scope(:1144-1154) bounds scope to 0..4294967295 before :193-194 consume the octets and IP into __ip_from_bytes/__socket_from_parts. Flow is the constant zero. Callers cannot select uninitialized representation fields or overflow the U16/U32 transfer."},
{"file":"Core/net/net.jet","function":"socket_host","proof":"__socket_ip at net.jet:199 borrows a valid SocketAddr and copies its native IP enum (Top/NetHTTP.rs:2038-2039). ip_to_string_impl then reads copied octets for source formatting; the caller receives owned text, not a reference into the socket carrier. No raw address is accepted."},
{"file":"Core/net/net.jet","function":"socket_port","proof":"__socket_port at net.jet:202 borrows a typed SocketAddr and widens its initialized native u16 port to Int (Top/NetHTTP.rs:2042-2043). This scalar projection accepts every valid carrier and neither exposes a borrow nor changes socket ownership."},
{"file":"Core/net/net.jet","function":"socket_scope","proof":"__socket_scope at net.jet:215 matches the valid socket enum, returning None for IPv4 or widening the IPv6 u32 scope (Top/NetHTTP.rs:2046-2050). Source substitutes zero for None. No reinterpretation or caller-controlled pointer is involved and only an owned scalar escapes."},
{"file":"Core/net/net.jet","function":"udp_packet_addr","proof":"__udp_packet_addr at net.jet:413 borrows a typed UDPPacket and clones its stored SocketAddr (Top/NetHTTP.rs:2851-2852); the packet remains live during the projection. The return carrier owns its address rather than pointing into packet storage, so releasing the packet cannot invalidate the result."},
{"file":"Core/jobs/jobs.jet","function":"queue","proof":"The gates at jobs.jet:130-136 call __path_kind and __create_dir_all_raw with owned String paths computed by queue_default_path/files.dirname, after endpoint authority validation at :124-126. Metadata is a scalar kind query and recursive mkdir returns owned IOError; neither leaf accepts or returns memory addresses. The kind check at :131-132 rejects observed symlinks before the checked SQLite open. Path races do not create a borrowed descriptor or dangling pointer because these are independent pathname operations, not identity-handle transfers."},
{"file":"Core/jobs/jobs.jet","function":"wait","proof":"The unsafe __sleep_ms leaf casts signed milliseconds to unsigned host Duration (Top/ServiceAuthority.rs:3792-3793). jobs.jet:386-387 reject negative/oversized timeout and :393 clamps the exact delay passed at :395 to 1..25, independently of wall-clock movement. No caller can reach the cast with a negative or oversized delay; the leaf only sleeps and exports no memory."},
{"file":"Core/jobs/jobs.jet","function":"queue_publish_transition","proof":"jobs.jet:486 constructs owned JSON fields and :487 clamps negative timestamps before __publish_parts(:489). The provider Top/ServiceAuthority.rs:3772-3787 validates JetDevtoolsEvent::from_parts before publication, owns entity/fields and borrows source/kind only during the call. No native pointer, callback or mutable queue storage is transferred to a sink; caller-chosen observation text cannot escape that validation boundary."},
{"file":"Core/jobs/jobs.jet","function":"queue_publish_status","proof":"jobs.jet:499-502 assemble owned status fields/entity and nonnegative timestamp, then __publish_parts(:504) invokes the existing validating JetDevtoolsEvent::from_parts gate (Top/ServiceAuthority.rs:3779-3786). The sink gets an owned validated event rather than references into JobQueueStatus, so callers cannot make event storage outlive a borrowed source value."},
{"file":"Core/testing/testing.jet","function":"testing_byte","proof":"__string_byte performs an unchecked native byte read. Its local helper testing.jet:353-355 is reached by testing_boundary only with 0 < position < text.count_bytes()(:362-363), by testing_diff_lines with cursor < end <= length(:387-393), by testing_append_bytes over a bounded prefix(:408-409,412-457), and testing_ascii_name over 0..<count_bytes(:508-509). These establish nonnegative in-storage indices before U64 conversion; none expose the index-taking helper as a public Core member or allow the String borrow to escape."},
{"file":"Core/testing/testing.jet","function":"testing_owned_text","proof":"__bytes_into_text at testing.jet:376 requires valid UTF-8. The only callers are testing_unified_diff(:503) and testing_ascii_name(:517). Diff output appends complete String bytes or scalar-aligned prefixes chosen by testing_boundary(:362-366,423-424,450-451); marker fragments are ASCII. Name output(:508-515) appends only ASCII alphanumerics, hyphen or underscore. Each exact owned list moves into String with no mutation between construction and transfer; arbitrary caller bytes cannot reach this local helper."},
{"file":"Core/tui/tui.jet","function":"capability_env_bytes","proof":"__cap_env_bytes at tui.jet:101 borrows a String name and returns an owned optional byte list. Its provider Ui.rs:514-515 obtains var_os and consumes OsString into encoded bytes; no borrowed environment storage is returned and source does not assume UTF-8 (capability_text uses checked String.from_bytes at :112). Arbitrary names cannot manufacture a native pointer or invalidate returned storage."},
{"file":"Core/tui/tui.jet","function":"capability_stdout_terminal","proof":"The gate at tui.jet:126-127 invokes __stdout_terminal with no caller-supplied values and returns only a copied Bool host terminal fact at :129. It lends no memory, descriptor ownership or callback to the caller; host terminal presence or absence cannot change the typed result's validity."},
{"file":"Core/tui/tui.jet","function":"normalized_style_test_node","proof":"The local test helper at tui.jet:651-655 calls __node_transfer with initialized text/dimensions, fixed kind/role, an empty child list, explicitly absent accessibility slots and present style slots. Its sole caller(:659-663) obtains StyleWords from style_words(:416-421), whose three flags are bounded bits 1/2/4 and whose colors are normalized by color_words. No arbitrary numeric representation or uninitialized optional slot is passed; the returned retained UINode owns its payload."},
  {"file":"Core/encoding/cbor.jet","function":"validate","proof":"The cbor.jet:95 gate only matches Dyn's private Number/TypedText variants to reject them(:97-98), and recursively validates initialized Array/Object payloads(:99-104). Discriminant matching precedes payload access; ordered Object storage is borrowed through object_entries while value remains live. No raw address, variant transmute or payload reference escapes in the error/unit result."},
  {"file":"Core/encoding/cbor.jet","function":"encode_value","proof":"The cbor.jet:157 gate grants access to private Dyn.Bytes only after its matching branch(:171); all byte reads iterate the owned list length(:173). Object entry views are taken only in the Object branch(:179-180) and retain the value borrow throughout bounded index ordering. encode(:111) validates the tree first, rejecting internal Number/TypedText. The caller's exclusively borrowed output receives copied bytes, never native payload references."},
  {"file":"Core/encoding/cbor_stream.jet","function":"retain_key","proof":"__bytes_into_text at cbor_stream.jet:208 consumes stored, which :205 fills with exactly text.count_bytes() bytes obtained through bounds-checked stream.byte_at from a valid String. It neither truncates a scalar nor edits its encoding. Only the exact owned copy moves into the retained key; arbitrary wire bytes cannot reach this transfer."},
  {"file":"Core/encoding/cbor_stream.jet","function":"next_inner","proof":"For a CBOR text item cbor_stream.jet:303 obtains the owned payload, :306 strictly validates that exact list's UTF-8 and returns Syntax on failure, then :308 immediately moves it into __bytes_into_text. No mutation intervenes, and binary items(:304) never use the text transfer. Host/wire callers cannot bypass the validator with malformed payload bytes."},
  {"file":"Core/encoding/csv.jet","function":"close_field","proof":"__bytes_into_text at csv.jet:124 consumes the whole-value parser's field. typed_records(:213-222) obtains complete scalar byte sequences from a valid String, and TypedCSV.push(:138-200) either appends the whole sequence or replaces/removes ASCII CSV syntax; doubled quotes and CRLF are ASCII. Thus field is empty or concatenated complete UTF-8 scalars at every close; it is moved and reset before reuse(:124-126), never exposed as arbitrary caller bytes."},
  {"file":"Core/encoding/csv.jet","function":"query_text","proof":"csv.jet:315-316 enforce the bounded read's size/list limits and :317 validates sample.bytes with the strict shared UTF-8 validator. The gate at :318-319 immediately moves that same owned list into __bytes_into_text; failures return FieldError before conversion. A caller's arbitrary file contents cannot produce an invalid String or retain a native read-buffer borrow."},
  {"file":"Core/encoding/csv.jet","function":"finish_field","proof":"CSVReader.character(:725-744) validates lead width, all continuation bytes, overlong/surrogate/out-of-range cases before next_inner(:880-882) calls push. push(:798-874) appends each entire validated scalar or only ASCII quote/CRLF escapes. Consequently finish_field's __bytes_into_text(:769) moves only complete UTF-8 storage, then resets field(:771). Partial/invalid wire scalars fail without reaching this gate."},
  {"file":"Core/encoding/json.jet","function":"emit_coercion","proof":"json.jet:101-102 calls __coerce_emit with ordinary borrowed String text; it supplies no pointer, descriptor or callback. The reserved leaf preserves the existing stderr emission boundary, and the helper returns no borrowed storage. Caller-selected message bytes remain valid UTF-8 and cannot become an unchecked address or outlive the call as a native reference."},
  {"file":"Core/encoding/json.jet","function":"coerce_line","proof":"The gate at json.jet:109-110 calls __coerce_now with no arguments and assigns its owned Int clock snapshot. Subsequent interpolation(:112) is safe String construction. No caller can choose a raw memory operand or receive a host borrow through this parameterless scalar query."},
  {"file":"Core/encoding/json.jet","function":"append_events","proof":"json.jet:707-709 match each private Dyn.Number/TypedText/Bytes discriminant before projecting its initialized String/list into the corresponding typed DataEvent. Only owned enum payloads are returned; no mismatched variant access, pointer formation or borrowed backing view escapes. Public variants are recursively handled outside the gate(:712-735)."},
  {"file":"Core/encoding/json.jet","function":"write_json","proof":"json.jet:855-870 discriminate private Number/TypedText/Bytes before reading each payload. Number/text are ordinary String inputs to output helpers; Bytes are read only over 0..<bytes.len()(:865-868). The caller holds an exclusive output-list borrow, which receives copied encoding bytes; no native storage reference or unchecked index is returned. Other variants leave the gate for normal recursive rendering."},
  {"file":"Core/encoding/stream.jet","function":"object_entries","proof":"__object_entries_view requires an Object and returns a borrowed ordered backing view. stream.jet:11 checks that exact discriminant immediately before :13; other variants panic before the intrinsic. The result's `from value` lifetime at :10 ties the view to the same borrowed Dyn, so callers cannot keep it after releasing its owner or reinterpret another payload."},
  {"file":"Core/encoding/stream.jet","function":"object_from_entries","proof":"__object_from_entries at stream.jet:21 receives a consuming list of typed String/Dyn entries, moving its sole allocation into an initialized Object carrier. No pointer, length or discriminant is supplied separately, and the caller cannot reuse the moved list. Arbitrary duplicate keys affect encoding policy, not the validity or ownership of the native payload."},
  {"file":"Core/encoding/stream.jet","function":"bytes_payload","proof":"stream.jet:27 checks private Dyn.Bytes before __bytes_view on the same value; all other variants panic(:29). The View<U8> result is declared `from value`(:25), retaining the owner borrow rather than exporting an independent pointer. Callers cannot select a different variant's backing or use the view after the carrier's lifetime."},
  {"file":"Core/encoding/stream.jet","function":"typed_number","proof":"The gate at stream.jet:33-34 constructs the compiler-private Number enum from a consuming typed String; it does not reinterpret memory. Its source caller json.jet:562 is in Parser.number after numeric grammar validation and checked String construction(:561). The owned lexeme is retained for typed decoding, never used as an address or unchecked index; consumers use normal checked numeric parsing. Callers cannot corrupt enum layout or retain the moved String's storage."},
  {"file":"Core/encoding/stream.jet","function":"typed_string","proof":"__typed_text at stream.jet:40 consumes an initialized String into the private typed-text carrier. UTF-8 is already guaranteed by the String type; this operation neither changes its bytes nor accepts a separately forgeable pointer/length. The caller loses ownership of the moved text and receives an owning Dyn enum, not an escaping borrow."},
  {"file":"Core/encoding/stream.jet","function":"read_string","proof":"stream.jet:294 validates the exact decoded byte list, returning Syntax and releasing tracked capacity on failure(:295-296). The __bytes_into_text gate(:299-300) immediately consumes that same list without intervening mutation. Escaped or raw wire bytes therefore cannot bypass UTF-8 validation or expose a dangling parser buffer."},
  {"file":"Core/encoding/stream.jet","function":"number","proof":"Both gates discharge separate obligations: stream.jet:360 rejects invalid JSON numeric grammar and :365 rejects invalid UTF-8 before :371 consumes that exact byte list into String. Only after those checks does :375-376 construct DataEvent.Number when typed_numbers is enabled. The valid initialized lexeme moves into its matching carrier; no unchecked numeric payload reinterpretation or raw buffer reference reaches callers."},
  {"file":"Core/encoding/stream.jet","function":"fold_text","proof":"stream.jet:555 consumes a valid String into __text_into_bytes solely to measure allocation capacity. That byte list is not modified; :557 can fail before reconstruction, otherwise :559 immediately moves the unchanged UTF-8 list into __bytes_into_text. Budget checks affect retention, not validity; no caller receives the intermediate storage or can mutate it between conversions."},
  {"file":"Core/encoding/stream.jet","function":"fold","proof":"stream.jet:566 checks the private DataEvent.Number discriminant before moving its initialized String through fold_text(:567) into matching Dyn.Number(:568). fold_text preserves UTF-8 while accounting allocation ownership. No other event variant is interpreted as a number; recursion and ordinary event handling occur outside the gate and no native payload borrow escapes."},
  {"file":"Core/encoding/stream.jet","function":"text_from_bytes","proof":"stream.jet:834 strictly validates the caller's exact owned byte list, returning EncodingError on failure; :836 then immediately moves it into __bytes_into_text. No mutation or alternate allocation occurs between validation and transfer. Even arbitrary external byte input cannot return a String violating UTF-8 or retain a pointer into the moved list."},
  {"file":"Core/encoding/stream.jet","function":"byte_at","proof":"The unchecked __string_byte at stream.jet:845 is dominated by :841-842, which reject index < 0 or index >= text.count_bytes() before U64 conversion. The borrowed String remains live for the one-byte read; only copied U8 returns. Callers cannot cause a negative, out-of-storage or dangling pointer read through this helper."},
  {"file":"Core/encoding/stream.jet","function":"ascii_text","proof":"Despite its formatter-oriented name, ascii_text admits no unchecked caller assumption: stream.jet:1402 runs strict __utf8_validate on the exact owned list and panics on invalid UTF-8. Only the succeeding path at :1404-1405 consumes it into __bytes_into_text, with no intervening write. Malformed formatter output cannot create an invalid String."},
  {"file":"Core/encoding/stream.jet","function":"wire_float","proof":"The gate at stream.jet:1415-1416 only transfers the owned valid String produced by Float interpolation(:1413) into __text_into_bytes. It is the safe direction of the UTF-8 transfer and accepts no raw bytes or caller pointer. Later digit editing uses a separate list and checked ascii_text reconstruction; no alias to the moved String is retained."},
  {"file":"Core/encoding/toml.jet","function":"toml_leaf","proof":"The gate at toml.jet:718 permits private carrier matching, not raw memory. Number/TypedText are explicitly rejected(:724); Bytes are read after their discriminant and within list bounds(:726-731). Arrays recurse on typed values and Objects use object_entries only in their matching branch(:742-745), keeping the owner live. The return is owned formatted text, never a native payload borrow."},
  {"file":"Core/encoding/xml_engine.jet","function":"text_byte","proof":"__string_byte at xml_engine.jet:135 is an unchecked byte read. Its only local callers are byte_prefix_text(:139-142) and unit_prefix(:2029-2032), both iterating 0..<needle.count_bytes(), and ascii_equal(:1386-1389), which first checks equal String storage lengths then iterates 0..<left.count_bytes(). Every index is nonnegative and below the corresponding borrowed String's byte length; no pointer or view escapes the copied U8 result."},
  {"file":"Core/encoding/xml_engine.jet","function":"byte_text","proof":"__bytes_into_text at xml_engine.jet:174 consumes a copy of source[start..<end]. Every local caller slices bytes of an existing String (DOCTYPE/entity/declaration helpers) or Scanner.source, whose sole construction path parse_document_with(:667-669) takes String.bytes(). Boundaries are zero/end or offsets adjacent to ASCII XML delimiters, quotes and whitespace, which cannot lie inside a UTF-8 continuation sequence. Checked list accesses(:172) prevent out-of-storage copies; the exact copied scalar-aligned allocation moves with no intervening mutation or escaping borrow."},
  {"file":"Core/encoding/xml_engine.jet","function":"normalize_attribute_text","proof":"xml_engine.jet:2204 starts with bytes of a valid String. The in-place loop(:2207-2215) replaces only ASCII CR/LF/tab with ASCII space and removes the ASCII LF following CR; all non-ASCII bytes keep their order and membership. write never exceeds read, and :2217 truncates only the finished compacted sequence. Thus __bytes_into_text(:2219) consumes valid complete UTF-8 for every caller text, not an assumed-valid wire allocation."},
  {"file":"Core/encoding/yaml.jet","function":"yaml_leaf","proof":"yaml.jet:320 grants private carrier matching. Number/TypedText explicitly panic(:326); Bytes are read only after their matching discriminant over 0..<bytes.len()(:328-333). Object payload views are obtained only in the Object branch(:344-347), retaining the owner; arrays recurse on typed values. The caller receives an owned String and cannot request a mismatched payload or retain native storage references."},
  {"file":"Core/service/service.jet","function":"__authority_validate","proof":"service.jet:155 projects owned identity fields; :156 rejects invalid identity before __endpoint_validation_snapshot(:160). Its native ServiceAuthority.rs:360-368 keeps the registry mutex across exact/candidate lookup and copies only String/scalar facts. No registry guard, endpoint pointer or mutable reference escapes; policy checks operate on owned snapshots, so caller identity cannot extend a native borrow."},
  {"file":"Core/service/service.jet","function":"endpoint_show","proof":"__endpoint_parts at service.jet:265 only clones the initialized endpoint tree/worker Strings and widens generation (Services.rs:3273-3274). Interpolation(:266) returns owned text. Any typed endpoint is valid for this projection; no unchecked index or native reference reaches callers."},
  {"file":"Core/service/service.jet","function":"set_delivery","proof":"service.jet:270 holds an exclusive tree borrow, :272 rejects a started tree, and :273 writes only a typed ServiceDelivery field via __tree_write_delivery. Services.rs:3259-3260 performs a normal field assignment under that borrow; no pointer or secondary tree alias is returned. Callers cannot change the validated tree concurrently with the transfer."},
  {"file":"Core/service/service.jet","function":"set_restart","proof":"service.jet:277 lends the tree exclusively; :279 checks not started before __tree_write_restart. __group_write_restart is indexed only by 0..<__tree_group_count(:281-282), while neither write changes group storage length (Services.rs:3258-3266). Every transferred policy is a typed enum, and no group reference escapes to invalidate that count."},
  {"file":"Core/service/service.jet","function":"drain_dead_letters","proof":"service.jet:286 holds the exclusive tree borrow and :288 checks started before counting and clearing the same dead-letter list(:289-290). Native __tree_dead_letter_count and __tree_clear_dead_letters(Services.rs:3249-3250,3268) return a copied length and clear owned storage without exporting an element borrow. Callers cannot hold a conflicting live list view across the clear."},
  {"file":"Core/service/service.jet","function":"set_state_empty","proof":"service.jet:295 retains an exclusive tree borrow. :297-300 reject a started tree or existing snapshot/events before __tree_write_empty_adapter and __tree_clear_state_authority(:301-302), whose provider(Services.rs:3269-3272) only assigns typed fields. No stored payload reference is returned; callers cannot interleave a state-record insertion between check and replacement."},
  {"file":"Core/service/service.jet","function":"select_state_adapter","proof":"The exclusive tree borrow at service.jet:315 covers :317-326 checks for not-started, no records, recognized migration, bounded visible store/schema and positive version before __tree_write_state_adapter/__tree_write_state_authority(:327-328). Their provider(Services.rs:3277-3284) initializes owning optional fields from typed String/scalars, not raw memory. The caller cannot replace a checked field set or retain an alias into the new authority."},
  {"file":"Core/service/service.jet","function":"restore_snapshot","proof":"__state_load expects a present state authority (Services.rs:3292-3295). service.jet:348 checks that exact option before :349, borrows the live typed tree, and :355 checks nonempty records before reading records[0]. read_state_record(:374-392) checks schema, framing, byte length and UTF-8. Returned snapshot text is owned, not a reference into tree/store storage."},
  {"file":"Core/service/service.jet","function":"replay_events","proof":"service.jet:362-364 call __tree_state_authority/__tree_event_records only as borrowed field projections; native Services.rs:3286-3290,3297 copy optional Strings and the event list. Each copied record passes checked framing(:367) before join. No list-element reference or native store handle is exported, so releasing or later editing the tree cannot invalidate the owned replay text."},
  {"file":"Core/service/service.jet","function":"upgrade_receipt","proof":"__tree_upgrade_receipt at service.jet:397 clones the existing optional typed receipt (Services.rs:3425). Source rejects None before returning the owning value. No native reference, rollback-store pointer or unchecked payload access escapes for any typed tree."},
  {"file":"Core/service/service.jet","function":"__upgrade_receipt_display","proof":"__upgrade_receipt_parts at service.jet:403 projects initialized scalar fields and cloned String/pinned-shard list (Services.rs:3426-3430), omitting rollback storage. :404-405 only format those owned fields. Caller-supplied typed receipts cannot choose a memory address or retain a borrow into the native receipt."},
  {"file":"Core/service/service.jet","function":"tree_show","proof":"__tree_display_parts at service.jet:411 copies the live tree's initialized fields and handler-name pairs (Services.rs:3432-3439); it exports neither worker tasks nor mailbox storage. :413 iterates the returned owned list and matches typed restart/delivery enums for text. No native payload borrow or separately supplied index reaches the caller."},
  {"file":"Core/service/service.jet","function":"workflow_history","proof":"__workflow_replay_parts at service.jet:429 uses the checked native workflow authority/replay and collects owned run-id/history/outcome records (Services.rs:3441-3445), propagating errors. :435-438 only search those copied records and join the matching list. Arbitrary run_id cannot be used as a native vector index or expose a replay guard."},
  {"file":"Core/service/service.jet","function":"workflow_outcome","proof":"service.jet:444 obtains owned records through __workflow_replay_parts, whose provider validates authority and collects owned payloads (Services.rs:3441-3445). :450-455 match run_id and require a present typed outcome rather than indexing with caller input. The result owns its panic text/enum payload and contains no reference into mutable workflow storage."},
  {"file":"Core/service/service.jet","function":"observe","proof":"__tree_observation_parts at service.jet:461 is a read-only projection into copied counts/booleans and typed workflow status codes (Services.rs:3447-3463). The local loop(:470-478) reads that owned list and formats text; no task, channel, mutable view or raw address is exposed. Every typed tree therefore remains valid regardless of caller observation timing."},
  {"file":"Core/service/service.jet","function":"tree","proof":"__tree_allocate at service.jet:542 materializes every native initializer field from typed arguments. :543-566 provide generation1, typed restart/delivery, positive fixed budget, empty initialized worker/group/record collections, false started, Empty state, absent authorities/receipts and a fresh Arc task group from __tree_task_group_new. Services.rs:1327-1352 only allocate/assign those fields. The caller supplies only owned name text, not uninitialized carriers or native pointers."},
  {"file":"Core/service/service.jet","function":"state_path_kind","proof":"__path_kind at service.jet:617 accepts a borrowed valid String path and returns copied metadata kind or owned IOError. It performs a final-component pathname query, not a descriptor/identity ownership transfer; no caller pointer or reference into OS metadata is returned. A path race can alter the metadata result but cannot invalidate a caller memory borrow."},
  {"file":"Core/service/service.jet","function":"directory_generation","proof":"service.jet:643 calls __tree_generation on a borrowed typed tree; Services.rs:6272-6273 only copy its initialized i64 generation into an owned Int. The caller supplies no index/address, and no scheduler or tree reference escapes this scalar projection."},
  {"file":"Core/service/service.jet","function":"event_count","proof":"__tree_event_count at service.jet:649 only reads the live tree's event_log length (Services.rs:4735-4736), returning an owned scalar. No record element, raw backing pointer or mutable borrow is exposed, so callers cannot outlive or invalidate event storage via this query."},
  {"file":"Core/service/service.jet","function":"dead_letter_count","proof":"__tree_dead_letter_count at service.jet:655 borrows the typed tree and reads its initialized owned list length (Services.rs:3249-3250). The result is a copied Int with no native payload reference or caller-controlled address; subsequent clear/drop cannot invalidate it."},
  {"file":"Core/service/service.jet","function":"workflow_start","proof":"service.jet:855 retains an exclusive tree borrow through workflow_prepare(:857), identity/version validation(:861-862), __workflow_count-bounded index loops(:864,872) and cursor/handle updates. __workflow_append receives a framed String only after run-id overflow rejection(:879-881); post-append cursor selection re-reads count(:885). Provider field/index leaves(Services.rs:7244-7258) use normal bounds-checked Vec access and owned projections, never reinterpret an id as a pointer. No mutable workflow view escapes."},
  {"file":"Core/service/service.jet","function":"workflow_step","proof":"The exclusive tree borrow at service.jet:892 covers sync(:894), bounded deterministic step checks(:898-902), and workflow_find(:903), which searches 0..<__workflow_count(:1061-1064). Cursor/history leaves operate only on that selected index; __workflow_event_at uses optional checked history.get (Services.rs:7253). :913-915 reject terminal/oversized history before framed append. No caller run_id becomes an unchecked address or borrowed history reference."},
  {"file":"Core/service/service.jet","function":"workflow_activity","proof":"service.jet:923 exclusively borrows the tree; workflow_prepare and workflow_token checks(:925-935) precede bounded max_attempts(:937) and count-bounded workflow_find(:938). Activity-definition helpers iterate __workflow_steps_len and parse field counts(:1114-1121), not caller indices. Native index leaves use checked Vec access (Services.rs:7244-7258); :965-967 reject terminal state before framed append. Returned TaskStatus is copied, with no workflow/storage borrow escaping."},
  {"file":"Core/service/service.jet","function":"workflow_activity_retry","proof":"service.jet:976 holds the exclusive tree borrow across sync, bounded key checks and workflow_find(:978-986). The selected activity definition is owned; :989-998 validate outcome, replay agreement, terminal state and retry bound before computing the next attempt and __workflow_append(:1000-1002). History lookups are optional checked projections and index leaves use normal Vec bounds (Services.rs:7253-7258). Callers cannot forge a vector pointer or retain mutable activity storage."},
  {"file":"Core/service/service.jet","function":"workflow_activity_complete","proof":"service.jet:1011 exclusively borrows the tree through sync, key/outcome checks and count-bounded workflow_find(:1013-1023). The two replay entries(:1027-1028) use __workflow_event_at's checked optional history.get (Services.rs:7253); only matching initialized events advance the cursor(:1029-1030). :1038-1047 reject conflicting completion before framed append. The return is an owned typed outcome; no raw event reference or unchecked caller index escapes."},
  {"file":"Core/service/service.jet","function":"group","proof":"service.jet:1189 rejects started state and :1190-1194 validate_group against owned topology facts before __group_reparent(:1196). validate_group(:1205-1230) proves every named worker exists and membership is unique, discharging the provider's expect at Services.rs:3310. Reparent and __group_push mutate only typed tree/task storage, with no worker reference escaping to caller code between validation and use."},
  {"file":"Core/service/service.jet","function":"chaos_fail","proof":"service.jet:1235 rejects unstarted trees and :1237 bounds the __tree_write_chaos_count increment before :1239. __first_running_endpoint(:1240) returns an owned optional endpoint (Services.rs:3302-3303), not a vector index or pointer, and fail_worker performs endpoint validation. Only a copied count returns; arbitrary caller state cannot overflow the native counter or obtain a mutable worker borrow."},
  {"file":"Core/service/service.jet","function":"append_event","proof":"__event_store_append requires a present authority (Services.rs:3316-3317). service.jet:1251-1255 establish started EventLog state, present authority, bounded valid payload and record-count bound before constructing the owned framed record(:1256). The durable append(:1257) must succeed before __tree_event_push(:1261); the native leaf owns its inserted String. No reference into storage or independently supplied memory length escapes."},
  {"file":"Core/service/service.jet","function":"commit_snapshot","proof":"service.jet:1267-1270 establish started Snapshot state, present authority and bounded valid payload before framing and __snapshot_store_write(:1271-1272). Native Services.rs:3320-3326 holds the authority mutex across store write and tree.snapshot assignment and owns the record String. Callers cannot bypass the present-option precondition, interpose a write inside the leaf, or receive a reference into replaced snapshot storage."},
  {"file":"Core/service/service.jet","function":"validate_tree_endpoint","proof":"service.jet:1295-1296 borrow typed tree/endpoint only to obtain owned route/identity parts (Services.rs:3328-3336; ServiceAuthority.rs:351-355). :1297-1301 enforce matching authority/tree and permitted generation before locked __authority_validate. Neither caller identity nor generation is interpreted as a pointer/index, and no registry/tree borrow escapes this error/unit result."},
  {"file":"Core/service/service.jet","function":"directory_resolve","proof":"__directory_entry at service.jet:1321 finds by String and clones an optional entry (Services.rs:3338-3339), rejecting absence. :1324-1335 check generation, signed directory proof, authority and live worker before returning its owned endpoint. __worker_route searches typed identity and returns owned facts, not a worker reference. An arbitrary name or stale endpoint cannot turn into an unchecked payload index or escaping native borrow."},
  {"file":"Core/service/service.jet","function":"send","proof":"service.jet:1345-1364 check started state, endpoint authority/generation, bounded message and live nondraining worker before __authority_try_send. That operation receives typed owned text/endpoint, not caller addresses. Post-send __worker_sent_account(:1374) uses only the index returned by a fresh __worker_route on the same tree (Services.rs:3344-3352); native Vec indexing is bounds checked and task writes retain their lock. No mailbox or task reference escapes to the caller."},
  {"file":"Core/service/service.jet","function":"delivery_status","proof":"__delivery_read at service.jet:1444 obtains the checked durable entry and materializes owned __DeliveryFacts (ServiceAuthority.rs:2727-2744). Failure returns before any field use; :1448 returns only the typed state enum. Callers cannot choose a log-memory address or outlive a registry/log lock through this projection."},
  {"file":"Core/service/service.jet","function":"delivery_receipt","proof":"service.jet:1459 first obtains checked owning __DeliveryFacts through __delivery_read(ServiceAuthority.rs:2727-2744). :1463-1465 require successful signature derivation before :1467 constructs a fully initialized source receipt from its owned typed fields. No native entry reference or lock guard is exposed, and invalid delivery identity/authority returns an error before a receipt is emitted."},
  {"file":"Core/service/service.jet","function":"delivery_events","proof":"__delivery_read(true) at service.jet:1473 returns owned record fields, not a borrowed log buffer (ServiceAuthority.rs:2727-2743). Every indexed record access is dominated by the exact fields.len()==10 or ==6 checks(:1480,1494); counters parse through checked workflow_i64, signatures verify before acceptance(:1491-1493,1505-1510), and :1514 guards events[0]. Malformed caller/log data cannot access an absent field or escape a mutable durable-entry borrow."},
  {"file":"Core/service/service.jet","function":"delivery_retry","proof":"service.jet:1522 invokes __delivery_probe, whose native ServiceAuthority.rs:2746-2747 checks the durable entry before success. :1526 materializes a typed runtime from the delivery's cloned store and constant zero retention, then __runtime_retry receives typed carriers only. The returned Delivery owns its identity fields; no log reference, pointer or guard escapes, including on retry errors."},
  {"file":"Core/service/service.jet","function":"endpoint_receive","proof":"service.jet:1533-1537 validate owned endpoint identity before __endpoint_mailbox. Native ServiceAuthority.rs:2751-2776 captures the channel in an owning Arc and returns initialized started/draining facts, so releasing the registry lock cannot dangle the mailbox. :1542 enforces lifecycle/depth policy before one __mailbox_receive; :1544-1547 decode its status into owned message/error. No caller can supply a raw channel address or receive a borrowed queue element."},
  {"file":"Core/service/service.jet","function":"local_delivery_ids","proof":"service.jet:1622 requires successful __worker_route before __mailbox_snapshot(:1623), using only the returned valid worker index. Native Services.rs:3362-3366 copies messages/idempotency records into owned lists; :1627-1634 match initialized identity/message fields before __delivery_id with a fully initialized zero-retention runtime. No pointer or mutable mailbox/record view is exported and arbitrary endpoint text is never used directly as an index."},
  {"file":"Core/service/service.jet","function":"send_durable","proof":"service.jet:1642-1658 check delivery mode, state authority, bounded key/message, started state and endpoint routing before __runtime_send. Mailbox snapshot/restore indices come only from __worker_named_index(:1676,1693), which searches current workers (Services.rs:3368-3369); native restores use checked Vec access(:3378-3379). Each authority operation retains its own locks and returns owning Delivery/String facts; mailbox replacement consumes its owned message list, never exporting channel references. Dead-letter/idempotency pushes are length bounded(:1683,1702)."},
  {"file":"Core/service/service.jet","function":"receive","proof":"service.jet:1715-1726 validate tree lifecycle/endpoint before selecting a worker. Capacity bounds :1728-1729 dominate __take_pending; each mailbox operation(:1751-1752,1774-1783) uses the index returned by a fresh __worker_route, and rollback reselects target(:1787-1790). Native Services.rs:3362-3423 use owned snapshots, checked Vec indexing and locked/Arc channel operations, not unchecked memory. :1780-1782 admit only a successful owned message; no queue element or task guard escapes to callers."},
  {"file":"Core/service/service.jet","function":"pending_delivery_rollback","proof":"The gate at service.jet:1806 passes typed endpoint/owned pending records to __requeue_pending, then an owned prior message snapshot to __mailbox_restore in both result branches(:1809,1815). Its local callers in receive(:1759,1762) pass the index obtained from __worker_route, never caller text as an address; native Services.rs:3378-3379 still bounds-check the Vec access. Both operations own/copy their inputs under existing locks and return errors, not references into restored mailbox storage."},
  {"file":"Core/service/service.jet","function":"worker","proof":"service.jet:1868-1876 reject started state, capacity outside1..1000000, invalid names, excess workers and duplicates before allocation. __endpoint_unchecked receives route fields/issued authority(:1878-1893), and __worker_mailbox_new receives bounded capacity plus an empty owned queue(:1894). :1906-1914 initialize task/topology and every __worker_push field; the mailbox moves rather than cloning its forkable channel. The handler is a typed checked function item, not a raw caller address; native Services.rs:7289-7305 only store initialized owned fields."},
  {"file":"Core/service/service.jet","function":"fail_worker","proof":"service.jet:2008 holds an exclusive tree borrow and :2010-2014 validate lifecycle/endpoint before obtaining owning topology/worker facts. Target indices derive from worker_scope/worker_index(:2019-2031), group/supervisor accesses are explicitly bounded(:2023,2092,2109), and previous/desired lists are constructed for exactly facts.workers.len()(:2059-2067). Native writes(Services.rs:7308-7329) use checked Vec indexing; task partition writes hold one mutex(:7339-7341). Replacement mailboxes own their queues and move at :2103, so close/restart cannot expose dangling channel storage."},
  {"file":"Core/service/service.jet","function":"restarts","proof":"service.jet:2125 validates the endpoint before owned topology/worker facts. :2134 checks supervisor against facts.groups.len() before selecting budget; :2136 iterates worker facts and matches tree/name before reading that worker's owning restart list. Providers preserve worker insertion order (Services.rs:3305-3307,7269-7277), and accesses are normal bounds-checked projections. The result is a copied count, not a restart-list or task reference."},
  {"file":"Core/service/service.jet","function":"mailbox_depth","proof":"service.jet:2146 validates the endpoint, then :2151-2155 iterate owning topology/worker facts and call __worker_mailbox_depth only with the matching in-range worker index. Native Services.rs:7328-7329 bounds-check workers and copies the live Arc channel's depth. No channel pointer/view or mutable mailbox reference reaches caller code, even for stale or unknown endpoints which return errors."},
  {"file":"Core/ui/host.jet","function":"transfer_request","proof":"__request_transfer requires kind 0/1 and explicit absent-directory state (Core/HostServices.rs:2135-2152). The local helper's only callers host.jet:172,176 supply exactly those constants, true directory_absent, an empty typed filter list and false multiple. The typed UIFSGrant is borrowed/cloned into an owning request, not interpreted as a memory address; title is valid String text. No current-host scope is accessed by this layout-only leaf and callers cannot supply unchecked tags to the local helper."},
  {"file":"Core/ui/host.jet","function":"capabilities","proof":"The host.jet:154-155 __capabilities gate dispatches to jet_ui_host_capabilities (Core/HostServices.rs:1838-1840), which queries capability_facts through jet_ui_with_current_host. In the repaired scope (7ee5e69a8, :1786-1820), only jet_ui_with_host owns the private lifetime-bound guard: TLS stores the address of its live stack-held host reference without transmute, and Drop restores the predecessor before that reference/host borrow ends, including unwinding; safe callers cannot obtain or forget the guard. :1822-1834 hold the mutable TLS borrow across the callback, refusing recursive access and nested installation before another host reference is made; the default host also stays mutably borrowed. No caller address is accepted and the result is copied capability bits (:74-76), not a host reference."},
  {"file":"Core/ui/host.jet","function":"open_file","proof":"host.jet:160-161 moves the typed request into __open_file, whose jet_ui_host_open_file provider (Core/HostServices.rs:1841-1845) calls jet_ui_open_file under jet_ui_with_current_host. The private lifetime-bound scope at :1786-1820 is owned only by jet_ui_with_host, keeps its stack-held host reference live without transmute, and restores predecessor TLS before the frame/host borrow ends on return or unwind; safe callers cannot leak its guard. The mutable TLS borrow at :1822-1834 stays held throughout the provider, so re-entry or nested installation fails before minting another host reference (the default host is borrowed too). Ui.rs:217-227 retain a request clone and validate the returned selection against it before success. Caller requests contain typed owned values, not host addresses; the result owns granted paths (:1348-1350) and exports no scoped host borrow."},
  {"file":"Core/ui/host.jet","function":"save_file","proof":"host.jet:166-167 moves its typed request into __save_file; jet_ui_host_save_file (Core/HostServices.rs:1846-1850) invokes jet_ui_save_file under jet_ui_with_current_host. :1786-1820 confine the lifetime-bound guard to jet_ui_with_host, store only its live stack-held host-reference address without transmute, and restore predecessor TLS before that frame/host borrow ends, including unwind; safe callers never receive a forgettable guard. :1822-1834 keep the mutable TLS borrow across the call, rejecting recursive access or nested installation before a second host reference exists, with a borrowed default host when none is installed. Ui.rs:231-241 retain the original request clone for validating the returned save selection. Arbitrary typed requests cannot supply a raw host pointer, and successful selections own their granted paths rather than borrowing host storage."},
  {"file":"Core/ui/host/accessibility.jet","function":"attach","proof":"accessibility.jet:7-11 consumes typed node/metadata via __attach. Its jet_ui_host_attach_accessibility provider (Ui.rs:283-293) owns the node, lends it exclusively to host.attach_accessibility only inside jet_ui_with_current_host, and returns the owned node or error; the source discards the successful node and returns unit. Core/HostServices.rs:1786-1820 give jet_ui_with_host sole ownership of a private lifetime-bound guard, storing the live stack-held host-reference address without transmute and restoring predecessor TLS before the frame/host borrow ends on return or unwind, so callers cannot forget the guard. :1822-1834 retain the mutable TLS borrow across attachment, refusing re-entry and nested installation before another host reference can alias it, and borrow the default host when applicable. Neither caller-selected metadata nor the unit/error result can export a node or host borrow."},
  {"file":"Core/ui/host/accessibility.jet","function":"project","proof":"accessibility.jet:16-17 passes typed node/id to __project; jet_ui_host_project_accessibility (Ui.rs:295-300) owns the node and lends it read-only during host.project_accessibility inside jet_ui_with_current_host. Core/HostServices.rs:1786-1820 confine the private lifetime-bound guard to jet_ui_with_host: its stack-held host reference remains live without a lifetime transmute, and predecessor TLS is restored before the frame/host borrow ends even on unwind; no safe caller can obtain or leak the guard. :1822-1834 hold the mutable TLS borrow for the entire projection, rejecting recursive access or nested installation before a second host reference, with a live default-host borrow otherwise. The id is a typed identity, not a host address; the optional projection owns node id and metadata (:1673-1676), never a reference into the consumed node or scoped host."},
  {"file":"Core/ui/host/clipboard.jet","function":"read_text","proof":"clipboard.jet:6-7 invokes __read_text without caller memory; jet_ui_host_clipboard_read_text (Core/HostServices.rs:1852-1854) reaches host.read_clipboard_text via Ui.rs:243-247 inside jet_ui_with_current_host. The private lifetime-bound guard at :1786-1820 is owned only by jet_ui_with_host, points TLS at its live stack-held host reference without transmute, and restores predecessor TLS before that frame/host borrow ends on return or unwind; safe callers cannot forget it. :1822-1834 keep the mutable TLS borrow across the read, refusing recursive host access or nested installation before a second host reference is created and borrowing the default host otherwise. The returned clipboard carrier owns its String and optional range (:1368-1371), so dropping the host cannot invalidate returned text."},
  {"file":"Core/ui/host/clipboard.jet","function":"write_text","proof":"clipboard.jet:12-13 lends a valid String to __write_text; jet_ui_host_clipboard_write_text (Core/HostServices.rs:1856-1860) copies that str into an owned String before passing it to host.write_clipboard_text inside jet_ui_with_current_host. :1786-1820 confine the lifetime-bound guard to jet_ui_with_host, retaining its live stack-held host reference without transmute and restoring predecessor TLS before the frame/host borrow ends on return or unwind; safe callers never receive a forgettable guard. :1822-1834 hold the mutable TLS borrow across writing, rejecting re-entry or nested installation before another host reference is formed and keeping the default host borrowed otherwise. The host receives owned UTF-8 text, not a caller buffer pointer, and the result contains only a copied character count (:1374-1376) or owned error, never a scoped borrow."},
  {"file":"Core/ui/host/drag_drop.jet","function":"poll","proof":"drag_drop.jet:33-34 calls __poll with no caller-selected address; jet_ui_host_drag_poll (Core/HostServices.rs:1867-1869) invokes host.poll_drag_event inside jet_ui_with_current_host. The private lifetime-bound scope at :1786-1820 belongs only to jet_ui_with_host, which keeps its stack-held host reference live without transmute and restores predecessor TLS before the frame/host borrow ends on return or unwind; safe callers cannot obtain/leak its guard. :1822-1834 hold the mutable TLS borrow across polling, refusing recursive access or nested installation before a second host reference exists and borrowing the default host otherwise. The optional typed drag event is returned by value, with owned item storage rather than a host queue reference; no host borrow escapes even when the queue is empty or the provider errors."},
  {"file":"Core/ui/host/ime.jet","function":"poll","proof":"ime.jet:25-26 invokes __poll without caller memory; jet_ui_host_ime_poll (Core/HostServices.rs:1863-1865) calls host.poll_ime_event inside jet_ui_with_current_host. :1786-1820 give only jet_ui_with_host the private lifetime-bound guard: TLS addresses its live stack-held host reference without transmute, and predecessor TLS is restored before that frame/host borrow ends on return or unwind, so safe callers cannot forget the guard. :1822-1834 retain the mutable TLS borrow through polling, rejecting recursive access and nested installation before another host reference could alias it and borrowing the default host otherwise. The optional IME event is an owned typed value, including its optional composition text/ranges, not a borrowed queue element; no scoped reference reaches the caller."},
  {"file":"Core/ui/host/shortcuts.jet","function":"register","proof":"shortcuts.jet:11-12 moves the typed binding into __register; jet_ui_host_shortcuts_register (Core/HostServices.rs:1871-1875) passes it by value to host.register_shortcut inside jet_ui_with_current_host. The private lifetime-bound guard at :1786-1820 remains solely in jet_ui_with_host, keeps its stack-held host reference live without transmute, and restores predecessor TLS before the frame/host borrow ends on return or unwind; safe callers cannot obtain or leak it. :1822-1834 hold the mutable TLS borrow throughout registration, refusing re-entry or nested installation before a second host reference is minted and borrowing the default host otherwise. Any typed binding owns its action String (:1543-1547), not a caller memory address; the returned binding/error is owned and carries no scoped host or registry borrow."},
  {"file":"Core/ui/host/shortcuts.jet","function":"dispatch","proof":"shortcuts.jet:17-18 moves its typed shortcut into __dispatch; jet_ui_host_shortcuts_dispatch (Core/HostServices.rs:1877-1881) calls host.dispatch_shortcut inside jet_ui_with_current_host. :1786-1820 confine the private lifetime-bound guard to jet_ui_with_host, using its live stack-held host-reference address without transmute and restoring predecessor TLS before that frame/host borrow ends on return or unwind; safe callers cannot forget the guard. :1822-1834 keep the mutable TLS borrow live throughout dispatch, rejecting recursive current-host access or nested installation before another host reference can alias it and borrowing the default host otherwise. Caller shortcut text is typed owned data, not an executable pointer; dispatch returns an owned Dispatched binding or Unhandled (:1569-1572), or an owned error, never a scoped host/registry reference."},
  {"file":"Core/ui/ui.jet","function":"gtk_backend","proof":"ui.jet:126-127 invoke __gtk_backend without caller-supplied memory or callbacks. Its provider UiGtk.rs:2069-2070 calls JetGtkBackend::new(:1346-1371), allocating an owning Arc<Mutex<GtkState>> with every field initialized, empty queues and null not-yet-initialized GTK handles. Construction neither dereferences those handles nor installs/borrows the current host. The caller receives the owned backend carrier, not a pointer into a scoped provider."},
  {"file":"Core/ui/ui.jet","function":"tui_backend","proof":"__tui_backend at ui.jet:132 has no arguments and allocates JetTuiBackend through Ui.rs:1860-1861,1572-1591. The provider owns a fresh Arc<Mutex<State>> with initialized capability facts, absent frames/focus, zero counters and an empty grid. It performs no current-host scope dereference and exports no borrow of constructor-local storage; arbitrary safe callers cannot supply a forged native backing pointer."},
  {"file":"Core/ui/ui.jet","function":"null_backend","proof":"__null_backend at ui.jet:137 has no caller-selected fields and calls JetNullBackend::new (Ui.rs:1856-1857,1346-1356). That constructor owns a new Arc<Mutex<State>> containing initialized absent frame/event/focus slots and an empty command list; it neither accesses scoped host pointers nor returns references into temporary storage. The returned carrier keeps its allocation alive independently of the caller frame."},
  {"file":"Core/ui/ui.jet","function":"node_accessibility","proof":"ui.jet:148 calls __accessibility_transfer with typed UINode/UIAccessibility and the explicit present=true flag. Ui.rs:302-308 only assign Some(metadata) to the owning node then return that node; no discriminant reinterpretation, unchecked index, callback invocation or current-host access occurs. Caller metadata remains an initialized typed record and the result owns its Strings/lists rather than borrowing source storage."},
  {"file":"Core/ui/ui.jet","function":"node_shortcut","proof":"ui.jet:165 passes initialized typed node/shortcut with present=true to __shortcut_transfer. Its provider Ui.rs:311-317 only stores the typed shortcut in the node's Option and returns the owning node, with no scoped-host dispatch or raw address access. Any valid typed shortcut therefore preserves layout/lifetimes, and no native registry borrow escapes."},
  {"file":"Core/ui/ui.jet","function":"transfer_node","proof":"__node_transfer trusts numeric role/kind/absence/style representations (Ui.rs:1938-1985). ui.jet:173-184 derives role solely as -1 or0..3 and kind solely as0..4 from typed enums. :185-192 couples color presence to initialized text, fixes all six absent-slot bits to63, and marks style absent with zero initialized words. Native :1987-2002 own the copied label/color and typed children, never dereferencing caller addresses. Width/height are stored floats, not memory lengths; invalid geometry cannot cause an unchecked allocation transfer here."},
  {"file":"Core/ui/ui.jet","function":"transfer_viewport","proof":"__viewport_transfer converts numeric device/dimensions/scale to native enum/u32/u16 (Core/Preview.rs:895-908). The local helper's only callers ui.jet:115-117 supply device tags0,1,2, positive dimensions390..1280/800..1119 and scale1 or2; each fits its native width and all fields are initialized before the gate(:197-198). It accesses no current host and returns an owning plain viewport rather than a pointer or borrowed resource; arbitrary external numbers cannot reach this local transfer."},
  {"file":"Core/vendor/sqlite3/fixture/run.jet","function":"run","proof":"The no-argument fixture supplies only fixed SQL and an in-memory database name. Exclusive U64 out-slots start at zero; successful sqlite3_open_v2 and sqlite3_prepare_v2 checks dominate all database/statement uses. SQLite owns the resulting handles; the fixture never forges an address, lends a callback, or exposes a handle to callers. It steps the statement before reading column zero, finalizes it once before closing the database once, and uses neither afterward. Borrowed String storage remains live for each synchronous call; any failed operation stops before further handle use."},
  {"file": "Core/sys/sys.jet", "function": "getpgid", "proof": "A read-only POSIX query: it changes no process state, touches no memory the caller owns, and returns an Int or IOError."},
  {"file": "Core/sys/sys.jet", "function": "getsid", "proof": "A read-only POSIX query: it changes no process state, touches no memory the caller owns, and returns an Int or IOError."}
]
```
<!-- unsafe-abstractions:end -->
