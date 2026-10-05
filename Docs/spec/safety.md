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

Safe Jet's memory and data-race safety is proved twice before code is
generated. Sema owns every user-facing check (I3) and reports each violation
as a registered diagnostic. MIR Lint ([mir-lint.md](mir-lint.md)) then re-proves
the result independently over the canonical MIR on every compile; a failure is
an internal compiler error. On the Rust path rustc's borrow checker and `Send`
checks are a third check (I2). The native backend has no rustc, so MIR Lint is
the second proof it relies on:

- `ownership.move` backs use-after-move (E0121);
- `alias.exclusive` backs in-call aliasing (E0204), closures that keep a write
  live, and overlapping task-group loans (E1101);
- `view.valid` backs views whose owner changed (E0212), owners changed while
  lent to a task (E1101), and views that escape their owners (E2305);
- `send.boundary` backs mutable or borrowed captures that cross a task or
  parallel boundary (E1101) and Cell, view, or function values that cross one
  (E1102).

The MIR Lint fixtures prove each pairing: lowering's output for the matching
`tests/ui` program, with that sema check deleted, is an internal compiler error.
Writing through a frozen value (E1113) is not backed yet, because the MIR has no
frozen fact. Until it does, the native backend relies on sema alone for that
rule.
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
A source-level gate does not itself require a Rust unsafe operation: expert
crypto calls may use safe, source-owned Core Jet bodies while sema still requires
`#Unsafe` for nonce, algorithm, and secret-exposure obligations (E0510).
All actual Rust unsafe operations retain the gated-form scan, and every ungated
example retains the negative unsafe-outside-vetted scan.

<!-- unsafe-ratchet:begin -->
<!-- unsafe-ratchet:data
{
  "schema": 1,
  "total": 215,
  "counts": {
    "Examples": 38,
    "jet": 89,
    "jetpack": 1,
    "tests": 87
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
      "line": 34,
      "column": 5,
      "reason": "the stop runs the shared cleanup boundary"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/cleanup_matrix.jet",
      "line": 72,
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
      "line": 55,
      "column": 9,
      "reason": "each argument cell is inside this call's writable mapping"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Calls.jet",
      "line": 60,
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
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 100,
      "column": 9,
      "reason": "the slot is an aligned word inside the writable mapping, checked above"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 106,
      "column": 9,
      "reason": "the relocation is an aligned word inside the writable mapping, checked above"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 127,
      "column": 5,
      "reason": "`main` is the runtime's `int32_t jet_rt_main(void (*)(void))` and `entry` is the image's checked no-argument entry"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 149,
      "column": 5,
      "reason": "the slot is an aligned word inside the mapping, now writable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 38,
      "column": 1,
      "reason": "a raw Linux x86-64 system call; the caller passes the call number and arguments that call expects"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 84,
      "column": 5,
      "reason": "the caller owns `count` writable bytes at `address`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 91,
      "column": 5,
      "reason": "the caller owns `count` readable bytes at `address`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 99,
      "column": 5,
      "reason": "an anonymous mapping aliases no existing memory"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 106,
      "column": 5,
      "reason": "the caller owns the mapping it changes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 112,
      "column": 5,
      "reason": "the caller owns the mapping and nothing uses it any more"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 119,
      "column": 5,
      "reason": "exit_group ends every thread of the process"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 124,
      "column": 1,
      "reason": "stores one word at an address the caller owns and has mapped writable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 131,
      "column": 1,
      "reason": "reads one word at an address the caller owns and has mapped readable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 138,
      "column": 1,
      "reason": "reads one byte at an address the caller owns and has mapped readable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 161,
      "column": 9,
      "reason": "the caller owns the destination words"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 171,
      "column": 1,
      "reason": "calls machine code; the caller vouches for the code and its System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 184,
      "column": 1,
      "reason": "calls machine code; the caller vouches for the code and its two-word System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 199,
      "column": 1,
      "reason": "dlopen reads the NUL-terminated path at `path`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 207,
      "column": 1,
      "reason": "dlsym reads the NUL-terminated name at `name`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 230,
      "column": 5,
      "reason": "`name` is a NUL-terminated copy of `path`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 242,
      "column": 5,
      "reason": "`name` is a NUL-terminated copy of `symbol`; dlsym only reads the library's symbol table"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Tests/ComptimeScalars.jet",
      "line": 29,
      "column": 9,
      "reason": "the identity returns its single owning String unchanged; decoding has copied it"
    },
    {
      "package": "jet",
      "file": "Core/auth/auth.jet",
      "line": 126,
      "column": 5,
      "reason": "auth PASETO v4.public verification uses the fixed Ed25519 primitive"
    },
    {
      "package": "jet",
      "file": "Core/crypto/crypto.jet",
      "line": 785,
      "column": 5,
      "reason": "crypto expert: Argon2id password hashing"
    },
    {
      "package": "jet",
      "file": "Core/crypto/crypto.jet",
      "line": 812,
      "column": 5,
      "reason": "crypto expert: Argon2id password verification"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 70,
      "column": 5,
      "reason": "vault typed load provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 77,
      "column": 5,
      "reason": "vault typed current provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 84,
      "column": 5,
      "reason": "vault typed status provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 91,
      "column": 5,
      "reason": "vault typed versions provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 98,
      "column": 5,
      "reason": "vault typed generation provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 105,
      "column": 5,
      "reason": "vault typed store provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 113,
      "column": 5,
      "reason": "vault key import: signing key material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 121,
      "column": 5,
      "reason": "vault key import: X25519 secret material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 129,
      "column": 5,
      "reason": "vault wrapped import provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 136,
      "column": 5,
      "reason": "vault typed rotation provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 144,
      "column": 5,
      "reason": "vault typed retirement provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 152,
      "column": 5,
      "reason": "vault typed revocation provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 160,
      "column": 5,
      "reason": "vault write authorization provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 168,
      "column": 5,
      "reason": "vault wrapped authorization provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 176,
      "column": 5,
      "reason": "vault typed generation commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 184,
      "column": 5,
      "reason": "vault typed store commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 192,
      "column": 5,
      "reason": "vault key import commit: signing key material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 200,
      "column": 5,
      "reason": "vault key import commit: X25519 secret material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 208,
      "column": 5,
      "reason": "vault wrapped import commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 216,
      "column": 5,
      "reason": "vault typed rotation commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 224,
      "column": 5,
      "reason": "vault typed retirement commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 232,
      "column": 5,
      "reason": "vault typed revocation commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 239,
      "column": 5,
      "reason": "vault passphrase export provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 247,
      "column": 5,
      "reason": "vault recipient export provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/email/email.jet",
      "line": 545,
      "column": 13,
      "reason": "read the DKIM key length, then wipe this copy"
    },
    {
      "package": "jet",
      "file": "Core/mem/mem.jet",
      "line": 36,
      "column": 1,
      "reason": "`addr` is the address of a live, aligned `T` that stays valid for every use of the returned pointer"
    },
    {
      "package": "jet",
      "file": "Core/mem/mem.jet",
      "line": 40,
      "column": 1,
      "reason": "`pointer` names a live, aligned, readable `T` for the whole access"
    },
    {
      "package": "jet",
      "file": "Core/mem/mem.jet",
      "line": 44,
      "column": 1,
      "reason": "`pointer` names a live, aligned, writable `T` for the whole access"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 158,
      "column": 1,
      "reason": "posix umask: the caller owns the process file-creation mask and restores it when other code relies on it"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 174,
      "column": 5,
      "reason": "posix getpgid: inspect a process group"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 180,
      "column": 5,
      "reason": "posix getsid: inspect a process session"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 185,
      "column": 1,
      "reason": "posix getpriority: the caller vouches that `which` names this process or one it controls"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 189,
      "column": 1,
      "reason": "posix setpriority: the caller owns the scheduling priority of the process it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 193,
      "column": 1,
      "reason": "posix utime: the caller owns the file whose timestamps it rewrites"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 199,
      "column": 1,
      "reason": "posix stop: the caller accepts that the process ends here after the shared cleanup boundary"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 206,
      "column": 1,
      "reason": "posix pipe: the caller owns both returned descriptors and closes each exactly once"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 210,
      "column": 1,
      "reason": "posix close_fd: the caller owns `fd` and nothing uses it after this call"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 214,
      "column": 1,
      "reason": "posix mkfifo: the caller owns `path` and removes the named pipe it creates"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 218,
      "column": 1,
      "reason": "posix fork: the caller handles both the parent and the child return and reaps the child"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 222,
      "column": 1,
      "reason": "posix setsid: the caller owns the process session topology it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 226,
      "column": 1,
      "reason": "posix wait: the caller owns the child processes it reaps"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 230,
      "column": 1,
      "reason": "posix waitpid: the caller owns the child process it reaps"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 234,
      "column": 1,
      "reason": "posix setuid: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 238,
      "column": 1,
      "reason": "posix setgid: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 242,
      "column": 1,
      "reason": "posix setpgrp: the caller owns the process group topology it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 246,
      "column": 1,
      "reason": "posix setpgid: the caller owns the process group topology it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 250,
      "column": 1,
      "reason": "posix kill: the caller owns or is entitled to signal the target process"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 254,
      "column": 1,
      "reason": "posix initgroups: the caller changes supplementary groups deliberately"
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
      "package": "jetpack",
      "file": "crates/jetpack/batteries/embedded/embedded.jet",
      "line": 181,
      "column": 9,
      "reason": "host replay uses a stack cell for the CMSDK UART0 control register and never dereferences the target address"
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
| jet | 89 |
| jetpack | 1 |
| tests | 87 |
| **total** | **215** |

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
| Examples | Examples/features/io/cleanup_matrix.jet | 34:5 | "the stop runs the shared cleanup boundary" |
| Examples | Examples/features/io/cleanup_matrix.jet | 72:5 | "the exit callbacks have no captured state" |
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
| jet | Compiler/JetBackend/Source/Image/Calls.jet | 55:9 | "each argument cell is inside this call's writable mapping" |
| jet | Compiler/JetBackend/Source/Image/Calls.jet | 60:5 | "loaded.entry is the selector-generated pointer-to-cells adapter; all cells remain mapped through its return" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 48:5 | "function is the resolved (ptr,len)-&gt;word runtime constructor; buffer remains readable until it returns" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 117:5 | "word is a live String handle and both functions borrow it through the canonical runtime C ABI" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 124:9 | "the runtime's borrowed UTF-8 projection holds length readable bytes until the owning String is dropped" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 144:13 | "word is a live exact-Int carrier and stringify returns a fresh owning String" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 148:13 | "the temporary String is owned here and decoding has copied its bytes" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 100:9 | "the slot is an aligned word inside the writable mapping, checked above" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 106:9 | "the relocation is an aligned word inside the writable mapping, checked above" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 127:5 | "`main` is the runtime's `int32_t jet_rt_main(void (*)(void))` and `entry` is the image's checked no-argument entry" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 149:5 | "the slot is an aligned word inside the mapping, now writable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 38:1 | "a raw Linux x86-64 system call; the caller passes the call number and arguments that call expects" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 84:5 | "the caller owns `count` writable bytes at `address`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 91:5 | "the caller owns `count` readable bytes at `address`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 99:5 | "an anonymous mapping aliases no existing memory" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 106:5 | "the caller owns the mapping it changes" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 112:5 | "the caller owns the mapping and nothing uses it any more" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 119:5 | "exit_group ends every thread of the process" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 124:1 | "stores one word at an address the caller owns and has mapped writable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 131:1 | "reads one word at an address the caller owns and has mapped readable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 138:1 | "reads one byte at an address the caller owns and has mapped readable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 161:9 | "the caller owns the destination words" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 171:1 | "calls machine code; the caller vouches for the code and its System V signature" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 184:1 | "calls machine code; the caller vouches for the code and its two-word System V signature" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 199:1 | "dlopen reads the NUL-terminated path at `path`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 207:1 | "dlsym reads the NUL-terminated name at `name`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 230:5 | "`name` is a NUL-terminated copy of `path`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 242:5 | "`name` is a NUL-terminated copy of `symbol`; dlsym only reads the library's symbol table" |
| jet | Compiler/JetBackend/Tests/ComptimeScalars.jet | 29:9 | "the identity returns its single owning String unchanged; decoding has copied it" |
| jet | Core/auth/auth.jet | 126:5 | "auth PASETO v4.public verification uses the fixed Ed25519 primitive" |
| jet | Core/crypto/crypto.jet | 785:5 | "crypto expert: Argon2id password hashing" |
| jet | Core/crypto/crypto.jet | 812:5 | "crypto expert: Argon2id password verification" |
| jet | Core/crypto/vault.jet | 70:5 | "vault typed load provider boundary" |
| jet | Core/crypto/vault.jet | 77:5 | "vault typed current provider boundary" |
| jet | Core/crypto/vault.jet | 84:5 | "vault typed status provider boundary" |
| jet | Core/crypto/vault.jet | 91:5 | "vault typed versions provider boundary" |
| jet | Core/crypto/vault.jet | 98:5 | "vault typed generation provider boundary" |
| jet | Core/crypto/vault.jet | 105:5 | "vault typed store provider boundary" |
| jet | Core/crypto/vault.jet | 113:5 | "vault key import: signing key material" |
| jet | Core/crypto/vault.jet | 121:5 | "vault key import: X25519 secret material" |
| jet | Core/crypto/vault.jet | 129:5 | "vault wrapped import provider boundary" |
| jet | Core/crypto/vault.jet | 136:5 | "vault typed rotation provider boundary" |
| jet | Core/crypto/vault.jet | 144:5 | "vault typed retirement provider boundary" |
| jet | Core/crypto/vault.jet | 152:5 | "vault typed revocation provider boundary" |
| jet | Core/crypto/vault.jet | 160:5 | "vault write authorization provider boundary" |
| jet | Core/crypto/vault.jet | 168:5 | "vault wrapped authorization provider boundary" |
| jet | Core/crypto/vault.jet | 176:5 | "vault typed generation commit provider boundary" |
| jet | Core/crypto/vault.jet | 184:5 | "vault typed store commit provider boundary" |
| jet | Core/crypto/vault.jet | 192:5 | "vault key import commit: signing key material" |
| jet | Core/crypto/vault.jet | 200:5 | "vault key import commit: X25519 secret material" |
| jet | Core/crypto/vault.jet | 208:5 | "vault wrapped import commit provider boundary" |
| jet | Core/crypto/vault.jet | 216:5 | "vault typed rotation commit provider boundary" |
| jet | Core/crypto/vault.jet | 224:5 | "vault typed retirement commit provider boundary" |
| jet | Core/crypto/vault.jet | 232:5 | "vault typed revocation commit provider boundary" |
| jet | Core/crypto/vault.jet | 239:5 | "vault passphrase export provider boundary" |
| jet | Core/crypto/vault.jet | 247:5 | "vault recipient export provider boundary" |
| jet | Core/email/email.jet | 545:13 | "read the DKIM key length, then wipe this copy" |
| jet | Core/mem/mem.jet | 36:1 | "`addr` is the address of a live, aligned `T` that stays valid for every use of the returned pointer" |
| jet | Core/mem/mem.jet | 40:1 | "`pointer` names a live, aligned, readable `T` for the whole access" |
| jet | Core/mem/mem.jet | 44:1 | "`pointer` names a live, aligned, writable `T` for the whole access" |
| jet | Core/sys/sys.jet | 158:1 | "posix umask: the caller owns the process file-creation mask and restores it when other code relies on it" |
| jet | Core/sys/sys.jet | 174:5 | "posix getpgid: inspect a process group" |
| jet | Core/sys/sys.jet | 180:5 | "posix getsid: inspect a process session" |
| jet | Core/sys/sys.jet | 185:1 | "posix getpriority: the caller vouches that `which` names this process or one it controls" |
| jet | Core/sys/sys.jet | 189:1 | "posix setpriority: the caller owns the scheduling priority of the process it changes" |
| jet | Core/sys/sys.jet | 193:1 | "posix utime: the caller owns the file whose timestamps it rewrites" |
| jet | Core/sys/sys.jet | 199:1 | "posix stop: the caller accepts that the process ends here after the shared cleanup boundary" |
| jet | Core/sys/sys.jet | 206:1 | "posix pipe: the caller owns both returned descriptors and closes each exactly once" |
| jet | Core/sys/sys.jet | 210:1 | "posix close_fd: the caller owns `fd` and nothing uses it after this call" |
| jet | Core/sys/sys.jet | 214:1 | "posix mkfifo: the caller owns `path` and removes the named pipe it creates" |
| jet | Core/sys/sys.jet | 218:1 | "posix fork: the caller handles both the parent and the child return and reaps the child" |
| jet | Core/sys/sys.jet | 222:1 | "posix setsid: the caller owns the process session topology it changes" |
| jet | Core/sys/sys.jet | 226:1 | "posix wait: the caller owns the child processes it reaps" |
| jet | Core/sys/sys.jet | 230:1 | "posix waitpid: the caller owns the child process it reaps" |
| jet | Core/sys/sys.jet | 234:1 | "posix setuid: the caller drops or changes process credentials deliberately" |
| jet | Core/sys/sys.jet | 238:1 | "posix setgid: the caller drops or changes process credentials deliberately" |
| jet | Core/sys/sys.jet | 242:1 | "posix setpgrp: the caller owns the process group topology it changes" |
| jet | Core/sys/sys.jet | 246:1 | "posix setpgid: the caller owns the process group topology it changes" |
| jet | Core/sys/sys.jet | 250:1 | "posix kill: the caller owns or is entitled to signal the target process" |
| jet | Core/sys/sys.jet | 254:1 | "posix initgroups: the caller changes supplementary groups deliberately" |
| jet | Jetpack/Publication/Source/Sign.jet | 58:5 | "export a fresh Core signing seed only to descriptor-relative owner-only key publication; the Core key owner is wiped on every outcome" |
| jet | Jetpack/Publication/Source/Sign.jet | 113:5 | "import the protected seed into Core signing without a second raw-secret copy; close the temporary Core Secret on success and failure" |
| jet | Jetpack/Publication/Source/Sign.jet | 137:5 | "package author signatures require Core's strict Ed25519 verification; no raw verifier or helper process" |
| jet | Jetpack/Publication/Tests/Index.jet | 266:5 | "the test owns the planted lock file it ages" |
| jet | Jetpack/Publication/Tests/Sign.jet | 164:5 | "original signing secret-erasure acceptance requires the Core volatile in-place wipe provider (#3597)" |
| jet | Jetpack/Store/Tests/ClosureGraph.jet | 59:13 | "the test owns the journal file it just wrote" |
| jet | Jetpack/Store/Tests/Seal.jet | 31:5 | "the store test owns the fixture file whose mtime it pins" |
| jet | Tools/agent-eval/domain-foundations/area-embedded/pkg/mmio_fixed.jet | 4:5 | "board.sensor_v1 GPIO register 0x40000100 is a volatile MMIO word" |
| jet | Tools/agent-eval/domain-foundations/area-embedded/pkg/run.jet | 111:5 | "host replay uses a stack cell as a stand-in for the board register" |
| jetpack | crates/jetpack/batteries/embedded/embedded.jet | 181:9 | "host replay uses a stack cell for the CMSDK UART0 control register and never dereferences the target address" |
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
| tests | tests/fixtures/unsafe_obligations/run.jet | 5:5 | "local pointer" |
| tests | tests/fuzz/sema/differential/ex_crypto_crypto_migration.jet | 26:5 | "AES-256-GCM protocol interoperability" |
| tests | tests/fuzz/sema/valid/ex_lowlevel_lowlevel.jet | 10:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| tests | tests/fuzz/sema/valid/ex_lowlevel_pointer_cast_deref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
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
| tests | tests/ui/lowlevel_e3102.jet | 2:5 | "addr is valid" |
| tests | tests/ui/lowlevel_e3103.jet | 3:1 | "reads through caller-provided raw pointer" |
| tests | tests/ui/lowlevel_volatile_write_without_mem.jet | 2:5 | "addr is valid" |
| tests | tests/ui/lowlevel_volatile_write_wrong_ptr.jet | 4:5 | "plain ints are not raw pointers" |
| tests | tests/ui/lowlevel_volatile_write_wrong_value.jet | 6:5 | "slot is live and pointer never escapes" |
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
  below as a safe abstraction with its proof, it fails: a plain `pub fn` around
  a gated operation would let safe code reach that operation with no gate of
  its own. A wrapper whose caller must vouch for the operation is written
  `#Unsafe("<caller contract>") pub fn …` instead, so every call needs its own
  `#Unsafe` region (E3103), whether it goes through the module alias or a
  direct import. `core.mem.from_addr`, `volatile_read`, `volatile_write` and
  the `core.sys` POSIX controls are such functions.

The registry below is written by reviewers, one row per safe abstraction:
its file, function, and the proof that no caller can break memory safety
through it. The checker never writes a proof; it fails on a missing row and on
a row whose function no longer opens a gate.

<!-- unsafe-abstractions:begin -->
```json
[
  {"file": "Core/auth/auth.jet", "function": "verify_paseto", "proof": "The strict Ed25519 verifier only reads `key`, the PAE-encoded message and the 64-byte signature, all owned byte lists sliced and length-checked above; it returns a Bool and exposes no key material."},
  {"file": "Core/crypto/crypto.jet", "function": "password_hash_with_salt", "proof": "Password and salt lengths are bounded before Argon2id runs with fixed parameters (m=65536, t=3, p=1, 32-byte output); the derived key stays in a Secret that only feeds the PHC encoding."},
  {"file": "Core/crypto/crypto.jet", "function": "password_verify", "proof": "Every PHC field is parsed and bounded (memory, iterations, lanes, salt and digest lengths) before Argon2id runs; the derived key is compared in constant time and never leaves the function."},
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
  {"file": "Core/email/email.jet", "function": "validate_smtp_config", "proof": "The DKIM key bytes are copied into a local list inside the gate, measured, and zeroed before the gate ends; only the length leaves it."},
  {"file": "Core/sys/sys.jet", "function": "getpgid", "proof": "A read-only POSIX query: it changes no process state, touches no memory the caller owns, and returns an Int or IOError."},
  {"file": "Core/sys/sys.jet", "function": "getsid", "proof": "A read-only POSIX query: it changes no process state, touches no memory the caller owns, and returns an Int or IOError."}
]
```
<!-- unsafe-abstractions:end -->
