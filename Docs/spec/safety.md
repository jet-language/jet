# Safety

This specification defines Jet's default safety boundary and its audited
expert escape. It is for Jet users, library authors, and reviewers. The
executable spelling lives in the [syntax registry](../../Compiler/JetFoundation/Data/Syntax/);
sema checks the gate and its obligations in
[`UnsafeObligations.jet`](../../Compiler/JetSema/Source/Sema/UnsafeObligations.jet).
The focused [`unsafe_ratchet` cases](../../tests/cases/guard_selftest/) and the
[`ratchet checker`](../../Tools/agent/UnsafeRatchet.jet) provide the
repository proof; gated examples live under
[`Examples/features/`](../../Examples/features/). Terms such as stream and
reader follow the [Jet vocabulary](vocabulary.md).

## The audited escape

Safe Jet code cannot perform a raw memory operation, call an unsafe function,
or cross an unsafe foreign boundary without an enclosing audited gate. Write
the gate with Pascal-case `Unsafe` and a string reason:

```jet
cell: 1
#Unsafe("`cell` is live on this stack frame and the pointer never escapes") {
    ptr: *cell
    value: ptr.*
}
```

The same contract applies to a whole function:

```jet
#Unsafe("the foreign ABI preserves the scalar contract") add: fn(a: Int, b: Int) -> Int {
    a + b
}
```

The reason belongs to `#Unsafe` itself. Bare `#Unsafe { … }` and
`#Unsafe name: fn …` are rejected (E3112). Lowercase `unsafe` is recognized only to
teach the foreign-spelling diagnostic; it is not Jet syntax. An unsafe function
body is an audited region, but calling an unsafe function still requires an
enclosing `#Unsafe` region.

`use core.mem` is the import gate for the low-level memory names. It does not
replace the audit gate: raw-pointer formation, dereference, address-based
pointer construction, and volatile access also require `#Unsafe`. The sema
obligation pass can require `assert valid_ptr, aligned, no_alias` immediately
after a tracked operation. Those assertions are meaningful only inside the
gate and are removed before the IR is built.

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
agreement on those observables. Each case also needs an independent post-sema
check receipt; none exists yet (see below), so that leg reports a gap.
Generated-Rust differential runs are supplemental, not Rust-free proof.

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
No native code sanitizer exists on the JetIR backend, so every admitted tier
records its `sanitize` leg as an explicit coverage gap rather than a pass.
Missing objects, missing references, empty corpora, timeouts/crashes and
missing receipts likewise fail or stay open; none is silently passed.

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

Current limits: the JetIR backend emits no ASan Load/Store checks, stack
redzones/use-after-return, uninitialized-memory/UB or data-race checks.
The gate explicitly lists these, sanitized native compiler self-build and
mutation-control proofs for the IR checker's `alias.exclusive`, `view.valid` and
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
a registered diagnostic. The back end needs an independent second checker
over JetIR, alongside its runtime instrumentation
receipts, that backs:

- use-after-move (E0121);
- in-call aliasing (E0204), closures that keep a write live, and overlapping
  task-group loans (E1101);
- views whose owner changed (E0212), owners changed while lent to a task
  (E1101), and views that escape their owners (E2305);
- mutable or borrowed captures that cross a task or parallel boundary (E1101)
  and Cell, view, or function values that cross one (E1102);
- writes through a frozen value (E1113).

That checker does not exist yet. Until it lands, the native backend relies
on sema alone for these rules, and its controls must be independent mutation
controls (clean and planted programs that bypass sema), not receipts obtained by
deleting semantic checks in a compiler build.

## Unsafe-region ratchet

The repository records every user-written semantic unsafe region in the
baseline below. Each row records its owner, source file, marker position, and
reason. The owner is the region's top-level repository directory (`Compiler`,
`Core`, `Jetpack`, `Tools`, `Examples`, `tests`, …); the per-owner counts are
what the ratchet compares. The JSON data and Markdown tables are one generated
registry; do not edit either representation by hand.

[`UnsafeRatchet.jet`](../../Tools/agent/UnsafeRatchet.jet)
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
jet Tools/agent/UnsafeRatchet.jet --update
```

If source removes only recorded regions, the checker lowers and rewrites the
baseline automatically. A new semantic region cannot hide behind a net
decrease. A count-preserving edit that changes a recorded file, position, or
reason is stale and fails until the baseline is refreshed. The
`unsafe-ratchet` block is intentionally kept byte-stable between such
refreshes.

`--check` compares without ever writing: a pure decrease fails too, naming the
`--update` run that records it. `jet Tools/agent/LaneCheck.jet` runs
`UnsafeRatchet.jet --check` on every lane check, so an unrecorded region, a
stale row or a failed CoreLib audit stops the check before commit.

Every region kind lowers to its own sentry (D-MEM-SENTRY1): a block, a gated
`#Unsafe fn` body, and a gated `#FFI`/`#Import(c)` declaration, whose sentry
wraps each call.
A source-level gate does not itself require a raw unsafe operation: expert
crypto calls may use safe, source-owned Core Jet bodies while sema still requires
`#Unsafe` for nonce, algorithm, and secret-exposure obligations (E0510).

<!-- unsafe-ratchet:begin -->
<!-- unsafe-ratchet:data
{
  "schema": 1,
  "total": 1336,
  "counts": {
    "Compiler": 675,
    "Core": 186,
    "Docs": 8,
    "Examples": 42,
    "Jetpack": 9,
    "Tools": 7,
    "tests": 409
  },
  "regions": [
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 172,
      "column": 5,
      "reason": "memfd_create reads the NUL-terminated name at `name`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 186,
      "column": 5,
      "reason": "the caller owns `fd` and nothing uses it any more"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 196,
      "column": 9,
      "reason": "the caller mapped `count` writable bytes at `address`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 361,
      "column": 9,
      "reason": "a private read-only view of the memfd aliases nothing writable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 447,
      "column": 5,
      "reason": "MAP_FIXED_NOREPLACE never replaces an existing mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 487,
      "column": 5,
      "reason": "MAP_FIXED replaces only the image's own reserved pages"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 492,
      "column": 9,
      "reason": "the bytes lie in the segment's last file page, mapped private and writable above"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 505,
      "column": 13,
      "reason": "the caller mapped [from, to) + delta private and writable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 521,
      "column": 5,
      "reason": "MAP_FIXED replaces only the view's own fresh pages"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 525,
      "column": 5,
      "reason": "the bytes lie in the view's last image page, mapped private and writable above"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 530,
      "column": 9,
      "reason": "MAP_FIXED replaces only the view's own fresh pages; the memfd is never written again"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 775,
      "column": 5,
      "reason": "openat reads the NUL-terminated path at `name`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 799,
      "column": 5,
      "reason": "the timespec lies in the fresh zeroed page; nanosleep only reads it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 810,
      "column": 5,
      "reason": "the caller owns the stack bytes below `at`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 820,
      "column": 5,
      "reason": "the caller owns the stack bytes below `at`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 927,
      "column": 5,
      "reason": "brk(0) only reports the current break"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 931,
      "column": 5,
      "reason": "the map's 104 bytes lie in the data page's auxiliary-vector room, read already"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 942,
      "column": 5,
      "reason": "prlimit64 writes one rlimit (two words) at `scratch`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1013,
      "column": 5,
      "reason": "the data page is this plan's own fresh mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1024,
      "column": 5,
      "reason": "the executor page is this plan's own fresh mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1051,
      "column": 13,
      "reason": "F_GETFD only reads the descriptor's flags"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1061,
      "column": 9,
      "reason": "rt_sigaction with no new action writes the old one (32 bytes) to the scratch"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1075,
      "column": 5,
      "reason": "a fresh anonymous reservation aliases nothing"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1085,
      "column": 5,
      "reason": "getrandom fills the 16 stack bytes at `random`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1124,
      "column": 5,
      "reason": "the data page holds 3840 bytes from LOADER_DATA_AUXV; /proc/self/auxv is [type, value] word pairs"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1140,
      "column": 9,
      "reason": "the pair lies inside the bytes read into the data page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1167,
      "column": 5,
      "reason": "the words lie in the stack mapping, above its guard page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1189,
      "column": 5,
      "reason": "the block is this plan's own fresh mapping of plan_size bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1206,
      "column": 5,
      "reason": "the executor's records only reset process state; it then enters the loaded image on its own stack"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1218,
      "column": 5,
      "reason": "the executor's first record forks; the child runs only the executor and enters the image, the parent returns here"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1231,
      "column": 5,
      "reason": "wait4 writes one int status to the fresh page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1241,
      "column": 5,
      "reason": "SIGKILL to a child this process started and has not reaped"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1252,
      "column": 9,
      "reason": "a kernel sigaction with handler SIG_IGN on a fresh page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1270,
      "column": 9,
      "reason": "a zeroed kernel sigaction (SIG_DFL) and an empty signal set on a fresh page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1301,
      "column": 5,
      "reason": "execve reads the NUL-terminated strings and the two NULL-ended arrays in the block; the empty signal set at the block's start"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 126,
      "column": 5,
      "reason": "pipe2 writes two descriptors into our scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 131,
      "column": 5,
      "reason": "pipe2 wrote the scratch page's first word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 137,
      "column": 9,
      "reason": "fcntl duplicates our own descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 149,
      "column": 5,
      "reason": "closes our own descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 196,
      "column": 9,
      "reason": "the poll set is our scratch page's first three entries"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 203,
      "column": 9,
      "reason": "poll reads and writes the three entries of our scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 222,
      "column": 9,
      "reason": "poll wrote the entries' returned events"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 251,
      "column": 9,
      "reason": "the poll set is our scratch page's first two entries"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 256,
      "column": 9,
      "reason": "poll reads and writes the two entries of our scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 262,
      "column": 9,
      "reason": "poll wrote the entries' returned events"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 299,
      "column": 9,
      "reason": "the bytes lie in our scratch mapping, whose size is a multiple of eight"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 312,
      "column": 5,
      "reason": "clock_gettime writes two words at scratch + 64, past the poll set"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 324,
      "column": 9,
      "reason": "kills our own child"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 331,
      "column": 9,
      "reason": "wait4 writes the status word into our scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 336,
      "column": 5,
      "reason": "wait4 wrote the status word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 74,
      "column": 1,
      "reason": "a raw Linux x86-64 system call; the caller passes the call number and arguments that call expects"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 102,
      "column": 5,
      "reason": "the caller owns `count` writable bytes at `address`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 109,
      "column": 5,
      "reason": "the caller owns `count` readable bytes at `address`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 117,
      "column": 5,
      "reason": "an anonymous mapping aliases no existing memory"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 124,
      "column": 5,
      "reason": "the caller owns the mapping it changes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 130,
      "column": 5,
      "reason": "the caller owns the mapping and nothing uses it any more"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 137,
      "column": 5,
      "reason": "exit_group ends every thread of the process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 142,
      "column": 1,
      "reason": "stores one word at an address the caller owns and has mapped writable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 149,
      "column": 1,
      "reason": "reads one word at an address the caller owns and has mapped readable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 156,
      "column": 1,
      "reason": "reads one byte at an address the caller owns and has mapped readable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 163,
      "column": 1,
      "reason": "stores one byte at an address the caller owns and has mapped writable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 192,
      "column": 9,
      "reason": "the caller owns the destination words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 202,
      "column": 1,
      "reason": "calls machine code; the caller vouches for the code and its System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 215,
      "column": 1,
      "reason": "calls machine code; the caller vouches for the code and its two-word System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 230,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked zero-argument Unit System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 240,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked three-word Unit System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 256,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked one-word Unit System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 268,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked two-word Unit System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 285,
      "column": 1,
      "reason": "a raw Linux x86-64 syscall; the caller supplies the syscall's valid machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 302,
      "column": 1,
      "reason": "reads one aligned machine word in live allocator-owned mapped memory"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 309,
      "column": 1,
      "reason": "stores one aligned machine word in live allocator-owned mapped memory; x86 TSO makes this a release store"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 318,
      "column": 1,
      "reason": "atomically compares and exchanges one aligned live machine word; every non-atomic access to protected data holds its lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 333,
      "column": 1,
      "reason": "x86 pause only hints that this thread is waiting in a spin loop"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 343,
      "column": 1,
      "reason": "cpuid only reports processor identification; rbx is saved and restored"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 356,
      "column": 1,
      "reason": "cpuid only reports processor identification; rbx is saved and restored"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 370,
      "column": 1,
      "reason": "cpuid only reports processor identification; rbx is saved and restored"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 383,
      "column": 1,
      "reason": "cpuid only reports processor identification; rbx is saved and restored"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 398,
      "column": 1,
      "reason": "xgetbv only reads an extended control register; the caller has seen CPUID.1:ECX.OSXSAVE"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 412,
      "column": 1,
      "reason": "reads the stack pointer register after one balanced push; no memory is read"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 427,
      "column": 1,
      "reason": "the caller owns count readable bytes at source and count writable bytes at target; an overlap has target below source"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 446,
      "column": 1,
      "reason": "the caller owns count readable bytes ending at source_last and count writable bytes ending at target_last; an overlap has target above source"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 462,
      "column": 1,
      "reason": "the caller owns count readable words whose last starts at source_last and count writable words whose last starts at target_last; an overlap has target above source"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 480,
      "column": 1,
      "reason": "the caller owns count writable 8-byte words at target"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 495,
      "column": 1,
      "reason": "reads one byte from a live readable mapped address supplied by the caller"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 502,
      "column": 1,
      "reason": "writes one byte to a live writable mapped address supplied by the caller"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 512,
      "column": 1,
      "reason": "writes at most eight bytes from a stack-local word while that word is live; the balanced pushes and pops do not expose stack storage after return"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 533,
      "column": 1,
      "reason": "bytes lends length readable bytes; the second iovec lends a stack-local LF; writev retains neither pointer and every push is balanced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 570,
      "column": 1,
      "reason": "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 617,
      "column": 1,
      "reason": "the calling thread is attached and GS points at its live Jet TCB"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 626,
      "column": 1,
      "reason": "ARCH_GET_GS writes one initialized aligned stack word only during this syscall; no TLS or heap is read, and the push/pop is balanced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 641,
      "column": 1,
      "reason": "atomically adds to one aligned live word and returns its previous value"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 654,
      "column": 1,
      "reason": "atomically exchanges one aligned live word; xchg has an implicit full barrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 667,
      "column": 1,
      "reason": "calls executable System V code with one raw word argument"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 680,
      "column": 1,
      "reason": "calls a Jet closure's code with the live closure header and one raw word argument"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 701,
      "column": 1,
      "reason": "raw clone child runs only Jet/syscall code, never Rust or musl; supplied stack contains entry and argument"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 742,
      "column": 1,
      "reason": "calls checked System V Unit code; the caller lends one live raw machine-word argument"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 754,
      "column": 1,
      "reason": "calls checked System V Unit code; the caller vouches for both raw machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 768,
      "column": 1,
      "reason": "calls checked System V Unit code; the caller vouches for all three raw machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 784,
      "column": 1,
      "reason": "calls a checked native runtime callback with one machine-word argument"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 797,
      "column": 1,
      "reason": "calls a checked native runtime callback with two machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 812,
      "column": 1,
      "reason": "calls a checked native runtime callback with three machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 829,
      "column": 1,
      "reason": "calls a checked native runtime callback with four machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 848,
      "column": 1,
      "reason": "calls a checked native runtime callback with five machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 871,
      "column": 1,
      "reason": "source and directory are live owned descriptors, name is terminated; linkat borrows a stack-local empty path and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 892,
      "column": 1,
      "reason": "ioctl reads one stack-local machine word with the supplied audited request and live fd; it retains no pointer and all pushes are balanced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 915,
      "column": 1,
      "reason": "object is a live fd for fstat or a terminated path for stat/lstat; output lends 128 writable bytes; the initialized stack stat frame never escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1004,
      "column": 1,
      "reason": "the audited caller lends the stat scalar row and valid syscall object"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1006,
      "column": 5,
      "reason": "packed Linux 32-bit fields become independent portable machine words without any allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1017,
      "column": 1,
      "reason": "calls a checked System V void(void) process entry that remains executable for the entire call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1027,
      "column": 1,
      "reason": "directory and path identify a source-approved statx object; output lends three writable words; the zeroed 256-byte statx stack buffer never escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1125,
      "column": 5,
      "reason": "address names an initialized canonical native word owned by the pinned request or retained slot"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1132,
      "column": 5,
      "reason": "address names an initialized canonical native byte owned by the pinned request or retained slot"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1139,
      "column": 5,
      "reason": "width and address range are checked; the caller retains the initialized slot's physical permit"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1175,
      "column": 5,
      "reason": "the caller owns exactly size writable bytes under the native payload permit; width and value were checked above"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1193,
      "column": 9,
      "reason": "both addresses have the checked canonical alloc(size,align) / free(pointer,size,align) ABI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1209,
      "column": 5,
      "reason": "the allocation receipt pairs this owner with its original free address and exact size/alignment"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1217,
      "column": 5,
      "reason": "the admitted runtime signature or pinned clone/drop role was checked before invoking its fixed word ABI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1225,
      "column": 1,
      "reason": "calls machine code; the caller vouches for its three-word System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 383,
      "column": 5,
      "reason": "unlinkat borrows a held parent and CString"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 387,
      "column": 5,
      "reason": "adopt only a successful openat descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 394,
      "column": 5,
      "reason": "adopt only a successful openat descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 436,
      "column": 5,
      "reason": "mkdirat borrows the retained directory and CString"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 451,
      "column": 5,
      "reason": "exclusive openat descriptor transfers ownership exactly once"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 470,
      "column": 5,
      "reason": "exclusive temporary and renameat borrow retained parent and names"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 489,
      "column": 5,
      "reason": "renameat borrows both retained directory authorities"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 542,
      "column": 5,
      "reason": "Win32 fills borrowed UTF16 buffer; handle remains owned"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 552,
      "column": 5,
      "reason": "Win32 writes exactly the C-layout attribute tag record"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 601,
      "column": 5,
      "reason": "aligned variable-sized FILE_RENAME_INFO uses selected C layout and live UTF16 loans"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetDevserver/Source/WebHost02.jet",
      "line": 614,
      "column": 5,
      "reason": "Win32 consumes borrowed C-layout disposition record"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 145,
      "column": 5,
      "reason": "pointer is a fresh payload whose align >= 8 block reserved the word 24 bytes before it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 153,
      "column": 5,
      "reason": "pointer is an align >= 8 payload being freed with its allocated size; its block reserved the count word 24 bytes before it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 176,
      "column": 5,
      "reason": "anonymous private mmap owns fresh writable bytes and cannot alias a live heap block; a failed syscall produces no allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 190,
      "column": 5,
      "reason": "the caller has removed this mapping from all heap lists and no payload remains live"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 203,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 206,
      "column": 5,
      "reason": "the fresh state page is private to initialization, which precedes every allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 222,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 253,
      "column": 5,
      "reason": "initialization wrote the arena table word once, before any allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 265,
      "column": 5,
      "reason": "reads the stack pointer register only; nothing is dereferenced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 279,
      "column": 9,
      "reason": "the live totals are process-lifetime arena words; the quiescent caller needs no lock to read them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 306,
      "column": 5,
      "reason": "root is an aligned lock word in the process-lifetime state mapping; CAS/xchg acquire all metadata it protects and the futex wait only reads that word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 321,
      "column": 5,
      "reason": "the calling thread holds this lock; xchg publishes its completed metadata updates and reports sleepers, and the futex wake only names the word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 330,
      "column": 5,
      "reason": "slab and its linked neighbours are live heap-owned headers protected by the held heap lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 348,
      "column": 5,
      "reason": "head and slab are live heap-owned metadata protected by the held heap lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 382,
      "column": 5,
      "reason": "the slab-region lock owns the cursor, end and free-slab stack words and every stacked slab's link word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 420,
      "column": 5,
      "reason": "the slab-region lock owns the free-slab stack; the slab is unlinked from every class list and holds no live or cached block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 436,
      "column": 5,
      "reason": "madvise only changes the page size of the fresh private region"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 448,
      "column": 5,
      "reason": "the slab is this arena's alone: fresh from its region or off the free-slab stack; the held arena lock excludes all list readers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 474,
      "column": 5,
      "reason": "the held arena lock protects the class list, the slab's free links, carve word and live count, the retained count and the arena live total; row is the caller's exclusive cache row; the blocks leave the slab before any caller sees them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 525,
      "column": 5,
      "reason": "the held arena lock protects the slab's free links, carve word, live count, class list, retained count and live total; the block is no longer any payload or cache entry"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 554,
      "column": 5,
      "reason": "the held arena lock makes the arena's scratch row exclusive; it is emptied again before the lock is released"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 563,
      "column": 5,
      "reason": "block is this caller's exclusive block; its two cached words are read before the payload header overwrites them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 589,
      "column": 5,
      "reason": "an idempotent flag: every writer stores the same 1"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 603,
      "column": 5,
      "reason": "row is this thread's private cache row and block its cached head; both cached words are read before the payload header overwrites them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 618,
      "column": 5,
      "reason": "row is this thread's private cache row; each cached block's slab is live while the block is cached, so its arena word is readable, and the block returns under that arena's lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 649,
      "column": 9,
      "reason": "row is one of this thread's eight private cache rows"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 688,
      "column": 5,
      "reason": "the large-cache lock owns every class list, its link words and the cached byte count"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 708,
      "column": 9,
      "reason": "the large-cache lock owns every class list and the cached byte count; the mapping's first word is no live payload once it is freed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 739,
      "column": 5,
      "reason": "pointer's two header words and the aligned payload are wholly within this page-rounded class mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 758,
      "column": 5,
      "reason": "pointer is a live direct allocation of exactly `size` bytes at align 1 whose two header words name its mapping and the mapping's bytes; the mode and tracked-count words are aligned words read atomically on x86, as alloc_reserve and alloc_untrack read them; a refused mremap leaves the mapping as it was"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 794,
      "column": 9,
      "reason": "row is this thread's private cache row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 813,
      "column": 5,
      "reason": "the caller holds the policy lock and cell is a counter in alloc_root"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 821,
      "column": 5,
      "reason": "the aligned mode word is read atomically on x86; system allocation needs no policy lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 827,
      "column": 5,
      "reason": "the policy lock exclusively owns the mode, cap and accounting words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 856,
      "column": 5,
      "reason": "the policy lock exclusively owns live bytes; restoration does not erase outstanding tracked blocks"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 870,
      "column": 5,
      "reason": "the held policy lock serializes all native allocator configuration and reservations"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 887,
      "column": 5,
      "reason": "the policy lock serializes restoration with native reserve and free; tracked blocks remain in the side table"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 901,
      "column": 5,
      "reason": "the policy lock protects this bounded fact word in alloc_root"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 916,
      "column": 5,
      "reason": "the private raw-heap node is fully initialized before linking under the policy/tracker lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 929,
      "column": 5,
      "reason": "the process-lifetime tracker count is an aligned x86 word; a zero count avoids any lookup or lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 936,
      "column": 5,
      "reason": "the policy/tracker lock exclusively owns all side-table links and count"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1010,
      "column": 13,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row (TCB @320 + 16 per class, alloc_cache_row) and block its cached head, whose two cached words are read before the payload header overwrites them; an align <= 8 payload at block + header is aligned because blocks start 32-aligned"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1054,
      "column": 13,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count is an aligned word read atomically, as alloc_untrack reads it; row is this thread's private cache row; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1100,
      "column": 17,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row of the size's class and block its cached head, whose next word is read before the count overwrites it; the block's second word is its slab word, the payload header's; the payload at block + 24 is 8-aligned because blocks start 32-aligned"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1133,
      "column": 21,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count is an aligned word read atomically, as alloc_untrack reads it; row is this thread's private cache row of the size's class; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's next word overwrites it; the block's second word stays its slab word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1174,
      "column": 13,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row of the size's class and block its cached head, whose two cached words are read before the payload header overwrites them; an align-1 payload at block + 16 needs no alignment"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1202,
      "column": 13,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count is an aligned word read atomically, as alloc_untrack reads it; row is this thread's private cache row of the size's class; the caller frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1240,
      "column": 9,
      "reason": "row is this thread's private cache row; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1256,
      "column": 5,
      "reason": "the lowering returns this still-live direct allocation with exactly its original size and alignment"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1268,
      "column": 5,
      "reason": "the caller's live payload header names its slab, whose arena word is fixed when the slab is created"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1273,
      "column": 5,
      "reason": "the lowering frees exactly once with the allocation's original layout; its slab remains live until this last payload is returned under its arena's lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1328,
      "column": 5,
      "reason": "the fresh state page and stub page are private to initialization, which precedes every allocation; the stub page becomes read+execute before any call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1335,
      "column": 5,
      "reason": "checked-heap start runs during runtime-init, before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1349,
      "column": 5,
      "reason": "the checked-mode stub only copies rbp to rax and returns; a missing stub leaves no chain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1357,
      "column": 13,
      "reason": "frame is an active caller's [saved rbp][return address] pair on this thread's stack: every followed link ascends by less than 16 MiB"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1363,
      "column": 9,
      "reason": "out lends count writable words: a checked block's chain words or a trap's scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1377,
      "column": 5,
      "reason": "block is this caller's fresh heap block of prefix + size + 8 bytes; every word written lies in block .. pointer + size + 8"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1401,
      "column": 5,
      "reason": "a checked payload's canary word precedes it; the quarantine lock orders racing frees of one block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1410,
      "column": 5,
      "reason": "the live canary proves pointer is a checked payload whose header and tail this free now owns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1423,
      "column": 5,
      "reason": "the quarantine lock owns the ring and its counters; every parked block is freed and owned by the quarantine"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1458,
      "column": 5,
      "reason": "aligned whole-word stores; each checked take or give loads its observer word once"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1467,
      "column": 5,
      "reason": "the freeing caller owns size payload bytes at pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1482,
      "column": 5,
      "reason": "pointer is a parked checked payload owned by the quarantine; its header, payload and tail stay mapped until retired"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1505,
      "column": 5,
      "reason": "the freed canary proved the checked header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1514,
      "column": 5,
      "reason": "pointer is an inspected parked checked payload; its header still names the enclosing block and layout"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1541,
      "column": 5,
      "reason": "the quarantine lock owns the ring and its counters"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1557,
      "column": 5,
      "reason": "data lends length readable bytes; write retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1593,
      "column": 5,
      "reason": "path is a fresh private page; the image is mapped read-only and only read"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1617,
      "column": 5,
      "reason": "image is a read-only private mapping of this executable's ELF64 file; its program header table lies inside it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1646,
      "column": 9,
      "reason": "image is a read-only private mapping of this executable's ELF64 file; every offset read comes from its own section and symbol tables"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1689,
      "column": 9,
      "reason": "words lends count readable chain words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1733,
      "column": 9,
      "reason": "the live canary proved the checked header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1779,
      "column": 5,
      "reason": "remaining is in 1..=8, so write borrows only the initialized stack word; a short write advances within it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1836,
      "column": 5,
      "reason": "the stack-word syscalls unblock SIGABRT and, if necessary, restore its default disposition before raising it again; no heap pointer is needed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Atomic.jet",
      "line": 14,
      "column": 5,
      "reason": "place is caller-owned aligned8 private unpublished storage for one atomic word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Atomic.jet",
      "line": 19,
      "column": 5,
      "reason": "place borrows one aligned8 live atomic word; x86 aligned MOV is the SeqCst load mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Atomic.jet",
      "line": 24,
      "column": 5,
      "reason": "place borrows one aligned8 live atomic word; XCHG is the full-barrier SeqCst store mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Atomic.jet",
      "line": 29,
      "column": 5,
      "reason": "place borrows one aligned8 live atomic word; LOCK CMPXCHG orders success and failure SeqCst"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Atomic.jet",
      "line": 34,
      "column": 5,
      "reason": "place borrows one aligned8 live atomic word; LOCK XADD returns the previous wrapping64 value SeqCst"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CAbiSession.jet",
      "line": 20,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CAbiSession.jet",
      "line": 28,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ClosureRefs.jet",
      "line": 9,
      "column": 5,
      "reason": "the caller's owned reference keeps the aligned closure header live; locked compare-exchange atomically publishes one more owner"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ClosureRefs.jet",
      "line": 20,
      "column": 5,
      "reason": "consumes one reference to an aligned live closure header; locked compare-exchange provides release ordering and last-owner acquire before its release helper"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CodecCSV.jet",
      "line": 264,
      "column": 5,
      "reason": "checked native Decode callback; the tree, value slot and out words stay live for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 9,
      "column": 5,
      "reason": "data lends index's initialized packed native U8 element"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 14,
      "column": 5,
      "reason": "data owns index's writable packed native U8 element"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 87,
      "column": 5,
      "reason": "native lowering owns both live ranges of count bytes; the leaves receive the copy order that overlap requires"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 122,
      "column": 5,
      "reason": "checked native value-returning callback; its raw argument stays valid for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 128,
      "column": 5,
      "reason": "checked native value-returning callback; both raw arguments stay valid for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 134,
      "column": 5,
      "reason": "checked unary closure adapter with borrowed environment, input and output slots"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 140,
      "column": 5,
      "reason": "checked binary closure adapter with live environment, inputs and output slots"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 146,
      "column": 5,
      "reason": "checked native Unit callback; its raw argument stays valid for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Collections.jet",
      "line": 152,
      "column": 5,
      "reason": "checked Unit clone callback; source remains live and target lends writable output storage"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerFS.jet",
      "line": 169,
      "column": 9,
      "reason": "the prefix remains terminated and scratch lends 128 writable portable stat-fact bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerFS.jet",
      "line": 190,
      "column": 5,
      "reason": "name is NUL-terminated and scratch lends 128 writable portable stat-fact bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerHost.jet",
      "line": 9,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerHost.jet",
      "line": 128,
      "column": 5,
      "reason": "startup lends the start stack: the null-terminated envp array, then the auxiliary vector's [type, value] pairs up to AT_NULL"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerHost.jet",
      "line": 149,
      "column": 5,
      "reason": "page is a fresh private page: the path, then the [type, value] pairs read into it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 12,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 115,
      "column": 13,
      "reason": "the owned readable file lends its descriptor; mmap creates exactly length read-only private bytes; the map owner alone unmaps them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Comptime.jet",
      "line": 14,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ComptimeServe.jet",
      "line": 113,
      "column": 13,
      "reason": "the compiler calls an entry of this image's table, or a runtime export it resolved, on the word arguments that entry's System V signature takes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ComptimeServe.jet",
      "line": 161,
      "column": 5,
      "reason": "the serve entry runs once, on the image's only thread, before it serves"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ComptimeServe.jet",
      "line": 207,
      "column": 5,
      "reason": "the serve entry installed this runtime export, whose System V signature takes these five words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CountMinSketch.jet",
      "line": 8,
      "column": 5,
      "reason": "caller holds this native CMS mutex and owns the initialized four-byte counter"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CryptoEntropy.jet",
      "line": 20,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CryptoEntropy.jet",
      "line": 51,
      "column": 13,
      "reason": "the test/fault harness owns the synchronous callback and context until reset; the callback borrows only this writable suffix"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CryptoEntropy.jet",
      "line": 56,
      "column": 5,
      "reason": "Linux getrandom flags0 writes only the exclusively borrowed writable span and retains no address; kernel negative errno is preserved"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/DataEvent.jet",
      "line": 5,
      "column": 5,
      "reason": "address borrows an initialized word in a live native DataEvent or List"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/DataEvent.jet",
      "line": 8,
      "column": 5,
      "reason": "address owns a writable word in a native DataEvent"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/DataLimits.jet",
      "line": 5,
      "column": 5,
      "reason": "address borrows an initialized word in a live native limits carrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/DataLimits.jet",
      "line": 8,
      "column": 5,
      "reason": "address owns a writable word in a native limits carrier or getter out slot"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Deadline.jet",
      "line": 74,
      "column": 5,
      "reason": "clock_gettime writes two thread-owned TCB words, not retained after the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Display.jet",
      "line": 8,
      "column": 5,
      "reason": "borrows an initialized aligned word in an E4 header, exact-Int node or owned formatter workspace"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Display.jet",
      "line": 14,
      "column": 5,
      "reason": "writes an aligned word in an exclusively owned formatter workspace or fresh E4 header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Display.jet",
      "line": 20,
      "column": 5,
      "reason": "reads one initialized byte in an owned formatter digit buffer or borrowed exact-Int limb"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Display.jet",
      "line": 26,
      "column": 5,
      "reason": "writes one byte in an exclusively owned formatter buffer; value is in 0..255"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Dyn.jet",
      "line": 7,
      "column": 5,
      "reason": "address borrows an initialized word in a live native Dyn or List"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Dyn.jet",
      "line": 10,
      "column": 5,
      "reason": "address owns a writable word in a native Dyn or List"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/DynCodec.jet",
      "line": 108,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/DynCodec.jet",
      "line": 135,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/EncodingErrors.jet",
      "line": 5,
      "column": 5,
      "reason": "borrows an initialized aligned word in a live encoding carrier or option input"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/EncodingErrors.jet",
      "line": 8,
      "column": 5,
      "reason": "owns a writable aligned word in a fresh encoding carrier or caller output"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/EnvOverlay.jet",
      "line": 44,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/EnvRoutes.jet",
      "line": 18,
      "column": 5,
      "reason": "removed lends exactly one initialized writable C bool byte; no adjacent padding belongs to this output"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 52,
      "column": 5,
      "reason": "the image's entry calls this before it starts any other thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 62,
      "column": 9,
      "reason": "jet_rt_main calls this on the main thread before the entry runs"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 78,
      "column": 5,
      "reason": "rt_sigaction borrows the four-word action on the stack; handler is this runtime's C-ABI export with the (signal, info, context) signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 89,
      "column": 5,
      "reason": "stack is a fresh private mapping; sigaltstack copies its first three words (ss_sp, ss_flags, ss_size)"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 107,
      "column": 5,
      "reason": "stack is this thread's registered alternate stack, unused outside a handler; sigaltstack copies its first three words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 139,
      "column": 5,
      "reason": "rt_sigaction borrows the four-word SIG_DFL action on the stack; tgkill signals this thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/FaultAllocation.jet",
      "line": 21,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/FaultSchedule.jet",
      "line": 32,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/FilesPathOps.jet",
      "line": 35,
      "column": 5,
      "reason": "the terminated raw path and the 128-byte stat fact row are live for the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/FilesStreams.jet",
      "line": 24,
      "column": 9,
      "reason": "the freshly opened owned descriptor and the 128-byte stat fact row are live for fstat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/FilesWalk.jet",
      "line": 33,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/GeometryGlue.jet",
      "line": 4,
      "column": 5,
      "reason": "borrows an initialized aligned word inside a live native geometry carrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/GeometryGlue.jet",
      "line": 7,
      "column": 5,
      "reason": "owns a writable aligned word in a fresh native geometry carrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 37,
      "column": 5,
      "reason": "borrows an initialized word in the live body, chunks, closure, List or E4 allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 43,
      "column": 5,
      "reason": "owns the word, or holds the body's transition lock for this state field"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 49,
      "column": 5,
      "reason": "atomic refcount, take/close flag or transition lock in a live body allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 100,
      "column": 5,
      "reason": "the last closure reference calls its generated release(box), which drops captures and frees the box"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 205,
      "column": 5,
      "reason": "the claimed closer code has the audited code(box) ABI, with no transition lock held"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 318,
      "column": 17,
      "reason": "reader code(box, descriptor) borrows exactly count writable U8 bytes; no body lock is held"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HTTPBody.jet",
      "line": 371,
      "column": 17,
      "reason": "the source holds count initialized UTF8 or raw reader bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 14,
      "column": 5,
      "reason": "runtime startup owns a fresh anonymous mapping before any program thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 28,
      "column": 5,
      "reason": "rt_sigaction synchronously borrows the four-word SIG_IGN action; no signal handler or restorer runs for ignored SIGPIPE"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 45,
      "column": 5,
      "reason": "the caller lends length readable bytes for the complete synchronous write"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 60,
      "column": 5,
      "reason": "the output lock exclusively owns the initialized buffer and length"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 95,
      "column": 5,
      "reason": "the caller lends length bytes; the output lock owns the destination buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 158,
      "column": 5,
      "reason": "bytes lends length readable bytes; writev borrows its own LF until return"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 175,
      "column": 5,
      "reason": "the held host lock lends enough scratch for Linux TCGETS"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 218,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 226,
      "column": 5,
      "reason": "value is a borrowed E4 JetCText header; stdout copies its bytes before returning"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 238,
      "column": 5,
      "reason": "value is a borrowed E4 JetCText header; writev borrows its bytes and its own LF until return"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostDirectoryList.jet",
      "line": 9,
      "column": 9,
      "reason": "getdents supplies a live terminated name; openat describes the entry without following links"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostDirectoryList.jet",
      "line": 15,
      "column": 13,
      "reason": "the owned descriptor and full Linux stat buffer live through fstat and close"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostDirectoryList.jet",
      "line": 46,
      "column": 9,
      "reason": "an owned fresh directory descriptor and writable getdents64 buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostDirectoryList.jet",
      "line": 79,
      "column": 5,
      "reason": "close the fresh stream descriptor once after reading"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFileLock.jet",
      "line": 83,
      "column": 5,
      "reason": "the borrowed lock descriptor and 128 writable stat-fact bytes live through fstat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 8,
      "column": 5,
      "reason": "the caller borrows a live FD72 or runtime scratch word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 14,
      "column": 5,
      "reason": "the caller owns the FD72 lock or exclusive runtime scratch word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 54,
      "column": 5,
      "reason": "the fresh path allocation includes its terminator byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 285,
      "column": 9,
      "reason": "the caller lends 128 writable bytes for portable stat facts"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 374,
      "column": 5,
      "reason": "the terminated borrowed path and 128-byte stat fact output are live for the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 425,
      "column": 5,
      "reason": "the caller lends one writable Bool byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 441,
      "column": 9,
      "reason": "the held directory and terminated source-approved single name stay live through openat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 464,
      "column": 9,
      "reason": "the held directory and terminated final name live through O_PATH|O_NOFOLLOW|O_CLOEXEC openat; no read access or link traversal"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 472,
      "column": 9,
      "reason": "openat returned an exclusively owned descriptor and the caller lends StatFacts128; fstat copies initialized scalar facts"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 506,
      "column": 5,
      "reason": "both duplicated descriptors and the terminated name live through the held-inode linkat syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 528,
      "column": 9,
      "reason": "the descriptor is held live, output begins with the borrowed empty pathname, statx writes only its private stack frame"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 539,
      "column": 5,
      "reason": "the terminated source-approved path and three writable birth-time output words stay live through statx"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 593,
      "column": 5,
      "reason": "both terminated borrowed names and the caller's optional compact byte output remain live only through lgetxattr"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 623,
      "column": 9,
      "reason": "the held directory, terminated plain name and writable byte range are borrowed only through readlinkat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 649,
      "column": 5,
      "reason": "held directory duplicates and terminated names prevent any descriptor-close/reuse race during renameat2"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 669,
      "column": 9,
      "reason": "the held fd and stack-local source-validated winsize word live only through ioctl"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 15,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 179,
      "column": 13,
      "reason": "the owned walk descriptor and terminated part live through openat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 238,
      "column": 13,
      "reason": "the walk directory and terminated final part live through openat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 433,
      "column": 5,
      "reason": "the terminated borrowed path and the 256-byte statx buffer are live for the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 539,
      "column": 13,
      "reason": "the walk directory and terminated final part live through O_PATH|O_NOFOLLOW|O_CLOEXEC openat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 547,
      "column": 17,
      "reason": "the owned O_PATH descriptor and the caller's 128-byte stat facts live through fstat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 554,
      "column": 21,
      "reason": "the owned O_PATH descriptor, birth's zeroed empty pathname and three writable words live through statx"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 622,
      "column": 5,
      "reason": "both walk directories and terminated final parts live through renameat/renameat2"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 636,
      "column": 9,
      "reason": "both walk directories and terminated final parts live through linkat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 21,
      "column": 5,
      "reason": "value transfers one 56-byte error header and its nullable owned E4 text children"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 32,
      "column": 5,
      "reason": "value borrows the complete error header; result exclusively owns 56 fresh bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 68,
      "column": 5,
      "reason": "value exclusively owns a new initialized IOError with two owned text children; error lends the caller's pointer slot"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 85,
      "column": 5,
      "reason": "value lends five initialized context words; result exclusively owns its header and cloned nullable E4 children"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 101,
      "column": 5,
      "reason": "value transfers its 40-byte header and two independently owned nullable text children"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 117,
      "column": 9,
      "reason": "the optional code box borrows one initialized exact-Int word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 123,
      "column": 5,
      "reason": "nullable option boxes lend one initialized payload word; value owns cloned texts and the complete new context"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 134,
      "column": 5,
      "reason": "value borrows an initialized context header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 138,
      "column": 5,
      "reason": "value borrows a context; some lends the writable pointer output for a present field"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 150,
      "column": 5,
      "reason": "value borrows a context; some lends the writable exact-Int word output for a present field"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 158,
      "column": 5,
      "reason": "value borrows an initialized error header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 162,
      "column": 5,
      "reason": "value borrows a complete error header; the selected payload must match its canonical tag"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 173,
      "column": 5,
      "reason": "context transfers its initialized five words and text owners into the fresh error; only the context header is then freed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 187,
      "column": 5,
      "reason": "value borrows an initialized ResourceLimit error"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostIO.jet",
      "line": 202,
      "column": 5,
      "reason": "value exclusively owns seven initialized words; ResourceLimit has no text children"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 8,
      "column": 5,
      "reason": "value exclusively owns four initialized words; inputs lend exact Int owners and a valid E4 message"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 18,
      "column": 5,
      "reason": "value lends the complete immutable carrier and all three owned children"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 25,
      "column": 5,
      "reason": "value transfers its 32-byte header, two independently retained Int words and owned E4 message"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 34,
      "column": 5,
      "reason": "value lends the immutable carrier; return retains its exact Int field"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 38,
      "column": 5,
      "reason": "value lends the initialized presence word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 42,
      "column": 5,
      "reason": "value lends the immutable carrier; return retains its exact Int field"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNet.jet",
      "line": 46,
      "column": 5,
      "reason": "value lends the immutable carrier; return deep-clones its owned E4 message"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 5,
      "column": 5,
      "reason": "value lends a complete initialized network header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 8,
      "column": 5,
      "reason": "value exclusively owns the network header or its mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 38,
      "column": 5,
      "reason": "value owns sixteen octets; bytes lends length initialized U8 elements"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 50,
      "column": 5,
      "reason": "value lends IP24; data owns length initialized native U8 elements; output receives ownership"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 83,
      "column": 5,
      "reason": "bytes lends width initialized little-endian bytes (width<=8)"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetAddress.jet",
      "line": 93,
      "column": 5,
      "reason": "bytes owns width writable little-endian bytes (width<=8)"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetPoller.jet",
      "line": 24,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetPoller.jet",
      "line": 62,
      "column": 5,
      "reason": "the root lock owns a packed 12B epoll_event; state retains the native fd through this synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetPoller.jet",
      "line": 88,
      "column": 9,
      "reason": "events owns 64 packed epoll_event entries; epoll_wait retains no buffer pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetResolver.jet",
      "line": 106,
      "column": 5,
      "reason": "host lends length E4 bytes; outputs lend their writable pointer/count words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetResolver.jet",
      "line": 127,
      "column": 5,
      "reason": "name owns a trailing NUL byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostParallelism.jet",
      "line": 451,
      "column": 5,
      "reason": "sched_getaffinity writes at most the owned 128-byte mask; pid0 names this thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostParallelism.jet",
      "line": 526,
      "column": 9,
      "reason": "the probe answer is fixed for the process; a racing probe stores the same word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostParallelism.jet",
      "line": 534,
      "column": 5,
      "reason": "cpuid and xgetbv only report processor identification and the OS-enabled state"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostPipe.jet",
      "line": 163,
      "column": 13,
      "reason": "the FD72 lock protects this initialized refill-buffer byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 7,
      "column": 5,
      "reason": "the audited caller supplies the Linux syscall's valid word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 102,
      "column": 5,
      "reason": "the caller lends the C-ABI tuple's Bool byte and two writable Int words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 182,
      "column": 5,
      "reason": "buffer owns the sixteen-byte struct rlimit64 the kernel writes; the caller lends the tuple's Bool byte and three Int words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 202,
      "column": 5,
      "reason": "buffer owns the sixteen-byte struct rlimit64 the kernel reads and does not retain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 218,
      "column": 5,
      "reason": "buffer owns the 144-byte struct rusage the kernel writes; the caller lends the tuple's Bool byte and four Int words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 232,
      "column": 5,
      "reason": "some lends a writable C-ABI Option success word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 242,
      "column": 5,
      "reason": "status lends an eight-byte out slot; wait4 writes its low four bytes and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 254,
      "column": 5,
      "reason": "read_fd lends eight writable bytes to pipe's two u32 results; split both facts into the caller's separate word slots"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 13,
      "column": 9,
      "reason": "the FD72 lock prevents native descriptor close/reuse throughout this nonblocking syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 55,
      "column": 13,
      "reason": "accept4 retains no address pointers and atomically sets CLOEXEC/NONBLOCK on the accepted socket"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 62,
      "column": 13,
      "reason": "private scratch lends sockaddr bytes and its exact 4B socklen; accept4 retains neither pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 97,
      "column": 9,
      "reason": "private scratch lends one native int and its 4B socklen; getsockopt retains neither pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 112,
      "column": 9,
      "reason": "private scratch lends exactly one native int; setsockopt retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 136,
      "column": 5,
      "reason": "data lends length native U8 elements; output owns length compact bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 150,
      "column": 9,
      "reason": "the held FD72 lock lends its live nonblocking socket; packed lends exactly length bytes; no pointer survives recvfrom/sendto"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 155,
      "column": 13,
      "reason": "recvfrom initialized result compact bytes; data owns the corresponding mutable native U8 slots"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 186,
      "column": 9,
      "reason": "the FD72 lock retains the socket; private scratch lends sockaddr bytes; packed lends length bytes; sendto retains no pointers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 203,
      "column": 9,
      "reason": "private scratch lends sockaddr/socklen and packed lends length mutable bytes; MSG_TRUNC returns the complete datagram length while writing at most length bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 212,
      "column": 17,
      "reason": "recvfrom initialized used compact bytes; the caller owns used mutable native U8 elements"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 236,
      "column": 5,
      "reason": "path lends its immutable E4 bytes for std-compatible sockaddr_un validation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 225,
      "column": 17,
      "reason": "Linux TCGETS initialized the 36-byte termios buffer; cfmakeraw sets VMIN=1, VTIME=0 in its control-character array"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 304,
      "column": 13,
      "reason": "rt_sigaction borrows four initialized stack words; no allocator, mutex or TLS is touched after fork"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 393,
      "column": 13,
      "reason": "rt_sigaction writes the four-word old action into this spawn's block; no allocator, mutex or TLS is touched in the child"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 403,
      "column": 5,
      "reason": "rt_sigprocmask reads the parent's saved mask word in this spawn's block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 447,
      "column": 5,
      "reason": "mprotect makes the lowest page of this spawn's fresh mapping the guard; the masks are words of this spawn's block; the child runs only jet_rt_process_spawn_child on the mapped stack and leaves through execve or exit_group before the parent resumes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 800,
      "column": 9,
      "reason": "ioctl borrows one stack word of source-validated winsize dimensions; no pointer escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 815,
      "column": 5,
      "reason": "source lends a NUL-terminated raw path including non-Unicode bytes and a writable 128-byte stat fact row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 826,
      "column": 5,
      "reason": "the source-approved native NUL path and Jet-owned PATH_MAX buffer are live across the mechanical realpath call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 831,
      "column": 9,
      "reason": "libc __errno_location returns this calling thread's live errno int; read only its initialized four bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 842,
      "column": 5,
      "reason": "successful realpath NUL-terminates its result within the PATH_MAX byte buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 34,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 48,
      "column": 5,
      "reason": "bytes borrows one live NUL-terminated startup string"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 80,
      "column": 5,
      "reason": "host argument registration is single-threaded and precedes user code"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 90,
      "column": 5,
      "reason": "host argument registration is single-threaded and precedes user code"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 104,
      "column": 5,
      "reason": "startup lends the immutable, null-terminated envp pointer array and its NUL-terminated entries"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 118,
      "column": 5,
      "reason": "pending exclusively owns raw_count + 2 words; registration is single-threaded and precedes user code"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 136,
      "column": 5,
      "reason": "rows exclusively owns raw_count index records; startup lends every pointed-to NUL string"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 157,
      "column": 5,
      "reason": "storage exclusively owns total_bytes; each source row lends its NUL string for this capture call, and published rows point only into the owned block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 169,
      "column": 5,
      "reason": "the index is published once: by single-threaded registration, or inside host_environment_require's once before it completes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 199,
      "column": 5,
      "reason": "bytes lends one live NUL-terminated startup string"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 213,
      "column": 5,
      "reason": "startup lends length bytes; data exclusively owns length initialized packed bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 226,
      "column": 5,
      "reason": "output lends a 24-byte native [U8] header and receives ownership of data"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 240,
      "column": 5,
      "reason": "row borrows a complete immutable startup index record"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 255,
      "column": 5,
      "reason": "row borrows its immutable record; data_out lends one writable pointer word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 264,
      "column": 5,
      "reason": "row borrows its immutable record; data_out lends one writable pointer word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 280,
      "column": 5,
      "reason": "startup lends argc initialized argv pointers and their immutable NUL strings"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 294,
      "column": 5,
      "reason": "argv[1] is an immutable startup string of `length` bytes; data lends the job name's static bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 339,
      "column": 5,
      "reason": "startup lends argc initialized argv pointers and their immutable NUL strings"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 357,
      "column": 5,
      "reason": "the startup index and key bytes are immutable borrowed data"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 21,
      "column": 5,
      "reason": "value borrows a complete immutable startup NUL string; output lends one writable U64 word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 48,
      "column": 5,
      "reason": "value borrows length immutable captured startup bytes; no pointer escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 82,
      "column": 5,
      "reason": "the held Host lock lends 16 writable bytes of its existing ioctl scratch; the canonical lookup retains no key pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 92,
      "column": 5,
      "reason": "output exclusively borrows the paired initialized empty Source out-record; no owner is overwritten"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 115,
      "column": 5,
      "reason": "Host startup already initialized its process-lifetime mapping; the held lock owns ioctl scratch32..47 and FIRST raw lookup only borrows the immutable captured index"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 147,
      "column": 13,
      "reason": "the paired Source owner receives the sole native U8 buffer with exact LEN/CAP/DATA; no raw pointer is cast into a List value"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStdio.jet",
      "line": 56,
      "column": 5,
      "reason": "text lends an E4 header and its bytes for this synchronous write; writev borrows its own stack LF"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStdio.jet",
      "line": 82,
      "column": 5,
      "reason": "the caller lends the tuple's exact-Int word and Bool byte outputs"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStdio.jet",
      "line": 92,
      "column": 5,
      "reason": "some lends the route's zeroed Text out slot"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 14,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 29,
      "column": 5,
      "reason": "clock_gettime synchronously writes the calling thread's two scratch words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 58,
      "column": 5,
      "reason": "before_out lends the tuple's one writable Bool byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 151,
      "column": 5,
      "reason": "path lends length bytes; std file open rejects embedded NUL"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 159,
      "column": 5,
      "reason": "name owns its trailing NUL byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 195,
      "column": 13,
      "reason": "bytes lends used initialized bytes; data owns used native U8 elements"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 10,
      "column": 5,
      "reason": "borrow an initialized word in a live exact-Int carrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 16,
      "column": 5,
      "reason": "write an initialized word in a uniquely owned exact-Int workspace"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 22,
      "column": 5,
      "reason": "borrow four initialized bytes in a live limb buffer; no adjacent limb is read"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 28,
      "column": 5,
      "reason": "write four bytes in a uniquely owned limb buffer; value is an unsigned 32-bit limb"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 386,
      "column": 9,
      "reason": "the caller holds an existing owner; atomic retain cannot race final-owner retirement"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 407,
      "column": 9,
      "reason": "the caller consumes one owner; atomic decrement publishes refs0 before shared-domain retirement"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 412,
      "column": 13,
      "reason": "the final owner has reached zero; the remaining shared hazard domain decides immediate free or retirement"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/IntHazard.jet",
      "line": 13,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/IntHazard.jet",
      "line": 92,
      "column": 5,
      "reason": "the native numeric forwarder lends a pure reload callback and its live closure context for this synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/JourneyTLS.jet",
      "line": 22,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/LogicalEnvironment.jet",
      "line": 79,
      "column": 5,
      "reason": "host argument registration is single-threaded and precedes user code"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 23,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 46,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 62,
      "column": 5,
      "reason": "an idempotent flag: every writer stores the same 1"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 77,
      "column": 5,
      "reason": "a whole-word setting: each setter stores one aligned word and the last setter wins, which is what setting it means"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 118,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 123,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 185,
      "column": 5,
      "reason": "only the main thread running jet_rt_main writes its root control"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 246,
      "column": 5,
      "reason": "only the main thread running jet_rt_main writes its root control"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 249,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 252,
      "column": 5,
      "reason": "entry is the loader/object's checked executable System V void(void) address, not a closure header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 258,
      "column": 5,
      "reason": "only the main thread running jet_rt_main writes its root control"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 280,
      "column": 5,
      "reason": "the entry has returned and drained its workers; only the main thread runs here"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/MemRegistry.jet",
      "line": 20,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/MemStorage.jet",
      "line": 23,
      "column": 5,
      "reason": "address borrows an initialized word in a native allocator header, descriptor, drop record, or Fixed metadata span"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/MemStorage.jet",
      "line": 26,
      "column": 5,
      "reason": "address exclusively owns one aligned writable allocator or Fixed metadata word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/MemStorage.jet",
      "line": 84,
      "column": 5,
      "reason": "code is the Lower-generated address-based lazy.drop thunk for the initialized value at pointer; its SysV result is Unit and discarded, and no allocator lock is held"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Memo.jet",
      "line": 10,
      "column": 5,
      "reason": "address borrows an aligned word of a live memo cell or its entry array"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Memo.jet",
      "line": 13,
      "column": 5,
      "reason": "address is an aligned word of a memo cell or entry array the caller's lock owns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NativeLayoutABI.jet",
      "line": 11,
      "column": 5,
      "reason": "thread-confined live Layout storage lends this initialized aligned word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NativeLayoutABI.jet",
      "line": 14,
      "column": 5,
      "reason": "thread-confined native Layout storage owns this aligned word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NativeLayoutABI.jet",
      "line": 78,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NativeLayoutText.jet",
      "line": 62,
      "column": 5,
      "reason": "runtime-init (jet_rt_layout_init) runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NetRoutes.jet",
      "line": 120,
      "column": 9,
      "reason": "the owned terminated host copy truncates at its validated scope suffix"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NetRoutes.jet",
      "line": 438,
      "column": 9,
      "reason": "output lends the C ABI bool result byte, not an eight-byte word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NetUnix.jet",
      "line": 189,
      "column": 5,
      "reason": "private scratch lends one native int and its 4B socklen; getsockopt retains neither pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Observe.jet",
      "line": 18,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Observe.jet",
      "line": 25,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Observe.jet",
      "line": 147,
      "column": 5,
      "reason": "an idempotent flag: every writer stores the same 1"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Observe.jet",
      "line": 453,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ObserveLive.jet",
      "line": 70,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ObserveLive.jet",
      "line": 118,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ObserveLive.jet",
      "line": 247,
      "column": 5,
      "reason": "vetted Observe Linux file/clock syscalls borrow native buffers only for their duration"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ObserveLive.jet",
      "line": 515,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Parallel.jet",
      "line": 43,
      "column": 9,
      "reason": "checked System V (env,start,end)->Unit callback; env remains borrowed and each range has exclusive output slots"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PluginHost.jet",
      "line": 105,
      "column": 5,
      "reason": "the linked guest entry reads the live instance and argument area only for this synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessExecutionCollect.jet",
      "line": 311,
      "column": 9,
      "reason": "ok is the canonical writable Bool output slot"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessExecutionCollect.jet",
      "line": 319,
      "column": 5,
      "reason": "ok lends exactly the C ABI Bool byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessExecutionCollect.jet",
      "line": 422,
      "column": 13,
      "reason": "the child mutex holds the capture builder's initialized bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessRoutes.jet",
      "line": 112,
      "column": 5,
      "reason": "supported_out lends exactly one writable C bool byte; Linux implements F_DUPFD_CLOEXEC"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 33,
      "column": 9,
      "reason": "written under the scheduler root mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 245,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProveJournal.jet",
      "line": 97,
      "column": 5,
      "reason": "the harness main arms the journal before any test, so before any other thread reads it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProveJournal.jet",
      "line": 165,
      "column": 5,
      "reason": "into lends `capacity` writable bytes for a raw Linux read of stdin"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProveJournal.jet",
      "line": 181,
      "column": 5,
      "reason": "cell lends 24 writable bytes to TIOCGPGRP and getpgrp takes none"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProveJournal.jet",
      "line": 189,
      "column": 9,
      "reason": "cell lends the new and old signal masks and the group id to rt_sigprocmask and TIOCSPGRP"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProveJournal.jet",
      "line": 197,
      "column": 9,
      "reason": "cell lends the group id to TIOCSPGRP and the old mask to rt_sigprocmask"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Reactive.jet",
      "line": 34,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Reactive.jet",
      "line": 74,
      "column": 5,
      "reason": "the observer id is a label nothing reads back; a racing creator can only repeat a label"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ReceiptSections.jet",
      "line": 235,
      "column": 5,
      "reason": "the terminated path and the 128-byte stat fact row are live for the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ReceiptSections.jet",
      "line": 325,
      "column": 13,
      "reason": "the path copy owns its terminator byte and every byte before it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ReceiptSections.jet",
      "line": 342,
      "column": 13,
      "reason": "the path copy owns every byte before its terminator"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Refcount.jet",
      "line": 31,
      "column": 5,
      "reason": "count addresses the aligned prefix word of a live refcounted block the caller holds"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Refcount.jet",
      "line": 37,
      "column": 5,
      "reason": "count addresses the aligned prefix word of a block the caller just allocated and still owns exclusively"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Refcount.jet",
      "line": 43,
      "column": 5,
      "reason": "LOCK XADD on the aligned prefix word of a live refcounted block; the caller's reference keeps it live"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Refcount.jet",
      "line": 70,
      "column": 9,
      "reason": "count addresses the aligned prefix word of a live refcounted block whose only reference the caller holds"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Refcount.jet",
      "line": 90,
      "column": 9,
      "reason": "LOCK CMPXCHG on the aligned prefix word of a live refcounted block; the caller's reference keeps it live"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteACore.jet",
      "line": 21,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteACore.jet",
      "line": 29,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAEncoding.jet",
      "line": 93,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAEncoding.jet",
      "line": 467,
      "column": 5,
      "reason": "the terminated raw path and the 128-byte stat fact row are live for the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAFiles.jet",
      "line": 31,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAFiles.jet",
      "line": 488,
      "column": 5,
      "reason": "the audited caller supplies the Linux syscall's valid word arguments and live buffers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAFiles.jet",
      "line": 494,
      "column": 5,
      "reason": "the audited caller supplies the Linux syscall's valid word arguments and live buffers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 46,
      "column": 5,
      "reason": "the raylib canvas belongs to the one thread that drives the window"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 51,
      "column": 9,
      "reason": "the raylib canvas belongs to the one thread that drives the window"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 56,
      "column": 5,
      "reason": "the raylib canvas belongs to the one thread that drives the window"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 223,
      "column": 5,
      "reason": "each row start in [at, stop) lends count writable canvas pixels of four bytes each, inside the live canvas"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 272,
      "column": 5,
      "reason": "each row start in [at, stop) lends tail + 8 writable canvas bytes, inside the live canvas"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 320,
      "column": 5,
      "reason": "each row start in [at, stop) lends count readable and writable canvas pixels of four bytes each, inside the live canvas"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 393,
      "column": 5,
      "reason": "the raylib canvas belongs to the one thread that drives the window"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 405,
      "column": 5,
      "reason": "the raylib textures belong to the one thread that drives the window"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 463,
      "column": 9,
      "reason": "the raylib textures belong to the one thread that drives the window"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 470,
      "column": 5,
      "reason": "the raylib textures belong to the one thread that drives the window"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAGame.jet",
      "line": 479,
      "column": 5,
      "reason": "the raylib textures belong to the one thread that drives the window"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteALog.jet",
      "line": 36,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAMath.jet",
      "line": 13,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteSys.jet",
      "line": 21,
      "column": 5,
      "reason": "the caller lends the C-ABI tuple's Bool byte and two writable Int words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteSys.jet",
      "line": 29,
      "column": 5,
      "reason": "some lends a writable C-ABI Option success word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteSys.jet",
      "line": 102,
      "column": 5,
      "reason": "averages owns three initialized f64 words; the caller lends the tuple's Bool byte, Int word and three Float words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteSys.jet",
      "line": 136,
      "column": 5,
      "reason": "buffer owns an initialized struct tms; the caller lends the tuple's Bool byte and six Int words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteTerm.jet",
      "line": 60,
      "column": 5,
      "reason": "the caller lends the tuple's Bool byte, Text word and Option<Text> tag/payload words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteTerm.jet",
      "line": 231,
      "column": 5,
      "reason": "the caller lends the tuple's Text word and Option<Text> tag/payload words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteTerm.jet",
      "line": 334,
      "column": 13,
      "reason": "written under the host lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteTesting.jet",
      "line": 84,
      "column": 13,
      "reason": "written under the host lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteTesting.jet",
      "line": 93,
      "column": 9,
      "reason": "written under the host lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ScalarReflectGlue.jet",
      "line": 4,
      "column": 5,
      "reason": "borrows an initialized aligned word in a live scalar/reflection carrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ScalarReflectGlue.jet",
      "line": 7,
      "column": 5,
      "reason": "owns a writable aligned word in a native scalar/reflection carrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 59,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 132,
      "column": 5,
      "reason": "the cold OS leaf borrows its initialized stack word; a successful nonzero base is this attached thread's live Jet TCB or the detached zero word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 205,
      "column": 5,
      "reason": "written under the scheduler root mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 216,
      "column": 9,
      "reason": "written under the scheduler root mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 232,
      "column": 9,
      "reason": "written under the scheduler root mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 535,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 796,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 931,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 1186,
      "column": 9,
      "reason": "sched_yield retains no memory and yields this OS worker"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 1293,
      "column": 9,
      "reason": "sched_yield retains no memory and yields this OS thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 13,
      "column": 5,
      "reason": "address names bytes bytes of live storage the caller owns or is freeing; effectful asm stores wipe every byte before release or reuse"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 38,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 110,
      "column": 17,
      "reason": "secret is a registered live Secret header (Close unregisters before it clears); its buffer holds capacity bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 128,
      "column": 5,
      "reason": "start..start+length covers the Secret buffer's mapped pages; madvise and mlock change only their dump and residency attributes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 147,
      "column": 5,
      "reason": "bytes is the moved native [U8] header; secret is fresh aligned24B storage and takes over its buffer reference without copying secret material"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 169,
      "column": 5,
      "reason": "secret is fresh aligned24B storage this call owns exclusively"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 188,
      "column": 5,
      "reason": "secret is a live exclusively borrowed native24B Secret header; len/cap/data follow the native List layout and all byte owners remain inside its buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sentry.jet",
      "line": 14,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Shared.jet",
      "line": 62,
      "column": 5,
      "reason": "x86 pause only hints that this thread is waiting in a spin loop; no memory is touched"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Shared.jet",
      "line": 437,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/SharedTransactions.jet",
      "line": 17,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/SharedTransactions.jet",
      "line": 23,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 6,
      "column": 5,
      "reason": "borrows an aligned live synchronization word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 10,
      "column": 5,
      "reason": "publishes an aligned live synchronization word with x86 release ordering"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 16,
      "column": 5,
      "reason": "atomic exchange on an aligned live synchronization word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 22,
      "column": 5,
      "reason": "atomic fetch-add on an aligned live synchronization word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 28,
      "column": 5,
      "reason": "atomic compare-exchange on an aligned live synchronization word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 34,
      "column": 5,
      "reason": "futex wake borrows a live aligned word and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 42,
      "column": 5,
      "reason": "futex borrows live state; timed wait uses this attached thread's reserved TCB scratch until the syscall returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 54,
      "column": 5,
      "reason": "clock_gettime writes two words of thread-owned TCB scratch; no pointer escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 108,
      "column": 5,
      "reason": "sync_sole_inflight is this sole thread's own count until sync_sole is cleared; sync_sole_end reads it only after a membarrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 125,
      "column": 5,
      "reason": "an elided section: only this thread has written the lock word and the count since sync_sole_enter"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 137,
      "column": 5,
      "reason": "membarrier and sched_yield retain no memory; sync_sole only ever goes from 1 to 0"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Task.jet",
      "line": 511,
      "column": 9,
      "reason": "written once, zeroed first, under the TLS registry mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TaskGroup.jet",
      "line": 254,
      "column": 9,
      "reason": "the canonical task selector yields without consuming any child result"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TerminalColor.jet",
      "line": 8,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TerminalInput.jet",
      "line": 22,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TerminalProcessRoutes.jet",
      "line": 58,
      "column": 9,
      "reason": "TIOCSWINSZ borrows one initialized stack winsize word; the Child72 lock prevents close/recycling until ioctl returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Test.jet",
      "line": 132,
      "column": 9,
      "reason": "the OS leaf borrows at most eight initialized bytes from its own stack word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestCapture.jet",
      "line": 31,
      "column": 9,
      "reason": "written under the TLS registry mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestContracts.jet",
      "line": 28,
      "column": 5,
      "reason": "the harness main opens contracts before any test, so before any other thread observes one"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 26,
      "column": 9,
      "reason": "startup lends the immutable, null-terminated envp pointer array"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 36,
      "column": 13,
      "reason": "the startup entry is a live NUL-terminated path for open"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 40,
      "column": 17,
      "reason": "startup is single-threaded and precedes user code"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 86,
      "column": 13,
      "reason": "buffer lends 4096 writable bytes after the kind byte to getcwd"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 93,
      "column": 13,
      "reason": "link is a live NUL-terminated /proc path and buffer lends 4096 writable bytes after the kind byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 153,
      "column": 5,
      "reason": "data lends `length` initialized bytes to one write"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestSuite.jet",
      "line": 24,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestSuite.jet",
      "line": 83,
      "column": 5,
      "reason": "`test: fn` calls `t.run()` on the harness's main thread, before any test task exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestSuite.jet",
      "line": 92,
      "column": 5,
      "reason": "the pinned named-fn box code takes its box and writable inline 16B result storage"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Text.jet",
      "line": 16,
      "column": 5,
      "reason": "address borrows an initialized word in a live text or iterator header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Text.jet",
      "line": 22,
      "column": 5,
      "reason": "address is an exclusively owned writable header word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Text.jet",
      "line": 26,
      "column": 5,
      "reason": "address borrows an initialized byte inside the caller's live buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Text.jet",
      "line": 32,
      "column": 5,
      "reason": "address is one exclusively owned writable byte and value is a byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TextGlue.jet",
      "line": 4,
      "column": 5,
      "reason": "borrows an initialized aligned word in a live native text diagnostic"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TextGlue.jet",
      "line": 7,
      "column": 5,
      "reason": "owns a writable aligned word in a fresh native text diagnostic"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TextMethods.jet",
      "line": 146,
      "column": 9,
      "reason": "item0 lends the C-ABI tuple's one writable Bool byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TextMethods.jet",
      "line": 151,
      "column": 5,
      "reason": "item0 lends the C-ABI tuple's one writable Bool byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 62,
      "column": 5,
      "reason": "every caller has attached before running Jet TLS code; an unattached thread's GS names the readable zero word, which the allocator alone treats as no TCB"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 72,
      "column": 5,
      "reason": "GS is reserved for Jet and base is a live self-addressed TCB or the detached zero word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 79,
      "column": 5,
      "reason": "gettid identifies this thread without reading its inherited GS"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 123,
      "column": 5,
      "reason": "getpid retains no memory"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 193,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 287,
      "column": 9,
      "reason": "the registry owns this live closure header whose code accepts header and one I64 value"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 377,
      "column": 5,
      "reason": "the caller transferred a System V I64->I64 code address and one argument to this thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 394,
      "column": 5,
      "reason": "getrlimit(RLIMIT_STACK) writes its two-word record into the owned 16-byte scratch and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 461,
      "column": 5,
      "reason": "fresh raw clone stack mapping with a PROT_NONE first-page guard; child may call only Jet/syscall code, never musl/Rust with inherited FS"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 478,
      "column": 5,
      "reason": "guarded stack is seeded with Jet-only entry/arg; TID word is live until join; flags share VM, FS, FILES, SIGHAND, THREAD, SYSVSEM and enable parent-set/child-clear TID"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 493,
      "column": 9,
      "reason": "kernel CHILD_CLEARTID clears this live word and wakes a non-private futex; recheck handles exit-before-wait and EINTR"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TimeGlue.jet",
      "line": 3,
      "column": 5,
      "reason": "borrows an initialized aligned word inside a native time carrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TimeGlue.jet",
      "line": 6,
      "column": 5,
      "reason": "owns a writable aligned word inside a fresh native time carrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 8,
      "column": 5,
      "reason": "the caller borrows an initialized word of a live JetText or static require descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 14,
      "column": 5,
      "reason": "the caller borrows exactly one initialized byte of a live UTF-8 carrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 20,
      "column": 5,
      "reason": "the caller owns eight writable bytes in a scratch mapping or allocated JetText storage"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 26,
      "column": 5,
      "reason": "address is one writable byte in the allocated debug-text buffer; value is a byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 32,
      "column": 5,
      "reason": "audited runtime syscall: write borrows live text, mmap creates scratch, munmap releases it, exit_group ends the process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 89,
      "column": 9,
      "reason": "the OS leaf borrows a stack word; generated calls have 1..8 valid bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UiBackends.jet",
      "line": 468,
      "column": 5,
      "reason": "a web page's module runs on one thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UiBackends.jet",
      "line": 514,
      "column": 5,
      "reason": "only an attached web build records, on its one thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UiBackends.jet",
      "line": 524,
      "column": 5,
      "reason": "a web page's module runs on one thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UiBackends.jet",
      "line": 905,
      "column": 9,
      "reason": "written under the host lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UiCarriersEnums.jet",
      "line": 5,
      "column": 5,
      "reason": "address borrows an initialized native UI carrier or E4 header word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UiCarriersEnums.jet",
      "line": 10,
      "column": 5,
      "reason": "address exclusively owns one writable native UI carrier word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Unicode.jet",
      "line": 145,
      "column": 5,
      "reason": "some is the caller's writable aligned Option<Int> payload slot, written only on success"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/VaultTestHooks.jet",
      "line": 22,
      "column": 5,
      "reason": "written under the TLS registry mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/VaultTestHooks.jet",
      "line": 27,
      "column": 5,
      "reason": "written under the TLS registry mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/VaultTestHooks.jet",
      "line": 182,
      "column": 5,
      "reason": "written under the vault test backup mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/World.jet",
      "line": 76,
      "column": 5,
      "reason": "registered provider code remains live at this world/lease boundary; output lends one writable word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/WsRoutes.jet",
      "line": 500,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/XMLGlue.jet",
      "line": 4,
      "column": 5,
      "reason": "borrows an initialized aligned word in a live XML carrier or optional input"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/XMLGlue.jet",
      "line": 7,
      "column": 5,
      "reason": "owns a writable aligned word in a fresh XML carrier or caller output"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 11,
      "column": 5,
      "reason": "a failed allocator witness terminates this isolated test process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 27,
      "column": 5,
      "reason": "both allocations own count bytes; touching their endpoints must not alias each other or their metadata"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 67,
      "column": 5,
      "reason": "first's private header names its live slab, whose arena word is fixed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 84,
      "column": 5,
      "reason": "the flushed cache left the slab linked; its live count and the class cell are stable while this thread is the heap's only user"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 94,
      "column": 9,
      "reason": "pointer's private header identifies its still-live slab, which must be the same mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 99,
      "column": 9,
      "reason": "no other thread touches the slab or its class list"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 113,
      "column": 5,
      "reason": "first's private header holds its mapping base and length"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 137,
      "column": 9,
      "reason": "the test owns the full payload and can read its allocator header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 154,
      "column": 9,
      "reason": "the chain word remains readable until its payload is freed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 161,
      "column": 5,
      "reason": "after all payloads are freed, exactly one empty slab is retained; any second empty slab was unlinked onto the free-slab stack"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 179,
      "column": 9,
      "reason": "the test owns the full payload and keeps its chain word there"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 187,
      "column": 9,
      "reason": "the chain word remains readable until its payload is freed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 194,
      "column": 5,
      "reason": "this thread is the heap's only user; the stack word is read once and the stacked slab's link word stays mapped"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 202,
      "column": 5,
      "reason": "the retired slab is the stack's top again; its pages were never unmapped"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 229,
      "column": 5,
      "reason": "the test owns this fresh align-8 block and its count word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 258,
      "column": 5,
      "reason": "the test owns this fresh align-8 block of size bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 267,
      "column": 5,
      "reason": "the test holds both owners of this block and its count word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 273,
      "column": 5,
      "reason": "the test still owns this block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 279,
      "column": 5,
      "reason": "a cached small block stays mapped; its payload bytes are not cache links"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 294,
      "column": 5,
      "reason": "the test owns these fresh align-8 blocks and their count words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 298,
      "column": 5,
      "reason": "the test owns these fresh align-8 blocks and their count words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 327,
      "column": 9,
      "reason": "the test owns the payload and keeps its chain word there"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 336,
      "column": 5,
      "reason": "row is this thread's own cache row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 340,
      "column": 9,
      "reason": "the chain word remains readable until its payload is freed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 346,
      "column": 9,
      "reason": "row is this thread's own cache row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 353,
      "column": 5,
      "reason": "after the flush the row is empty"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 363,
      "column": 5,
      "reason": "row is this thread's own cache row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 372,
      "column": 5,
      "reason": "the test owns this fresh align-8 block and its header words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 454,
      "column": 5,
      "reason": "the exact success line is two bounded stack words written to stdout"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocationPolicyTests.jet",
      "line": 4,
      "column": 5,
      "reason": "the single-threaded fixture reads the process-lifetime tracked-node count after all its payloads were freed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/EnvOverlayKernel.jet",
      "line": 6,
      "column": 5,
      "reason": "failed witness terminates its isolated real-runtime process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet",
      "line": 5,
      "column": 5,
      "reason": "an isolated failed scheduler witness exits with no further runtime cleanup"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet",
      "line": 209,
      "column": 5,
      "reason": "fixture byte writes stay within owned 19/20-byte text buffers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet",
      "line": 213,
      "column": 5,
      "reason": "fixture byte writes stay within owned 19/20-byte text buffers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet",
      "line": 217,
      "column": 5,
      "reason": "fixture byte writes stay within owned 19/20-byte text buffers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet",
      "line": 221,
      "column": 5,
      "reason": "fixture byte writes stay within owned 19/20-byte text buffers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet",
      "line": 230,
      "column": 5,
      "reason": "fixture byte writes stay within an owned one-byte text buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet",
      "line": 234,
      "column": 5,
      "reason": "fixture byte writes stay within an owned one-byte text buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 6,
      "column": 5,
      "reason": "a failed isolated Secret witness terminates this test process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 14,
      "column": 5,
      "reason": "header is fresh aligned24B storage owned by the test; data owns cap times the host U8 stride bytes or is zero for an empty List"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 23,
      "column": 5,
      "reason": "the pointer is a live small allocation; this isolated test is the sole allocator user and can inspect its retained slab metadata"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 36,
      "column": 5,
      "reason": "input and secret are distinct live headers; adoption leaves one live buffer owner and borrowed bytes alias only that parent's header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 57,
      "column": 5,
      "reason": "the test owns data and checks the initialized U8 slots, including epoch padding, before any release"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 68,
      "column": 5,
      "reason": "secret still owns its live24B header after Close; the buffer was released, and no pointer into it is used"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 80,
      "column": 5,
      "reason": "reused is newly allocated live32B storage exclusively owned by this witness, not a read of freed storage"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 112,
      "column": 5,
      "reason": "data is this test's live List allocation; every capacity byte is dirtied before the wipe"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 120,
      "column": 5,
      "reason": "the same live allocation remains exclusively owned while this test checks all wiped bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 140,
      "column": 5,
      "reason": "data is this test's fresh live allocation of bytes bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 152,
      "column": 5,
      "reason": "secret is a live Secret header over its own buffer; data stays live through the retained reference"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 166,
      "column": 5,
      "reason": "data is still live: the other holder's reference was never released"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 177,
      "column": 5,
      "reason": "frozen is a live Secret header over its own buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 217,
      "column": 5,
      "reason": "both buffers are this test's fresh live allocations of bytes bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 235,
      "column": 5,
      "reason": "kept is a live Secret header and kept_data its live buffer; the scrub only zeroes the buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 258,
      "column": 5,
      "reason": "this isolated native witness writes its17B success literal and exits without introducing any text or collection runtime dependency"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SentryInlineTests.jet",
      "line": 8,
      "column": 1,
      "reason": "the raw pointer deliberately escapes its frame for the sentry witness"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SentryInlineTests.jet",
      "line": 14,
      "column": 1,
      "reason": "an expired inline raw read must stop before any pointee byte is loaded"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SentryTests.jet",
      "line": 211,
      "column": 5,
      "reason": "the checked Source callback uses the exact two-word native ABI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/TextTests.jet",
      "line": 6,
      "column": 5,
      "reason": "a failed text runtime witness terminates only its isolated process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
      "line": 8,
      "column": 5,
      "reason": "a failed isolated runtime witness ends the process without allocation or libc"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
      "line": 36,
      "column": 5,
      "reason": "gettid verifies identity without trusting the TCB under test"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
      "line": 45,
      "column": 5,
      "reason": "ARCH_GET_GS writes a caller-owned word; the inherited pointer is compared, never dereferenced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
      "line": 204,
      "column": 13,
      "reason": "sched_yield widens both sides of the check-to-wait race without timing assumptions"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
      "line": 284,
      "column": 5,
      "reason": "heap-free fixture success line is one eight-byte write"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/UnicodeTests.jet",
      "line": 8,
      "column": 5,
      "reason": "a failed Unicode runtime witness ends only its isolated test process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetSema/Tests/ScalarGlobalFixtures.jet",
      "line": 21,
      "column": 5,
      "reason": "single-threaded test: plain module writes of each scalar type"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetSema/Tests/XmlCheckerRootsFixtures.jet",
      "line": 10,
      "column": 5,
      "reason": "no raw operations in this regression"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetSema/Tests/XmlCheckerRootsFixtures.jet",
      "line": 11,
      "column": 9,
      "reason": "nested tail retains its value grammar"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Source/Exec.jet",
      "line": 345,
      "column": 5,
      "reason": "the testkit stops its own step child at the resident-memory guard"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Source/Exec.jet",
      "line": 498,
      "column": 9,
      "reason": "the testkit stops a process its own case left running in the case scratch"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Source/StepsControl.jet",
      "line": 133,
      "column": 5,
      "reason": "kill(pid, 0) observes the case-owned child without sending a signal"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Source/StepsSessions.jet",
      "line": 363,
      "column": 5,
      "reason": "the case explicitly authorizes signalling its own spawned child"
    },
    {
      "package": "Core",
      "file": "Core/Auth/Auth.jet",
      "line": 113,
      "column": 5,
      "reason": "auth PASETO v4.public verification uses the fixed Ed25519 primitive"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 320,
      "column": 5,
      "reason": "vault key import: validate a serialized wrapped carrier at the typed crypto boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 326,
      "column": 5,
      "reason": "vault key import: expose the authenticated wrapped carrier, not plaintext key material"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 331,
      "column": 5,
      "reason": "vault key import: construct the typed recipient unlock request"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 336,
      "column": 5,
      "reason": "vault key import: construct the typed passphrase unlock request"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 715,
      "column": 5,
      "reason": "the expert API explicitly authorizes authenticated legacy-v1 migration"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 1260,
      "column": 5,
      "reason": "crypto expert: Argon2id password hashing"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 1287,
      "column": 5,
      "reason": "crypto expert: Argon2id password verification"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 2131,
      "column": 5,
      "reason": "crypto expert: the authenticated envelope owns the one-time Poly1305 key and MAC framing"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 2244,
      "column": 5,
      "reason": "crypto expert: typed key agreement validates key widths and rejects all-zero shared secrets"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 2253,
      "column": 5,
      "reason": "crypto expert: typed signing keys validate the Ed25519 seed width"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 2259,
      "column": 5,
      "reason": "crypto expert: typed verification validates public key and signature widths before strict verification"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/ExpertCt.jet",
      "line": 13,
      "column": 5,
      "reason": "Core fixed-limb workspace reserves its complete operation-owned word capacity before filling"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/ExpertCt.jet",
      "line": 39,
      "column": 5,
      "reason": "Core fixed-limb extraction reserves its exact public output width before filling"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/ExpertCt.jet",
      "line": 325,
      "column": 5,
      "reason": "Core Ed25519 hashing reserves the exact combined public part lengths before filling"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 98,
      "column": 5,
      "reason": "Core JETC2 applies the fixed XChaCha20-Poly1305 suite and protocol-owned nonce/AAD domains"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 106,
      "column": 5,
      "reason": "Core JETC2 authenticates a bounded record before releasing its plaintext"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 114,
      "column": 5,
      "reason": "Core JETC2 borrows shared material through expert, wiping its copy before propagating HKDF success or failure"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 129,
      "column": 5,
      "reason": "Core JETC2 exposes only an operation-scoped key copy and wipes it before either seal outcome escapes"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 144,
      "column": 5,
      "reason": "Core JETC2 exposes only an operation-scoped key copy and wipes it before either authentication outcome escapes"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 159,
      "column": 5,
      "reason": "Core JETC2 copies its derived wrap nonce and file key only for sealing, wiping both copies on either outcome"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 177,
      "column": 5,
      "reason": "Core JETC2 copies its derived wrap nonce only for authentication, wiping the copy on either outcome"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 235,
      "column": 5,
      "reason": "Core JETC2 checks secret draw widths through the audited exposure and wipes the temporary copy"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 273,
      "column": 5,
      "reason": "Core JETC2 copies a validated ephemeral Secret into a zeroizing X25519SecretKey; both owners retain their Close duty"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 230,
      "column": 5,
      "reason": "vault typed load provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 237,
      "column": 5,
      "reason": "vault typed current provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 243,
      "column": 5,
      "reason": "vault typed status provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 250,
      "column": 5,
      "reason": "vault typed versions provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 257,
      "column": 5,
      "reason": "vault typed generation provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 264,
      "column": 5,
      "reason": "vault typed store provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 272,
      "column": 5,
      "reason": "vault key import: signing key material"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 280,
      "column": 5,
      "reason": "vault key import: X25519 secret material"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 288,
      "column": 5,
      "reason": "vault wrapped import provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 295,
      "column": 5,
      "reason": "vault typed rotation provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 302,
      "column": 5,
      "reason": "vault typed retirement provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 309,
      "column": 5,
      "reason": "vault typed revocation provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 316,
      "column": 5,
      "reason": "vault write authorization provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 323,
      "column": 5,
      "reason": "vault wrapped authorization provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 329,
      "column": 5,
      "reason": "vault typed generation commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 335,
      "column": 5,
      "reason": "vault typed store commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 341,
      "column": 5,
      "reason": "vault key import commit: signing key material"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 347,
      "column": 5,
      "reason": "vault key import commit: X25519 secret material"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 353,
      "column": 5,
      "reason": "vault wrapped import commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 359,
      "column": 5,
      "reason": "vault typed rotation commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 365,
      "column": 5,
      "reason": "vault typed retirement commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 371,
      "column": 5,
      "reason": "vault typed revocation commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 377,
      "column": 5,
      "reason": "vault passphrase export provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 384,
      "column": 5,
      "reason": "vault recipient export provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Data/Data.jet",
      "line": 2441,
      "column": 9,
      "reason": "native path bytes originate from the host environment and are split at ASCII boundaries"
    },
    {
      "package": "Core",
      "file": "Core/Data/Data.jet",
      "line": 3340,
      "column": 5,
      "reason": "inspect existing private Decode carriers only inside checked Core data source"
    },
    {
      "package": "Core",
      "file": "Core/Data/Data.jet",
      "line": 3674,
      "column": 5,
      "reason": "inspect existing private Decode carriers only inside checked Core data source"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1468,
      "column": 9,
      "reason": "this consuming Close owns the sole SQLite database pointer, including open-error handles; close_v2 permits live statements to finalize later"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1475,
      "column": 9,
      "reason": "this consuming Close owns the sole statement pointer; SQLite permits finalize(NULL) and rusqlite drops finalize's status"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1503,
      "column": 1,
      "reason": "SQLite owns a live NUL-terminated error/name buffer; strlen and unpack finish before any following engine call"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1509,
      "column": 5,
      "reason": "copy this exclusively borrowed connection's error before reset/close can invalidate or overwrite it"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1525,
      "column": 5,
      "reason": "NUL-free filename goes through the canonical C String adapter; the output pointer slot belongs to this stack frame"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1549,
      "column": 5,
      "reason": "close the exclusively borrowed connection's sole SQLite pointer once; zeroing it below makes the later Close a no-op"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1556,
      "column": 5,
      "reason": "exclusive live database, constant transaction SQL, canonical C String adapter, and no callback/error output allocation"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1565,
      "column": 5,
      "reason": "read the exclusively borrowed SQLite connection's autocommit flag"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1589,
      "column": 5,
      "reason": "key stays alive and unmodified while SQLite borrows its UTF-8 data+byte length; the output slots are stack-owned"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1622,
      "column": 5,
      "reason": "reset this exclusively owned statement and clear the TRANSIENT bindings before returning it to its owning cache"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1633,
      "column": 5,
      "reason": "exclusive statement loan; scalar values are copied, borrowed String lives unmodified until SQLITE_TRANSIENT finishes, and packed blobs are freed only after SQLite copies them"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1685,
      "column": 5,
      "reason": "step/reset the exclusive statement, matching rusqlite execute_with_bound_parameters, then copy any SQLite error before another operation"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1716,
      "column": 1,
      "reason": "value is alive and unchanged while reading its UTF-8 byte count; no borrowed pointer escapes"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1740,
      "column": 5,
      "reason": "copy every SQLite column into an owned value while its row remains live, before the next step/reset/finalize; byte unpack reads only column_bytes bytes"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1915,
      "column": 5,
      "reason": "SQLite's allocator counter is read-only; this test owns its connections and retains no foreign column pointers"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1919,
      "column": 5,
      "reason": "all test handles have reached Close before reading SQLite's allocator counter"
    },
    {
      "package": "Core",
      "file": "Core/Db/Db.jet",
      "line": 1924,
      "column": 5,
      "reason": "failed opens, including non-null error handles, have already reached Close"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Cbor.jet",
      "line": 99,
      "column": 5,
      "reason": "inspect private typed carriers without changing public Dyn variants"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Cbor.jet",
      "line": 168,
      "column": 5,
      "reason": "validated private carriers retain byte strings for typed Encode"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/CborStream.jet",
      "line": 218,
      "column": 9,
      "reason": "the retained key copies exactly the already validated text bytes"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/CborStream.jet",
      "line": 321,
      "column": 17,
      "reason": "the exact CBOR text allocation passed strict UTF-8 validation"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Csv.jet",
      "line": 141,
      "column": 9,
      "reason": "input is a String; original complete UTF-8 scalars and ASCII quote escapes entered the field"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Csv.jet",
      "line": 379,
      "column": 5,
      "reason": "the exact owned query bytes passed the shared strict UTF-8 validator"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Csv.jet",
      "line": 399,
      "column": 5,
      "reason": "a String's bytes cut at scalar boundaries, plus ASCII quotes, are complete UTF-8"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Csv.jet",
      "line": 871,
      "column": 9,
      "reason": "each completed UTF-8 scalar was validated before its original bytes entered this field"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Eetf.jet",
      "line": 549,
      "column": 9,
      "reason": "inspect private typed carriers without changing public Dyn variants"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Json.jet",
      "line": 113,
      "column": 5,
      "reason": "preserve the shipped decoder's hidden stderr effect at its existing host boundary"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Json.jet",
      "line": 121,
      "column": 5,
      "reason": "preserve the shipped decoder's hidden wall-clock snapshot at its existing host boundary"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Json.jet",
      "line": 726,
      "column": 13,
      "reason": "preserve the native compiler-private Dyn carriers in Core JSON event projection"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Json.jet",
      "line": 731,
      "column": 13,
      "reason": "preserve the native compiler-private Dyn carriers in Core JSON event projection"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 16,
      "column": 9,
      "reason": "the Object variant was checked immediately before borrowing its ordered backing"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 24,
      "column": 5,
      "reason": "transfer the owned ordered entries allocation into the native Object carrier"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 30,
      "column": 5,
      "reason": "check the private Bytes variant immediately before borrowing its backing"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 37,
      "column": 5,
      "reason": "the shared parser validated this exact numeric lexeme"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 43,
      "column": 5,
      "reason": "retain decoded JSON text until the target Decode contract is known"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 324,
      "column": 9,
      "reason": "the shared UTF-8 validator just accepted this owned byte allocation"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 395,
      "column": 9,
      "reason": "the shared UTF-8 validator just accepted this owned ASCII token"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 400,
      "column": 13,
      "reason": "retain a validated number lexeme in the private event carrier until typed decoding"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 602,
      "column": 9,
      "reason": "transfer already validated UTF-8 storage to inspect its actual owned capacity"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 613,
      "column": 9,
      "reason": "the private numeric event is projected only into the matching typed tree carrier"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 989,
      "column": 5,
      "reason": "the shared UTF-8 validator just accepted this owned allocation"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 1001,
      "column": 5,
      "reason": "the byte position was checked against this String's storage length"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 1565,
      "column": 5,
      "reason": "the shared UTF-8 validator accepted this exact owned formatter buffer"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 1576,
      "column": 5,
      "reason": "transfer the owned UTF-8 String buffer before re-spelling its shortest digits"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 1722,
      "column": 13,
      "reason": "retain a validated number lexeme in the private event carrier"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 1727,
      "column": 13,
      "reason": "project the native compiler-private Bytes carrier into its event"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 1793,
      "column": 13,
      "reason": "classify the private Bytes carrier without borrowing or exposing its payload"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 1936,
      "column": 5,
      "reason": "owned validated UTF-8 allocation transfers in the encoding source seam"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 1973,
      "column": 5,
      "reason": "exercise the typed Encode byte carrier at the JSON preflight boundary"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/XmlEngine.jet",
      "line": 132,
      "column": 5,
      "reason": "all callers iterate or check the UTF-8 byte bounds before borrowing this byte"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/XmlEngine.jet",
      "line": 170,
      "column": 5,
      "reason": "the caller selected a scalar-aligned window of validated UTF-8 bytes"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/XmlEngine.jet",
      "line": 2180,
      "column": 5,
      "reason": "attribute whitespace compaction preserves the already validated UTF-8 byte sequence"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Yaml.jet",
      "line": 440,
      "column": 5,
      "reason": "inspect compiler-private Bytes and typed carriers; public Dyn variants are unchanged"
    },
    {
      "package": "Core",
      "file": "Core/Files/Files.jet",
      "line": 498,
      "column": 1,
      "reason": "the compiler store passes a live FileReader and Directory, with one NUL-free plain destination name; no source pathname is reopened"
    },
    {
      "package": "Core",
      "file": "Core/Files/Files.jet",
      "line": 504,
      "column": 1,
      "reason": "compiler store owns the explicit child descriptor slot and retains the held Directory until the process leaf duplicates its descriptor"
    },
    {
      "package": "Core",
      "file": "Core/Http/Client.jet",
      "line": 655,
      "column": 5,
      "reason": "checked resolver octets and port transferred into the socket carrier"
    },
    {
      "package": "Core",
      "file": "Core/Jobs/Jobs.jet",
      "line": 133,
      "column": 5,
      "reason": "queue preflight reads final-component metadata without following a symlink; metadata failures are ignored exactly as the SQLite provider does"
    },
    {
      "package": "Core",
      "file": "Core/Jobs/Jobs.jet",
      "line": 138,
      "column": 5,
      "reason": "create the lexical parent with the existing raw recursive mkdir kernel before SQLite open"
    },
    {
      "package": "Core",
      "file": "Core/Jobs/Jobs.jet",
      "line": 363,
      "column": 13,
      "reason": "match the queue's existing raw wall-clock wait without advancing controlled scheduler time"
    },
    {
      "package": "Core",
      "file": "Core/Jobs/Jobs.jet",
      "line": 544,
      "column": 5,
      "reason": "publish already committed queue observations through the existing validated Devtools event/sink boundary"
    },
    {
      "package": "Core",
      "file": "Core/Jobs/Jobs.jet",
      "line": 559,
      "column": 5,
      "reason": "publish source-projected queue status through the existing validated Devtools event/sink boundary"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 36,
      "column": 1,
      "reason": "`addr` is the address of a live, aligned `T` that stays valid for every use of the returned pointer"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 40,
      "column": 1,
      "reason": "`pointer` names a live, aligned, readable `T` for the whole access"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 44,
      "column": 1,
      "reason": "`pointer` names a live, aligned, writable `T` for the whole access"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 137,
      "column": 5,
      "reason": "checked IPv4/IPv6 octets transferred into the native address carrier"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 143,
      "column": 5,
      "reason": "read native address octets; no parsing or formatting policy in the carrier leaf"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 149,
      "column": 5,
      "reason": "read the native IP address discriminant"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 206,
      "column": 5,
      "reason": "validated IP octets, port and IPv6 numeric scope transferred into the socket carrier"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 213,
      "column": 5,
      "reason": "read socket IP representation; formatting stays in Jet"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 216,
      "column": 5,
      "reason": "read native socket port"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 228,
      "column": 5,
      "reason": "read native IPv6 socket scope identifier"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 430,
      "column": 5,
      "reason": "read the native UDP packet's socket-address carrier"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 70,
      "column": 9,
      "reason": "this consuming Close owns the sole jet_tls context; jet_tls_free accepts 0 and never closes the socket"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 549,
      "column": 5,
      "reason": "the output slot belongs to this frame; jet_tls_new stores 0 or a fresh context that __TLSContext owns"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 942,
      "column": 9,
      "reason": "the packed copy lives until the matching free below; Mbed TLS only reads it during jet_tls_write"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 948,
      "column": 17,
      "reason": "frees the packed chunk exactly once after its last jet_tls_write"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 954,
      "column": 9,
      "reason": "frees the packed chunk exactly once after its last jet_tls_write"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1483,
      "column": 5,
      "reason": "configures the live context before setup"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1487,
      "column": 5,
      "reason": "configures the live context before setup"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1492,
      "column": 5,
      "reason": "NUL-free PEM text is borrowed for this parse; Mbed TLS copies the certificates"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1498,
      "column": 5,
      "reason": "the NUL-free path is borrowed for this parse; Mbed TLS copies the certificates"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1504,
      "column": 5,
      "reason": "the NUL-free path is borrowed for this parse; Mbed TLS copies the certificates"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1510,
      "column": 5,
      "reason": "reads the live context's root list"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1516,
      "column": 5,
      "reason": "NUL-free PEM texts are borrowed for this parse; Mbed TLS copies the chain and the key"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1522,
      "column": 5,
      "reason": "the NUL-free protocol is copied by jet_tls_alpn_add"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1528,
      "column": 5,
      "reason": "binds the live context to the TCPStream's descriptor, which the stream state keeps open for the context's lifetime"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1534,
      "column": 5,
      "reason": "one nonblocking handshake step on the live context"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1540,
      "column": 5,
      "reason": "one nonblocking read into the live context's own buffer"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1546,
      "column": 5,
      "reason": "jet_tls_read just filled `length` bytes of the context's buffer; they are copied before the next engine call"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1554,
      "column": 5,
      "reason": "the caller's packed chunk holds `length` readable bytes for this call"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1560,
      "column": 5,
      "reason": "one nonblocking close_notify step on the live context"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1566,
      "column": 5,
      "reason": "reads the live context's verification flags"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1572,
      "column": 5,
      "reason": "reads the live context's negotiated version"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1578,
      "column": 5,
      "reason": "the suite name is a static NUL-terminated string; its length and unpack finish before the next engine call"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1589,
      "column": 5,
      "reason": "the live context keeps its verified peer chain while the DER is copied out; the output slots belong to this frame"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1599,
      "column": 5,
      "reason": "getenv returns a NUL-terminated string or NULL; it is copied before any environment change"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1606,
      "column": 1,
      "reason": "the caller guarantees a live NUL-terminated string at `address`"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1616,
      "column": 5,
      "reason": "reads the run's journal mode word; no argument is borrowed"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1623,
      "column": 5,
      "reason": "the packed copy lives until the matching free below; the runtime copies it into the journal during the call"
    },
    {
      "package": "Core",
      "file": "Core/Net/Tls.jet",
      "line": 1634,
      "column": 5,
      "reason": "the packed request lives until the matching free below; the served address points into the run's loaded journal, copied out before return"
    },
    {
      "package": "Core",
      "file": "Core/Process/Process.jet",
      "line": 148,
      "column": 5,
      "reason": "transfer a fully initialized Jet process plan into the temporary native ProcessSpec carrier; paired environment columns have equal length"
    },
    {
      "package": "Core",
      "file": "Core/Service/Service.jet",
      "line": 1427,
      "column": 5,
      "reason": "read the final path component's OS metadata kind without opening an identity handle"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 423,
      "column": 1,
      "reason": "posix umask: the caller owns the process file-creation mask and restores it when other code relies on it"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 592,
      "column": 5,
      "reason": "posix getpgid: inspect a process group"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 598,
      "column": 5,
      "reason": "posix getsid: inspect a process session"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 603,
      "column": 1,
      "reason": "posix getpriority: the caller vouches that `which` names this process or one it controls"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 610,
      "column": 1,
      "reason": "posix setpriority: the caller owns the scheduling priority of the process it changes"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 614,
      "column": 1,
      "reason": "posix utime: the caller owns the file whose timestamps it rewrites"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 621,
      "column": 1,
      "reason": "posix stop: the caller accepts that the process ends here after the shared cleanup boundary"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 631,
      "column": 1,
      "reason": "posix pipe: the caller owns both returned descriptors and closes each exactly once"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 637,
      "column": 1,
      "reason": "posix close_fd: the caller owns `fd` and nothing uses it after this call"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 652,
      "column": 1,
      "reason": "posix mkfifo: the caller owns `path` and removes the named pipe it creates"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 657,
      "column": 1,
      "reason": "posix fork: the caller handles both the parent and the child return and reaps the child"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 661,
      "column": 1,
      "reason": "posix setsid: the caller owns the process session topology it changes"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 665,
      "column": 1,
      "reason": "posix wait: the caller owns the child processes it reaps"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 671,
      "column": 1,
      "reason": "posix waitpid: the caller owns the child process it reaps"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 677,
      "column": 1,
      "reason": "posix setuid: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 681,
      "column": 1,
      "reason": "posix setgid: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 685,
      "column": 1,
      "reason": "posix setpgrp: the caller owns the process group topology it changes"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 689,
      "column": 1,
      "reason": "posix setpgid: the caller owns the process group topology it changes"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 693,
      "column": 1,
      "reason": "posix kill: the caller owns or is entitled to signal the target process"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 697,
      "column": 1,
      "reason": "posix initgroups: the caller changes supplementary groups deliberately"
    },
    {
      "package": "Core",
      "file": "Core/Testing/Testing.jet",
      "line": 1355,
      "column": 5,
      "reason": "the caller bounded this byte position by the String storage length"
    },
    {
      "package": "Core",
      "file": "Core/Testing/Testing.jet",
      "line": 1376,
      "column": 5,
      "reason": "all appended bytes came from complete UTF-8 strings or ASCII"
    },
    {
      "package": "Core",
      "file": "Core/Tui/Tui.jet",
      "line": 104,
      "column": 5,
      "reason": "Core TUI reads owned encoded OS environment bytes from the current host; no UTF-8 assumption"
    },
    {
      "package": "Core",
      "file": "Core/Tui/Tui.jet",
      "line": 130,
      "column": 5,
      "reason": "Core TUI samples the same stdout terminal fact consumed by terminal style/progress"
    },
    {
      "package": "Core",
      "file": "Core/Tui/Tui.jet",
      "line": 656,
      "column": 5,
      "reason": "Core TUI test: normalized style words and explicit absent UI slots"
    },
    {
      "package": "Core",
      "file": "Core/Ui/Ui.jet",
      "line": 159,
      "column": 5,
      "reason": "Core UI text input: the provider sets the explicitly requested IME slot"
    },
    {
      "package": "Core",
      "file": "Core/Ui/Ui.jet",
      "line": 196,
      "column": 5,
      "reason": "Core creates the real retained GTK renderer through the registered host allocation boundary"
    },
    {
      "package": "Core",
      "file": "Core/Ui/Ui.jet",
      "line": 201,
      "column": 5,
      "reason": "Core creates the real retained terminal renderer through the registered host allocation boundary"
    },
    {
      "package": "Core",
      "file": "Core/Ui/Ui.jet",
      "line": 206,
      "column": 5,
      "reason": "Core creates the real retained null renderer through the registered host allocation boundary"
    },
    {
      "package": "Core",
      "file": "Core/Ui/Ui.jet",
      "line": 217,
      "column": 5,
      "reason": "Core UI accessibility slot transfer: explicitly present validated metadata"
    },
    {
      "package": "Core",
      "file": "Core/Ui/Ui.jet",
      "line": 234,
      "column": 5,
      "reason": "Core UI shortcut slot transfer: explicitly present validated shortcut"
    },
    {
      "package": "Core",
      "file": "Core/Ui/Ui.jet",
      "line": 261,
      "column": 5,
      "reason": "Core UI host slot transfer: audited kind, role, explicit absent slots, and normalized style words"
    },
    {
      "package": "Core",
      "file": "Core/Vendor/Sqlite3/Fixture/run.jet",
      "line": 19,
      "column": 5,
      "reason": "exclusive out-slots hold SQLite pointers; zero words mean null, SQL is NUL-terminated for each borrowed call, and each live pointer is finalized/closed once"
    },
    {
      "package": "Docs",
      "file": "Docs/audits/g15-2026-11/probes/c4599_28_comptime_asm.jet",
      "line": 10,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Docs",
      "file": "Docs/audits/g15-2026-11/probes/c4599_28_comptime_asm.jet",
      "line": 25,
      "column": 5,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Docs",
      "file": "Docs/audits/g15-2026-11/probes/c4599_28_comptime_asm_reach.jet",
      "line": 10,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Docs",
      "file": "Docs/audits/g15-2026-11/probes/c4599_28_comptime_asm_reach.jet",
      "line": 22,
      "column": 9,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Docs",
      "file": "Docs/audits/g15-2026-11/probes/c4599_28_runtime_asm_reach.jet",
      "line": 10,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Docs",
      "file": "Docs/audits/g15-2026-11/probes/c4599_28_runtime_asm_reach.jet",
      "line": 22,
      "column": 9,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Docs",
      "file": "Docs/proposals/jai-style-sample/Data.jet",
      "line": 1274,
      "column": 9,
      "reason": "native path bytes originate from the host environment and are split at ASCII boundaries"
    },
    {
      "package": "Docs",
      "file": "Docs/proposals/jai-style-sample/Data.jet",
      "line": 1886,
      "column": 5,
      "reason": "inspect existing private Decode carriers only inside checked Core data source"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/crypto_migration.jet",
      "line": 33,
      "column": 5,
      "reason": "AES-256-GCM protocol interoperability"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/crypto_suite.jet",
      "line": 38,
      "column": 5,
      "reason": "known-answer check reads the RFC 7748 shared secret"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/expert_projection.jet",
      "line": 35,
      "column": 5,
      "reason": "expert cryptography is an explicit opt-in"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/primitive_matrix.jet",
      "line": 104,
      "column": 5,
      "reason": "known-answer check reads the RFC 7748 shared secret"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/primitive_matrix.jet",
      "line": 166,
      "column": 5,
      "reason": "expert known-answer vectors need raw key bytes"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/random_api_split.jet",
      "line": 35,
      "column": 5,
      "reason": "compare the typed and raw HKDF rungs"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/vault_keys.jet",
      "line": 43,
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
      "file": "Examples/features/errors/transact_os_stop.jet",
      "line": 33,
      "column": 5,
      "reason": "the stop runs the shared cleanup boundary"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/cleanup_matrix.jet",
      "line": 35,
      "column": 5,
      "reason": "the stop runs the shared cleanup boundary"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/cleanup_matrix.jet",
      "line": 80,
      "column": 5,
      "reason": "the exit callbacks have no captured state"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/os_process_control.jet",
      "line": 31,
      "column": 5,
      "reason": "POSIX process and pipe control for the core.sys surface"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/os_stop_cleanup.jet",
      "line": 26,
      "column": 5,
      "reason": "the stop callback uses the shared cleanup boundary"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/process_exit_cleanup.jet",
      "line": 22,
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
      "line": 21,
      "column": 5,
      "reason": "the clock file was created above in this test-owned directory"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/system_projection.jet",
      "line": 27,
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
      "file": "Examples/features/lowlevel/mem_from_addr_direct.jet",
      "line": 11,
      "column": 5,
      "reason": "the borrowed value is live in the caller and the pointer never escapes"
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
      "line": 36,
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
      "line": 22,
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
      "file": "Examples/features/text/checked_text_runtime_hole.jet",
      "line": 24,
      "column": 5,
      "reason": "checked text example output"
    },
    {
      "package": "Examples",
      "file": "Examples/features/text/checked_text_runtime_hole_stop.jet",
      "line": 23,
      "column": 5,
      "reason": "checked text example output"
    },
    {
      "package": "Examples",
      "file": "Examples/features/tooling/checked_text_type.jet",
      "line": 20,
      "column": 5,
      "reason": "checked text example output"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Foundation/Source/TreeHash.jet",
      "line": 569,
      "column": 9,
      "reason": "the authority retains the held directory until the process provider duplicates its descriptor; the child descriptor slot is supplied by the compiler"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Foundation/Source/TreeHash.jet",
      "line": 745,
      "column": 5,
      "reason": "source-store linkat uses the live held reader and directory, a validated plain name, and no source pathname; raw syscall facts are classified here"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Provider/Batteries/embedded/embedded.jet",
      "line": 181,
      "column": 9,
      "reason": "host replay uses a stack cell for the CMSDK UART0 control register and never dereferences the target address"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 67,
      "column": 5,
      "reason": "export a fresh Core signing seed only to descriptor-relative owner-only key publication; the Core key owner is wiped on every outcome"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 122,
      "column": 5,
      "reason": "import the protected seed into Core signing without a second raw-secret copy; close the temporary Core Secret on success and failure"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 146,
      "column": 5,
      "reason": "package author signatures require Core's strict Ed25519 verification; no raw verifier or helper process"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Publication/Tests/Index.jet",
      "line": 291,
      "column": 5,
      "reason": "the test owns the planted lock file it ages"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Store/Tests/ClosureGraph.jet",
      "line": 59,
      "column": 13,
      "reason": "the test owns the journal file it just wrote"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Store/Tests/Seal.jet",
      "line": 31,
      "column": 5,
      "reason": "the store test owns the fixture file whose mtime it pins"
    },
    {
      "package": "Tools",
      "file": "Tools/agent-eval/domain-foundations/area-embedded/pkg/mmio_fixed.jet",
      "line": 4,
      "column": 5,
      "reason": "board.sensor_v1 GPIO register 0x40000100 is a volatile MMIO word"
    },
    {
      "package": "Tools",
      "file": "Tools/agent-eval/domain-foundations/area-embedded/pkg/run.jet",
      "line": 111,
      "column": 5,
      "reason": "host replay uses a stack cell as a stand-in for the board register"
    },
    {
      "package": "Tools",
      "file": "Tools/agent/cache/Lanes.jet",
      "line": 305,
      "column": 21,
      "reason": "the guard signals only compiler, build and test jobs of this user, chosen by name from /proc"
    },
    {
      "package": "Tools",
      "file": "Tools/ci/CompiledWorkloadPeerLauncher.jet",
      "line": 589,
      "column": 9,
      "reason": "posix kill: every process in the sandbox's private PID namespace except its init and this supervisor belongs to the supervised command"
    },
    {
      "package": "Tools",
      "file": "Tools/ci/CompiledWorkloadPeerLauncher.jet",
      "line": 601,
      "column": 5,
      "reason": "libc signal + posix kill: restore SIG_DFL for one signal number, then re-raise it on this process so the caller observes the supervised command's signal"
    },
    {
      "package": "Tools",
      "file": "Tools/ci/CompiledWorkloadPeerProbe.jet",
      "line": 98,
      "column": 5,
      "reason": "posix kill: the probe ends itself with SIGTERM so the check observes the launcher propagating a signal death"
    },
    {
      "package": "Tools",
      "file": "Tools/packaging/NativeCCompiler.jet",
      "line": 120,
      "column": 9,
      "reason": "re-raise the compiler's terminating signal on this process, as the parent shell expects"
    },
    {
      "package": "tests",
      "file": "tests/cases/cffi/cffi_raw_status_out_pointer_reads_only_on_success.fixture/main.jet",
      "line": 16,
      "column": 5,
      "reason": "store_load receives a live non-null slot; bytes are read only after status zero"
    },
    {
      "package": "tests",
      "file": "tests/cases/cffi/concurrent_bridges_share_one_target.fixture/take0/main.jet",
      "line": 7,
      "column": 5,
      "reason": "the foreign call may write through the borrowed value"
    },
    {
      "package": "tests",
      "file": "tests/cases/cffi/ffi_capability_boundary_is_safe_only_inside_unsafe/input/o2/run.jet",
      "line": 7,
      "column": 5,
      "reason": "the foreign call may write through the borrowed value"
    },
    {
      "package": "tests",
      "file": "tests/cases/cffi/ffi_capability_boundary_is_safe_only_inside_unsafe/input/o3/run.jet",
      "line": 7,
      "column": 5,
      "reason": "the foreign call may write through the borrowed value"
    },
    {
      "package": "tests",
      "file": "tests/cases/cffi_native_matrix/fixtures/required_native_c_abi_matrix/default/matrix.jet",
      "line": 46,
      "column": 5,
      "reason": "live non-null out slot; read only after status zero"
    },
    {
      "package": "tests",
      "file": "tests/cases/cffi_native_matrix/fixtures/required_native_c_abi_matrix/sysv64/matrix.jet",
      "line": 46,
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
      "file": "tests/cases/collections_and_methods/sentry_typed_int_store_keeps_exact_carrier.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "the typed pointer refers to the live local cell for these stores"
    },
    {
      "package": "tests",
      "file": "tests/cases/comptime_cli_parity/comptime_child_is_reaped_before_the_program_runs.fixture/run.jet",
      "line": 15,
      "column": 5,
      "reason": "fork and reap the child before returning"
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
      "file": "tests/cases/comptime_core_pure_parity/crypto_expert_aead_sign_argon_aot_stdout_matches_known_answers.fixture/main.jet",
      "line": 8,
      "column": 5,
      "reason": "fixed RFC interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/cases/comptime_core_pure_parity/crypto_values_match_aot_comptime_forced_interpreter_and_default_dev/input/o1/run.jet",
      "line": 11,
      "column": 5,
      "reason": "fixed RFC interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/cases/comptime_core_pure_parity/crypto_values_match_aot_comptime_forced_interpreter_and_default_dev/input/o2/run.jet",
      "line": 11,
      "column": 5,
      "reason": "fixed RFC interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/cases/control_and_data/explicit_process_exit_cleanup_golden_matches_all_execution_tiers.fixture/process_exit_cleanup.jet",
      "line": 22,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "tests",
      "file": "tests/cases/core_crypto_argon2id/argon2id_parallel_lanes_match_rfc_vector_across_tiers/input/run.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed Argon2id parallel-lane known-answer vector"
    },
    {
      "package": "tests",
      "file": "tests/cases/custom/fixtures/from_addr_sentry_is_tier_parity/fixture.jet",
      "line": 6,
      "column": 5,
      "reason": "value is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_alignment/unsafe_sentries_alignment.jet",
      "line": 4,
      "column": 5,
      "reason": "the tuple storage is live but this raw Int is misaligned"
    },
    {
      "package": "tests",
      "file": "tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_expired_recursive_stack/unsafe_sentries_expired_recursive_stack.jet",
      "line": 11,
      "column": 5,
      "reason": "the recursive address outlives its owning Jet frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_expired_stack/unsafe_sentries_expired_stack.jet",
      "line": 8,
      "column": 5,
      "reason": "the returned address outlives its owning Jet frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_misaligned_list/unsafe_sentries_misaligned_list.jet",
      "line": 5,
      "column": 5,
      "reason": "the list storage is live but this raw Int is misaligned"
    },
    {
      "package": "tests",
      "file": "tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_provenance/unsafe_sentries_provenance.jet",
      "line": 6,
      "column": 5,
      "reason": "external address must be a live allocation"
    },
    {
      "package": "tests",
      "file": "tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_quarantine/unsafe_sentries_quarantine.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "tests",
      "file": "tests/cases/custom/fixtures/sentry_source_policy_off_is_tier_parity/fixture.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "tests",
      "file": "tests/cases/dev_parts_routine/jit_1216_adversarial_regressions.fixture/raw_alias/run.jet",
      "line": 5,
      "column": 5,
      "reason": "the pointer stays inside this stack frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/dev_parts_routine/unsafe_blocks_are_evaluated_with_live_sentries.fixture/fault.jet",
      "line": 3,
      "column": 5,
      "reason": "mapped address is valid and aligned"
    },
    {
      "package": "tests",
      "file": "tests/cases/diagnostics_coverage/sentry_metadata_uses_the_returned_fault_exit_code.fixture/app/main.jet",
      "line": 3,
      "column": 5,
      "reason": "calls the audited dependency"
    },
    {
      "package": "tests",
      "file": "tests/cases/diagnostics_coverage/sentry_metadata_uses_the_returned_fault_exit_code.fixture/dep/dep.jet",
      "line": 2,
      "column": 1,
      "reason": "intentionally invalid address"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/argon2id_rfc_and_expert_known_answers_kill_argon2i_mutation.fixture/run.jet",
      "line": 39,
      "column": 5,
      "reason": "original public expert Argon2id KAT"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/expert_aes256gcm_matches_nist_cavs_vector.fixture/run.jet",
      "line": 39,
      "column": 5,
      "reason": "original published known-answer vector"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/expert_surface_matches_reference_vectors_and_bounds.fixture/run.jet",
      "line": 44,
      "column": 5,
      "reason": "original expert interoperability vectors"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/expert_xchacha20poly1305_matches_cfrg_vector.fixture/run.jet",
      "line": 39,
      "column": 5,
      "reason": "original published known-answer vector"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/jetc_v1_expert_open_accepts_only_the_pinned_grammar.fixture/run.jet",
      "line": 42,
      "column": 5,
      "reason": "original pinned JETC v1 migration grammar"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/jetc_v1_migration_reseals_to_verified_v2_without_touching_source.fixture/run.jet",
      "line": 23,
      "column": 5,
      "reason": "crypto expert: the fixture seals a pinned JETC v1 envelope and migrates it"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/keygen_zeroizes_provider_copy_and_stack_seed.fixture/run.jet",
      "line": 41,
      "column": 5,
      "reason": "crypto expert: the test reads the generated seed to prove it is live and 32 bytes"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/prior_crypto_suite_still_uses_live_entropy.fixture/run.jet",
      "line": 44,
      "column": 5,
      "reason": "crypto expert: the test reads the generated seed length"
    },
    {
      "package": "tests",
      "file": "tests/cases/fixtures_crypto_bridge_entropy/typed_secret_wrap_sign_kdf_and_password_paths_preserve_roles.fixture/run.jet",
      "line": 51,
      "column": 5,
      "reason": "crypto expert: the test derives from the secret's raw bytes"
    },
    {
      "package": "tests",
      "file": "tests/cases/fmt/fmt_preserves_checked_text_type/input/run.jet",
      "line": 19,
      "column": 5,
      "reason": "formatter fixture"
    },
    {
      "package": "tests",
      "file": "tests/cases/fmt/fmt_preserves_unsafe_site_modes_and_postfix_obligations/input/run.jet",
      "line": 3,
      "column": 1,
      "reason": "caller keeps address live"
    },
    {
      "package": "tests",
      "file": "tests/cases/fmt/fmt_preserves_unsafe_site_modes_and_postfix_obligations/input/run.jet",
      "line": 13,
      "column": 5,
      "reason": "calling audited reader"
    },
    {
      "package": "tests",
      "file": "tests/cases/fmt/fmt_unsafe_reasons_escape_strings/input/run.jet",
      "line": 3,
      "column": 1,
      "reason": "caller says \"ok\""
    },
    {
      "package": "tests",
      "file": "tests/cases/fmt/fmt_unsafe_reasons_escape_strings/input/run.jet",
      "line": 8,
      "column": 5,
      "reason": "path C:\\tmp"
    },
    {
      "package": "tests",
      "file": "tests/cases/fmt_project/raw_c_imports_and_module_bindings_remain_clean_through_cli.fixture/runtime.jet",
      "line": 7,
      "column": 1,
      "reason": "audited C boundary"
    },
    {
      "package": "tests",
      "file": "tests/cases/fmt_project/raw_c_imports_and_module_bindings_remain_clean_through_cli.fixture/runtime.jet",
      "line": 10,
      "column": 1,
      "reason": "audited C boundary"
    },
    {
      "package": "tests",
      "file": "tests/cases/fmt_round_trip_regressions/raw_c_imports_and_module_binding_mutability_are_byte_exact_and_idempotent/input/run.jet",
      "line": 7,
      "column": 1,
      "reason": "audited C boundary"
    },
    {
      "package": "tests",
      "file": "tests/cases/fmt_round_trip_regressions/raw_c_imports_and_module_binding_mutability_are_byte_exact_and_idempotent/input/run.jet",
      "line": 10,
      "column": 1,
      "reason": "audited C boundary"
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
      "file": "tests/cases/liveness_lints/fallback_err_is_never_an_unused_local.fixture/main.jet",
      "line": 3,
      "column": 1,
      "reason": "probe"
    },
    {
      "package": "tests",
      "file": "tests/cases/liveness_lints/fallback_err_is_never_an_unused_local.fixture/main.jet",
      "line": 8,
      "column": 5,
      "reason": "probe"
    },
    {
      "package": "tests",
      "file": "tests/cases/memory_semantics/closure_boxes_free_at_their_last_owner.fixture/main.jet",
      "line": 318,
      "column": 5,
      "reason": "the exit callback only reads its captured List"
    },
    {
      "package": "tests",
      "file": "tests/cases/memory_semantics/unsafe_address_keeps_string_alive_to_block_end.fixture/main.jet",
      "line": 9,
      "column": 5,
      "reason": "label is live on this frame until this block ends"
    },
    {
      "package": "tests",
      "file": "tests/cases/observe/panic_context_uses_only_lexically_live_locals.fixture/unsafe/panic_lexical_unsafe.jet",
      "line": 3,
      "column": 5,
      "reason": "the region only binds and prints a local; it reads and writes no raw memory"
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
      "file": "tests/cases/place_window_lifetimes/borrowed_window_stays_caller_owned.fixture/main.jet",
      "line": 5,
      "column": 5,
      "reason": "the borrowed value is live in the caller"
    },
    {
      "package": "tests",
      "file": "tests/cases/place_window_lifetimes/heap_element_window_stays_allocator_owned.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "the list element remains heap-owned"
    },
    {
      "package": "tests",
      "file": "tests/cases/place_window_lifetimes/stack_window_uses_expiring_sentry_address.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "cell is live on this stack frame"
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
      "file": "tests/cases/process_sessions/subjects/native/session_fixture.jet",
      "line": 53,
      "column": 5,
      "reason": "the fixture's descendant rejoins the process group of the tree that spawned it"
    },
    {
      "package": "tests",
      "file": "tests/cases/process_sessions/subjects/native/session_fixture.jet",
      "line": 95,
      "column": 9,
      "reason": "kill -0 only asks whether the killed descendant still exists"
    },
    {
      "package": "tests",
      "file": "tests/cases/run_entry/module_write_from_fn_is_e0111.fixture/ok.jet",
      "line": 8,
      "column": 5,
      "reason": "this program starts no task, so nothing else writes `raw`"
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
      "file": "tests/cases/unsafe_and_runtime/every_unsafe_region_kind_enters_its_own_sentry_scope.fixture/regions.jet",
      "line": 1,
      "column": 1,
      "reason": "the cell is live on this frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/every_unsafe_region_kind_enters_its_own_sentry_scope.fixture/regions.jet",
      "line": 9,
      "column": 5,
      "reason": "call the gated reader"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/every_unsafe_region_kind_enters_its_own_sentry_scope.fixture/regions.jet",
      "line": 15,
      "column": 5,
      "reason": "test region calls the gated reader"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/ffi_capability_edge_uses_shared_sentry_prelude/main.jet",
      "line": 7,
      "column": 5,
      "reason": "the foreign call may write through the borrowed value"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/paren/main.jet",
      "line": 2,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be a live, valid Int"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/paren/main.jet",
      "line": 10,
      "column": 5,
      "reason": "addr is the address of `cell`, a live Int on this stack frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/plain/main.jet",
      "line": 2,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be a live, valid Int"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/plain/main.jet",
      "line": 10,
      "column": 5,
      "reason": "addr is the address of `cell`, a live Int on this stack frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/sentry_kernel_is_shared_by_aot_and_runtime_adapters/main.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/unsafe_fn_block_and_ptr_ops/main.jet",
      "line": 2,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be a live, valid Int"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/unsafe_fn_block_and_ptr_ops/main.jet",
      "line": 10,
      "column": 5,
      "reason": "addr is the address of `cell`, a live Int on this stack frame"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/unsafe_tier_emit_is_byte_exact/main.jet",
      "line": 2,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be valid"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/unsafe_tier_emit_is_byte_exact/main.jet",
      "line": 10,
      "column": 5,
      "reason": "safe: cell is live"
    },
    {
      "package": "tests",
      "file": "tests/cases/unsafe_and_runtime/fixtures/volatile_write_emit_is_byte_exact/main.jet",
      "line": 2,
      "column": 1,
      "reason": "UART TX register is mapped by the target machine"
    },
    {
      "package": "tests",
      "file": "tests/cases/web_build/module_local_dev_is_validated_as_web_runtime.fixture/main.jet",
      "line": 4,
      "column": 9,
      "reason": "exercise the web gate"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/aes/nist_gcm.jet",
      "line": 13,
      "column": 5,
      "reason": "published KAT or frozen seed AES-GCM fixture"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/aes/seed_differential.jet",
      "line": 13,
      "column": 5,
      "reason": "published KAT or frozen seed AES-GCM fixture"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 15,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 19,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 32,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 36,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 49,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 53,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 66,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 70,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 83,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 87,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 98,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 109,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 120,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 131,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 142,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 153,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 164,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 175,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 186,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 197,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 208,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 219,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 230,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 241,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 252,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 263,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 274,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 285,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 296,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 307,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 323,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 334,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 350,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 361,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 377,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 388,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 404,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 415,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 431,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 442,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 458,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 469,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 485,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 496,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 512,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 523,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 539,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 550,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 566,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 577,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 593,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 604,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 620,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 631,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 647,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 658,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 674,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 685,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 701,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 712,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 728,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 739,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 755,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 766,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 782,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 793,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 809,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 820,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 836,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 847,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 863,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 874,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 890,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 901,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 917,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 928,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 944,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 955,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 971,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 982,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 998,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1009,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1025,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1036,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1052,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1063,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1079,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1090,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1106,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1117,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1133,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1144,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1160,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1171,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1187,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1198,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1214,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1225,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1241,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1252,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1268,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1279,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1295,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1306,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1322,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1333,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1344,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1355,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1366,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1377,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1388,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1399,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1410,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1426,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1442,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1458,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1476,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1480,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1493,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1497,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1510,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1514,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1527,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1531,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1544,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1548,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1561,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1565,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1578,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1582,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1595,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1599,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1612,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1616,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1629,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1633,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1646,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1650,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1663,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1667,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1680,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1684,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1697,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1701,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1714,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1718,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1731,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1735,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1748,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1752,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1765,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1769,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1782,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1786,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1799,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1803,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1816,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1820,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1833,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1837,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1850,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1854,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1867,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1871,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1884,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1888,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1901,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1905,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1918,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1922,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1935,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1939,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1952,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1956,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1969,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1973,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1986,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 1990,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2003,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2007,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2020,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2024,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2037,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2041,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2054,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2058,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2071,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2075,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2088,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2092,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2105,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2109,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2122,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2126,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2139,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2143,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2156,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2160,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2173,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2177,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2190,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2194,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2207,
      "column": 9,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2211,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2222,
      "column": 9,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/ct_ed25519.jet",
      "line": 2238,
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
      "line": 10,
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
      "line": 12,
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
      "line": 9,
      "column": 5,
      "reason": "inspect fixed secret bytes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/shared_secret_bytes.jet",
      "line": 11,
      "column": 5,
      "reason": "inspect shared-secret length"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/signing_key_bytes.jet",
      "line": 9,
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
      "line": 9,
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
      "line": 11,
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
      "file": "tests/fixtures/explicit_cleanup_order.jet",
      "line": 33,
      "column": 5,
      "reason": "exercise the explicit process-stop cleanup boundary"
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
      "file": "tests/fixtures/native_global_word.jet",
      "line": 6,
      "column": 5,
      "reason": "the fixture's one-threaded caller (run, or global-word.c) is the only writer"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/native_inline_c_shared_state.jet",
      "line": 4,
      "column": 1,
      "reason": "updates this program's private counter"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/native_inline_c_shared_state.jet",
      "line": 13,
      "column": 1,
      "reason": "reads the counter defined by fixture_bump"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/native_inline_c_shared_state.jet",
      "line": 25,
      "column": 5,
      "reason": "the counter is private to these two bodies"
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
      "line": 28,
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
      "file": "tests/run/patterns_and_fields/fixed_size_list_widens_at_core_call.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed signature vector"
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
      "line": 14,
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
      "line": 9,
      "column": 5,
      "reason": "fixed password-hash vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy_type_precedence.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed password-hash vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy_valid.jet",
      "line": 9,
      "column": 5,
      "reason": "protocol-selected password policy"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy_valid.jet",
      "line": 17,
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
      "line": 11,
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
      "line": 9,
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
      "line": 8,
      "column": 5,
      "reason": "protocol-selected output length"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_hkdf_output_valid.jet",
      "line": 16,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/discarded_pure_quiet.jet",
      "line": 48,
      "column": 5,
      "reason": "the test deliberately keeps this stderr write behind its audit boundary"
    },
    {
      "package": "tests",
      "file": "tests/ui/e3106_unsafe_skip_without_per_site.jet",
      "line": 3,
      "column": 5,
      "reason": "reads a checked constant"
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
      "file": "tests/ui/ffi_inline_asm_call_needs_unsafe_region.jet",
      "line": 3,
      "column": 1,
      "reason": "the operands are scalar registers and r10 is a declared clobber"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_inline_asm_call_needs_unsafe_region.jet",
      "line": 14,
      "column": 5,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_inline_c_call_needs_unsafe_region.jet",
      "line": 1,
      "column": 1,
      "reason": "the scalar ABI contract matches the C definition"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_inline_c_call_needs_unsafe_region.jet",
      "line": 7,
      "column": 5,
      "reason": "call the audited inline C contract"
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
      "file": "tests/ui/ffi_inline_cpp_call_needs_unsafe_region.jet",
      "line": 1,
      "column": 1,
      "reason": "the scalar ABI contract matches the C++ definition"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_inline_cpp_call_needs_unsafe_region.jet",
      "line": 7,
      "column": 5,
      "reason": "call the audited inline C++ contract"
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
      "file": "tests/ui/module_write_from_fn_is_e0111.jet",
      "line": 11,
      "column": 5,
      "reason": "this program starts no task, so nothing else writes `raw`"
    },
    {
      "package": "tests",
      "file": "tests/ui/secrets_expiry/expiring_secret_loan_rejects_move_variadic_and_drop_paths_3.jet",
      "line": 13,
      "column": 9,
      "reason": "attempt to discard a loan"
    },
    {
      "package": "tests",
      "file": "tests/ui/unsafe_and_runtime/volatile_write_emit_is_byte_exact/run.jet",
      "line": 2,
      "column": 1,
      "reason": "UART TX register is mapped by the target machine"
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
    },
    {
      "package": "tests",
      "file": "tests/ui_lint/inline_asm_reads_its_core_mem_gate.jet",
      "line": 8,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "tests",
      "file": "tests/ui_lint/inline_asm_reads_its_core_mem_gate.jet",
      "line": 19,
      "column": 5,
      "reason": "call the audited register-only assembly contract"
    }
  ]
}
-->

### Counts

| crate/package | regions |
| --- | ---: |
| Compiler | 675 |
| Core | 186 |
| Docs | 8 |
| Examples | 42 |
| Jetpack | 9 |
| Tools | 7 |
| tests | 409 |
| **total** | **1336** |

### Regions

| crate/package | file | line | reason |
| --- | --- | ---: | --- |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 172:5 | "memfd_create reads the NUL-terminated name at `name`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 186:5 | "the caller owns `fd` and nothing uses it any more" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 196:9 | "the caller mapped `count` writable bytes at `address`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 361:9 | "a private read-only view of the memfd aliases nothing writable" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 447:5 | "MAP_FIXED_NOREPLACE never replaces an existing mapping" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 487:5 | "MAP_FIXED replaces only the image's own reserved pages" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 492:9 | "the bytes lie in the segment's last file page, mapped private and writable above" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 505:13 | "the caller mapped [from, to) + delta private and writable" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 521:5 | "MAP_FIXED replaces only the view's own fresh pages" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 525:5 | "the bytes lie in the view's last image page, mapped private and writable above" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 530:9 | "MAP_FIXED replaces only the view's own fresh pages; the memfd is never written again" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 775:5 | "openat reads the NUL-terminated path at `name`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 799:5 | "the timespec lies in the fresh zeroed page; nanosleep only reads it" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 810:5 | "the caller owns the stack bytes below `at`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 820:5 | "the caller owns the stack bytes below `at`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 927:5 | "brk(0) only reports the current break" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 931:5 | "the map's 104 bytes lie in the data page's auxiliary-vector room, read already" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 942:5 | "prlimit64 writes one rlimit (two words) at `scratch`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1013:5 | "the data page is this plan's own fresh mapping" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1024:5 | "the executor page is this plan's own fresh mapping" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1051:13 | "F_GETFD only reads the descriptor's flags" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1061:9 | "rt_sigaction with no new action writes the old one (32 bytes) to the scratch" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1075:5 | "a fresh anonymous reservation aliases nothing" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1085:5 | "getrandom fills the 16 stack bytes at `random`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1124:5 | "the data page holds 3840 bytes from LOADER_DATA_AUXV; /proc/self/auxv is [type, value] word pairs" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1140:9 | "the pair lies inside the bytes read into the data page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1167:5 | "the words lie in the stack mapping, above its guard page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1189:5 | "the block is this plan's own fresh mapping of plan_size bytes" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1206:5 | "the executor's records only reset process state; it then enters the loaded image on its own stack" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1218:5 | "the executor's first record forks; the child runs only the executor and enters the image, the parent returns here" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1231:5 | "wait4 writes one int status to the fresh page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1241:5 | "SIGKILL to a child this process started and has not reaped" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1252:9 | "a kernel sigaction with handler SIG_IGN on a fresh page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1270:9 | "a zeroed kernel sigaction (SIG_DFL) and an empty signal set on a fresh page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1301:5 | "execve reads the NUL-terminated strings and the two NULL-ended arrays in the block; the empty signal set at the block's start" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 126:5 | "pipe2 writes two descriptors into our scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 131:5 | "pipe2 wrote the scratch page's first word" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 137:9 | "fcntl duplicates our own descriptor" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 149:5 | "closes our own descriptor" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 196:9 | "the poll set is our scratch page's first three entries" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 203:9 | "poll reads and writes the three entries of our scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 222:9 | "poll wrote the entries' returned events" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 251:9 | "the poll set is our scratch page's first two entries" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 256:9 | "poll reads and writes the two entries of our scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 262:9 | "poll wrote the entries' returned events" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 299:9 | "the bytes lie in our scratch mapping, whose size is a multiple of eight" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 312:5 | "clock_gettime writes two words at scratch + 64, past the poll set" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 324:9 | "kills our own child" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 331:9 | "wait4 writes the status word into our scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 336:5 | "wait4 wrote the status word" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 74:1 | "a raw Linux x86-64 system call; the caller passes the call number and arguments that call expects" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 102:5 | "the caller owns `count` writable bytes at `address`" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 109:5 | "the caller owns `count` readable bytes at `address`" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 117:5 | "an anonymous mapping aliases no existing memory" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 124:5 | "the caller owns the mapping it changes" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 130:5 | "the caller owns the mapping and nothing uses it any more" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 137:5 | "exit_group ends every thread of the process" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 142:1 | "stores one word at an address the caller owns and has mapped writable" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 149:1 | "reads one word at an address the caller owns and has mapped readable" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 156:1 | "reads one byte at an address the caller owns and has mapped readable" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 163:1 | "stores one byte at an address the caller owns and has mapped writable" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 192:9 | "the caller owns the destination words" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 202:1 | "calls machine code; the caller vouches for the code and its System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 215:1 | "calls machine code; the caller vouches for the code and its two-word System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 230:1 | "calls machine code; the caller holds the mapping and its checked zero-argument Unit System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 240:1 | "calls machine code; the caller holds the mapping and its checked three-word Unit System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 256:1 | "calls machine code; the caller holds the mapping and its checked one-word Unit System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 268:1 | "calls machine code; the caller holds the mapping and its checked two-word Unit System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 285:1 | "a raw Linux x86-64 syscall; the caller supplies the syscall's valid machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 302:1 | "reads one aligned machine word in live allocator-owned mapped memory" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 309:1 | "stores one aligned machine word in live allocator-owned mapped memory; x86 TSO makes this a release store" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 318:1 | "atomically compares and exchanges one aligned live machine word; every non-atomic access to protected data holds its lock" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 333:1 | "x86 pause only hints that this thread is waiting in a spin loop" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 343:1 | "cpuid only reports processor identification; rbx is saved and restored" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 356:1 | "cpuid only reports processor identification; rbx is saved and restored" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 370:1 | "cpuid only reports processor identification; rbx is saved and restored" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 383:1 | "cpuid only reports processor identification; rbx is saved and restored" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 398:1 | "xgetbv only reads an extended control register; the caller has seen CPUID.1:ECX.OSXSAVE" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 412:1 | "reads the stack pointer register after one balanced push; no memory is read" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 427:1 | "the caller owns count readable bytes at source and count writable bytes at target; an overlap has target below source" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 446:1 | "the caller owns count readable bytes ending at source_last and count writable bytes ending at target_last; an overlap has target above source" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 462:1 | "the caller owns count readable words whose last starts at source_last and count writable words whose last starts at target_last; an overlap has target above source" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 480:1 | "the caller owns count writable 8-byte words at target" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 495:1 | "reads one byte from a live readable mapped address supplied by the caller" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 502:1 | "writes one byte to a live writable mapped address supplied by the caller" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 512:1 | "writes at most eight bytes from a stack-local word while that word is live; the balanced pushes and pops do not expose stack storage after return" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 533:1 | "bytes lends length readable bytes; the second iovec lends a stack-local LF; writev retains neither pointer and every push is balanced" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 570:1 | "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 617:1 | "the calling thread is attached and GS points at its live Jet TCB" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 626:1 | "ARCH_GET_GS writes one initialized aligned stack word only during this syscall; no TLS or heap is read, and the push/pop is balanced" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 641:1 | "atomically adds to one aligned live word and returns its previous value" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 654:1 | "atomically exchanges one aligned live word; xchg has an implicit full barrier" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 667:1 | "calls executable System V code with one raw word argument" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 680:1 | "calls a Jet closure's code with the live closure header and one raw word argument" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 701:1 | "raw clone child runs only Jet/syscall code, never Rust or musl; supplied stack contains entry and argument" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 742:1 | "calls checked System V Unit code; the caller lends one live raw machine-word argument" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 754:1 | "calls checked System V Unit code; the caller vouches for both raw machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 768:1 | "calls checked System V Unit code; the caller vouches for all three raw machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 784:1 | "calls a checked native runtime callback with one machine-word argument" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 797:1 | "calls a checked native runtime callback with two machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 812:1 | "calls a checked native runtime callback with three machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 829:1 | "calls a checked native runtime callback with four machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 848:1 | "calls a checked native runtime callback with five machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 871:1 | "source and directory are live owned descriptors, name is terminated; linkat borrows a stack-local empty path and retains no pointer" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 892:1 | "ioctl reads one stack-local machine word with the supplied audited request and live fd; it retains no pointer and all pushes are balanced" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 915:1 | "object is a live fd for fstat or a terminated path for stat/lstat; output lends 128 writable bytes; the initialized stack stat frame never escapes" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1004:1 | "the audited caller lends the stat scalar row and valid syscall object" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1006:5 | "packed Linux 32-bit fields become independent portable machine words without any allocation" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1017:1 | "calls a checked System V void(void) process entry that remains executable for the entire call" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1027:1 | "directory and path identify a source-approved statx object; output lends three writable words; the zeroed 256-byte statx stack buffer never escapes" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1125:5 | "address names an initialized canonical native word owned by the pinned request or retained slot" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1132:5 | "address names an initialized canonical native byte owned by the pinned request or retained slot" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1139:5 | "width and address range are checked; the caller retains the initialized slot's physical permit" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1175:5 | "the caller owns exactly size writable bytes under the native payload permit; width and value were checked above" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1193:9 | "both addresses have the checked canonical alloc(size,align) / free(pointer,size,align) ABI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1209:5 | "the allocation receipt pairs this owner with its original free address and exact size/alignment" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1217:5 | "the admitted runtime signature or pinned clone/drop role was checked before invoking its fixed word ABI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1225:1 | "calls machine code; the caller vouches for its three-word System V signature" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 383:5 | "unlinkat borrows a held parent and CString" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 387:5 | "adopt only a successful openat descriptor" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 394:5 | "adopt only a successful openat descriptor" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 436:5 | "mkdirat borrows the retained directory and CString" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 451:5 | "exclusive openat descriptor transfers ownership exactly once" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 470:5 | "exclusive temporary and renameat borrow retained parent and names" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 489:5 | "renameat borrows both retained directory authorities" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 542:5 | "Win32 fills borrowed UTF16 buffer; handle remains owned" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 552:5 | "Win32 writes exactly the C-layout attribute tag record" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 601:5 | "aligned variable-sized FILE_RENAME_INFO uses selected C layout and live UTF16 loans" |
| Compiler | Compiler/JetDevserver/Source/WebHost02.jet | 614:5 | "Win32 consumes borrowed C-layout disposition record" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 145:5 | "pointer is a fresh payload whose align &gt;= 8 block reserved the word 24 bytes before it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 153:5 | "pointer is an align &gt;= 8 payload being freed with its allocated size; its block reserved the count word 24 bytes before it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 176:5 | "anonymous private mmap owns fresh writable bytes and cannot alias a live heap block; a failed syscall produces no allocation" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 190:5 | "the caller has removed this mapping from all heap lists and no payload remains live" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 203:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 206:5 | "the fresh state page is private to initialization, which precedes every allocation" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 222:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 253:5 | "initialization wrote the arena table word once, before any allocation" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 265:5 | "reads the stack pointer register only; nothing is dereferenced" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 279:9 | "the live totals are process-lifetime arena words; the quiescent caller needs no lock to read them" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 306:5 | "root is an aligned lock word in the process-lifetime state mapping; CAS/xchg acquire all metadata it protects and the futex wait only reads that word" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 321:5 | "the calling thread holds this lock; xchg publishes its completed metadata updates and reports sleepers, and the futex wake only names the word" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 330:5 | "slab and its linked neighbours are live heap-owned headers protected by the held heap lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 348:5 | "head and slab are live heap-owned metadata protected by the held heap lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 382:5 | "the slab-region lock owns the cursor, end and free-slab stack words and every stacked slab's link word" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 420:5 | "the slab-region lock owns the free-slab stack; the slab is unlinked from every class list and holds no live or cached block" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 436:5 | "madvise only changes the page size of the fresh private region" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 448:5 | "the slab is this arena's alone: fresh from its region or off the free-slab stack; the held arena lock excludes all list readers" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 474:5 | "the held arena lock protects the class list, the slab's free links, carve word and live count, the retained count and the arena live total; row is the caller's exclusive cache row; the blocks leave the slab before any caller sees them" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 525:5 | "the held arena lock protects the slab's free links, carve word, live count, class list, retained count and live total; the block is no longer any payload or cache entry" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 554:5 | "the held arena lock makes the arena's scratch row exclusive; it is emptied again before the lock is released" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 563:5 | "block is this caller's exclusive block; its two cached words are read before the payload header overwrites them" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 589:5 | "an idempotent flag: every writer stores the same 1" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 603:5 | "row is this thread's private cache row and block its cached head; both cached words are read before the payload header overwrites them" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 618:5 | "row is this thread's private cache row; each cached block's slab is live while the block is cached, so its arena word is readable, and the block returns under that arena's lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 649:9 | "row is one of this thread's eight private cache rows" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 688:5 | "the large-cache lock owns every class list, its link words and the cached byte count" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 708:9 | "the large-cache lock owns every class list and the cached byte count; the mapping's first word is no live payload once it is freed" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 739:5 | "pointer's two header words and the aligned payload are wholly within this page-rounded class mapping" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 758:5 | "pointer is a live direct allocation of exactly `size` bytes at align 1 whose two header words name its mapping and the mapping's bytes; the mode and tracked-count words are aligned words read atomically on x86, as alloc_reserve and alloc_untrack read them; a refused mremap leaves the mapping as it was" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 794:9 | "row is this thread's private cache row" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 813:5 | "the caller holds the policy lock and cell is a counter in alloc_root" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 821:5 | "the aligned mode word is read atomically on x86; system allocation needs no policy lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 827:5 | "the policy lock exclusively owns the mode, cap and accounting words" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 856:5 | "the policy lock exclusively owns live bytes; restoration does not erase outstanding tracked blocks" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 870:5 | "the held policy lock serializes all native allocator configuration and reservations" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 887:5 | "the policy lock serializes restoration with native reserve and free; tracked blocks remain in the side table" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 901:5 | "the policy lock protects this bounded fact word in alloc_root" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 916:5 | "the private raw-heap node is fully initialized before linking under the policy/tracker lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 929:5 | "the process-lifetime tracker count is an aligned x86 word; a zero count avoids any lookup or lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 936:5 | "the policy/tracker lock exclusively owns all side-table links and count" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1010:13 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row (TCB @320 + 16 per class, alloc_cache_row) and block its cached head, whose two cached words are read before the payload header overwrites them; an align &lt;= 8 payload at block + header is aligned because blocks start 32-aligned" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1054:13 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count is an aligned word read atomically, as alloc_untrack reads it; row is this thread's private cache row; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1100:17 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row of the size's class and block its cached head, whose next word is read before the count overwrites it; the block's second word is its slab word, the payload header's; the payload at block + 24 is 8-aligned because blocks start 32-aligned" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1133:21 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count is an aligned word read atomically, as alloc_untrack reads it; row is this thread's private cache row of the size's class; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's next word overwrites it; the block's second word stays its slab word" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1174:13 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row of the size's class and block its cached head, whose two cached words are read before the payload header overwrites them; an align-1 payload at block + 16 needs no alignment" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1202:13 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count is an aligned word read atomically, as alloc_untrack reads it; row is this thread's private cache row of the size's class; the caller frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1240:9 | "row is this thread's private cache row; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1256:5 | "the lowering returns this still-live direct allocation with exactly its original size and alignment" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1268:5 | "the caller's live payload header names its slab, whose arena word is fixed when the slab is created" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1273:5 | "the lowering frees exactly once with the allocation's original layout; its slab remains live until this last payload is returned under its arena's lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1328:5 | "the fresh state page and stub page are private to initialization, which precedes every allocation; the stub page becomes read+execute before any call" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1335:5 | "checked-heap start runs during runtime-init, before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1349:5 | "the checked-mode stub only copies rbp to rax and returns; a missing stub leaves no chain" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1357:13 | "frame is an active caller's [saved rbp][return address] pair on this thread's stack: every followed link ascends by less than 16 MiB" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1363:9 | "out lends count writable words: a checked block's chain words or a trap's scratch page" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1377:5 | "block is this caller's fresh heap block of prefix + size + 8 bytes; every word written lies in block .. pointer + size + 8" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1401:5 | "a checked payload's canary word precedes it; the quarantine lock orders racing frees of one block" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1410:5 | "the live canary proves pointer is a checked payload whose header and tail this free now owns" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1423:5 | "the quarantine lock owns the ring and its counters; every parked block is freed and owned by the quarantine" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1458:5 | "aligned whole-word stores; each checked take or give loads its observer word once" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1467:5 | "the freeing caller owns size payload bytes at pointer" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1482:5 | "pointer is a parked checked payload owned by the quarantine; its header, payload and tail stay mapped until retired" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1505:5 | "the freed canary proved the checked header" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1514:5 | "pointer is an inspected parked checked payload; its header still names the enclosing block and layout" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1541:5 | "the quarantine lock owns the ring and its counters" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1557:5 | "data lends length readable bytes; write retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1593:5 | "path is a fresh private page; the image is mapped read-only and only read" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1617:5 | "image is a read-only private mapping of this executable's ELF64 file; its program header table lies inside it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1646:9 | "image is a read-only private mapping of this executable's ELF64 file; every offset read comes from its own section and symbol tables" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1689:9 | "words lends count readable chain words" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1733:9 | "the live canary proved the checked header" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1779:5 | "remaining is in 1..=8, so write borrows only the initialized stack word; a short write advances within it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1836:5 | "the stack-word syscalls unblock SIGABRT and, if necessary, restore its default disposition before raising it again; no heap pointer is needed" |
| Compiler | Compiler/JetRuntime/Source/Atomic.jet | 14:5 | "place is caller-owned aligned8 private unpublished storage for one atomic word" |
| Compiler | Compiler/JetRuntime/Source/Atomic.jet | 19:5 | "place borrows one aligned8 live atomic word; x86 aligned MOV is the SeqCst load mapping" |
| Compiler | Compiler/JetRuntime/Source/Atomic.jet | 24:5 | "place borrows one aligned8 live atomic word; XCHG is the full-barrier SeqCst store mapping" |
| Compiler | Compiler/JetRuntime/Source/Atomic.jet | 29:5 | "place borrows one aligned8 live atomic word; LOCK CMPXCHG orders success and failure SeqCst" |
| Compiler | Compiler/JetRuntime/Source/Atomic.jet | 34:5 | "place borrows one aligned8 live atomic word; LOCK XADD returns the previous wrapping64 value SeqCst" |
| Compiler | Compiler/JetRuntime/Source/CAbiSession.jet | 20:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CAbiSession.jet | 28:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/ClosureRefs.jet | 9:5 | "the caller's owned reference keeps the aligned closure header live; locked compare-exchange atomically publishes one more owner" |
| Compiler | Compiler/JetRuntime/Source/ClosureRefs.jet | 20:5 | "consumes one reference to an aligned live closure header; locked compare-exchange provides release ordering and last-owner acquire before its release helper" |
| Compiler | Compiler/JetRuntime/Source/CodecCSV.jet | 264:5 | "checked native Decode callback; the tree, value slot and out words stay live for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/Collections.jet | 9:5 | "data lends index's initialized packed native U8 element" |
| Compiler | Compiler/JetRuntime/Source/Collections.jet | 14:5 | "data owns index's writable packed native U8 element" |
| Compiler | Compiler/JetRuntime/Source/Collections.jet | 87:5 | "native lowering owns both live ranges of count bytes; the leaves receive the copy order that overlap requires" |
| Compiler | Compiler/JetRuntime/Source/Collections.jet | 122:5 | "checked native value-returning callback; its raw argument stays valid for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/Collections.jet | 128:5 | "checked native value-returning callback; both raw arguments stay valid for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/Collections.jet | 134:5 | "checked unary closure adapter with borrowed environment, input and output slots" |
| Compiler | Compiler/JetRuntime/Source/Collections.jet | 140:5 | "checked binary closure adapter with live environment, inputs and output slots" |
| Compiler | Compiler/JetRuntime/Source/Collections.jet | 146:5 | "checked native Unit callback; its raw argument stays valid for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/Collections.jet | 152:5 | "checked Unit clone callback; source remains live and target lends writable output storage" |
| Compiler | Compiler/JetRuntime/Source/CompilerFS.jet | 169:9 | "the prefix remains terminated and scratch lends 128 writable portable stat-fact bytes" |
| Compiler | Compiler/JetRuntime/Source/CompilerFS.jet | 190:5 | "name is NUL-terminated and scratch lends 128 writable portable stat-fact bytes" |
| Compiler | Compiler/JetRuntime/Source/CompilerHost.jet | 9:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CompilerHost.jet | 128:5 | "startup lends the start stack: the null-terminated envp array, then the auxiliary vector's [type, value] pairs up to AT_NULL" |
| Compiler | Compiler/JetRuntime/Source/CompilerHost.jet | 149:5 | "page is a fresh private page: the path, then the [type, value] pairs read into it" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 12:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 115:13 | "the owned readable file lends its descriptor; mmap creates exactly length read-only private bytes; the map owner alone unmaps them" |
| Compiler | Compiler/JetRuntime/Source/Comptime.jet | 14:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/ComptimeServe.jet | 113:13 | "the compiler calls an entry of this image's table, or a runtime export it resolved, on the word arguments that entry's System V signature takes" |
| Compiler | Compiler/JetRuntime/Source/ComptimeServe.jet | 161:5 | "the serve entry runs once, on the image's only thread, before it serves" |
| Compiler | Compiler/JetRuntime/Source/ComptimeServe.jet | 207:5 | "the serve entry installed this runtime export, whose System V signature takes these five words" |
| Compiler | Compiler/JetRuntime/Source/CountMinSketch.jet | 8:5 | "caller holds this native CMS mutex and owns the initialized four-byte counter" |
| Compiler | Compiler/JetRuntime/Source/CryptoEntropy.jet | 20:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CryptoEntropy.jet | 51:13 | "the test/fault harness owns the synchronous callback and context until reset; the callback borrows only this writable suffix" |
| Compiler | Compiler/JetRuntime/Source/CryptoEntropy.jet | 56:5 | "Linux getrandom flags0 writes only the exclusively borrowed writable span and retains no address; kernel negative errno is preserved" |
| Compiler | Compiler/JetRuntime/Source/DataEvent.jet | 5:5 | "address borrows an initialized word in a live native DataEvent or List" |
| Compiler | Compiler/JetRuntime/Source/DataEvent.jet | 8:5 | "address owns a writable word in a native DataEvent" |
| Compiler | Compiler/JetRuntime/Source/DataLimits.jet | 5:5 | "address borrows an initialized word in a live native limits carrier" |
| Compiler | Compiler/JetRuntime/Source/DataLimits.jet | 8:5 | "address owns a writable word in a native limits carrier or getter out slot" |
| Compiler | Compiler/JetRuntime/Source/Deadline.jet | 74:5 | "clock_gettime writes two thread-owned TCB words, not retained after the syscall" |
| Compiler | Compiler/JetRuntime/Source/Display.jet | 8:5 | "borrows an initialized aligned word in an E4 header, exact-Int node or owned formatter workspace" |
| Compiler | Compiler/JetRuntime/Source/Display.jet | 14:5 | "writes an aligned word in an exclusively owned formatter workspace or fresh E4 header" |
| Compiler | Compiler/JetRuntime/Source/Display.jet | 20:5 | "reads one initialized byte in an owned formatter digit buffer or borrowed exact-Int limb" |
| Compiler | Compiler/JetRuntime/Source/Display.jet | 26:5 | "writes one byte in an exclusively owned formatter buffer; value is in 0..255" |
| Compiler | Compiler/JetRuntime/Source/Dyn.jet | 7:5 | "address borrows an initialized word in a live native Dyn or List" |
| Compiler | Compiler/JetRuntime/Source/Dyn.jet | 10:5 | "address owns a writable word in a native Dyn or List" |
| Compiler | Compiler/JetRuntime/Source/DynCodec.jet | 108:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/DynCodec.jet | 135:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/EncodingErrors.jet | 5:5 | "borrows an initialized aligned word in a live encoding carrier or option input" |
| Compiler | Compiler/JetRuntime/Source/EncodingErrors.jet | 8:5 | "owns a writable aligned word in a fresh encoding carrier or caller output" |
| Compiler | Compiler/JetRuntime/Source/EnvOverlay.jet | 44:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/EnvRoutes.jet | 18:5 | "removed lends exactly one initialized writable C bool byte; no adjacent padding belongs to this output" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 52:5 | "the image's entry calls this before it starts any other thread" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 62:9 | "jet_rt_main calls this on the main thread before the entry runs" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 78:5 | "rt_sigaction borrows the four-word action on the stack; handler is this runtime's C-ABI export with the (signal, info, context) signature" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 89:5 | "stack is a fresh private mapping; sigaltstack copies its first three words (ss_sp, ss_flags, ss_size)" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 107:5 | "stack is this thread's registered alternate stack, unused outside a handler; sigaltstack copies its first three words" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 139:5 | "rt_sigaction borrows the four-word SIG_DFL action on the stack; tgkill signals this thread" |
| Compiler | Compiler/JetRuntime/Source/FaultAllocation.jet | 21:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/FaultSchedule.jet | 32:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/FilesPathOps.jet | 35:5 | "the terminated raw path and the 128-byte stat fact row are live for the syscall" |
| Compiler | Compiler/JetRuntime/Source/FilesStreams.jet | 24:9 | "the freshly opened owned descriptor and the 128-byte stat fact row are live for fstat" |
| Compiler | Compiler/JetRuntime/Source/FilesWalk.jet | 33:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/GeometryGlue.jet | 4:5 | "borrows an initialized aligned word inside a live native geometry carrier" |
| Compiler | Compiler/JetRuntime/Source/GeometryGlue.jet | 7:5 | "owns a writable aligned word in a fresh native geometry carrier" |
| Compiler | Compiler/JetRuntime/Source/HTTPBody.jet | 37:5 | "borrows an initialized word in the live body, chunks, closure, List or E4 allocation" |
| Compiler | Compiler/JetRuntime/Source/HTTPBody.jet | 43:5 | "owns the word, or holds the body's transition lock for this state field" |
| Compiler | Compiler/JetRuntime/Source/HTTPBody.jet | 49:5 | "atomic refcount, take/close flag or transition lock in a live body allocation" |
| Compiler | Compiler/JetRuntime/Source/HTTPBody.jet | 100:5 | "the last closure reference calls its generated release(box), which drops captures and frees the box" |
| Compiler | Compiler/JetRuntime/Source/HTTPBody.jet | 205:5 | "the claimed closer code has the audited code(box) ABI, with no transition lock held" |
| Compiler | Compiler/JetRuntime/Source/HTTPBody.jet | 318:17 | "reader code(box, descriptor) borrows exactly count writable U8 bytes; no body lock is held" |
| Compiler | Compiler/JetRuntime/Source/HTTPBody.jet | 371:17 | "the source holds count initialized UTF8 or raw reader bytes" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 14:5 | "runtime startup owns a fresh anonymous mapping before any program thread exists" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 28:5 | "rt_sigaction synchronously borrows the four-word SIG_IGN action; no signal handler or restorer runs for ignored SIGPIPE" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 45:5 | "the caller lends length readable bytes for the complete synchronous write" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 60:5 | "the output lock exclusively owns the initialized buffer and length" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 95:5 | "the caller lends length bytes; the output lock owns the destination buffer" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 158:5 | "bytes lends length readable bytes; writev borrows its own LF until return" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 175:5 | "the held host lock lends enough scratch for Linux TCGETS" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 218:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 226:5 | "value is a borrowed E4 JetCText header; stdout copies its bytes before returning" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 238:5 | "value is a borrowed E4 JetCText header; writev borrows its bytes and its own LF until return" |
| Compiler | Compiler/JetRuntime/Source/HostDirectoryList.jet | 9:9 | "getdents supplies a live terminated name; openat describes the entry without following links" |
| Compiler | Compiler/JetRuntime/Source/HostDirectoryList.jet | 15:13 | "the owned descriptor and full Linux stat buffer live through fstat and close" |
| Compiler | Compiler/JetRuntime/Source/HostDirectoryList.jet | 46:9 | "an owned fresh directory descriptor and writable getdents64 buffer" |
| Compiler | Compiler/JetRuntime/Source/HostDirectoryList.jet | 79:5 | "close the fresh stream descriptor once after reading" |
| Compiler | Compiler/JetRuntime/Source/HostFileLock.jet | 83:5 | "the borrowed lock descriptor and 128 writable stat-fact bytes live through fstat" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 8:5 | "the caller borrows a live FD72 or runtime scratch word" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 14:5 | "the caller owns the FD72 lock or exclusive runtime scratch word" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 54:5 | "the fresh path allocation includes its terminator byte" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 285:9 | "the caller lends 128 writable bytes for portable stat facts" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 374:5 | "the terminated borrowed path and 128-byte stat fact output are live for the syscall" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 425:5 | "the caller lends one writable Bool byte" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 441:9 | "the held directory and terminated source-approved single name stay live through openat" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 464:9 | "the held directory and terminated final name live through O_PATH&#124;O_NOFOLLOW&#124;O_CLOEXEC openat; no read access or link traversal" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 472:9 | "openat returned an exclusively owned descriptor and the caller lends StatFacts128; fstat copies initialized scalar facts" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 506:5 | "both duplicated descriptors and the terminated name live through the held-inode linkat syscall" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 528:9 | "the descriptor is held live, output begins with the borrowed empty pathname, statx writes only its private stack frame" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 539:5 | "the terminated source-approved path and three writable birth-time output words stay live through statx" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 593:5 | "both terminated borrowed names and the caller's optional compact byte output remain live only through lgetxattr" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 623:9 | "the held directory, terminated plain name and writable byte range are borrowed only through readlinkat" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 649:5 | "held directory duplicates and terminated names prevent any descriptor-close/reuse race during renameat2" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 669:9 | "the held fd and stack-local source-validated winsize word live only through ioctl" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 15:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 179:13 | "the owned walk descriptor and terminated part live through openat" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 238:13 | "the walk directory and terminated final part live through openat" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 433:5 | "the terminated borrowed path and the 256-byte statx buffer are live for the syscall" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 539:13 | "the walk directory and terminated final part live through O_PATH&#124;O_NOFOLLOW&#124;O_CLOEXEC openat" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 547:17 | "the owned O_PATH descriptor and the caller's 128-byte stat facts live through fstat" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 554:21 | "the owned O_PATH descriptor, birth's zeroed empty pathname and three writable words live through statx" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 622:5 | "both walk directories and terminated final parts live through renameat/renameat2" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 636:9 | "both walk directories and terminated final parts live through linkat" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 21:5 | "value transfers one 56-byte error header and its nullable owned E4 text children" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 32:5 | "value borrows the complete error header; result exclusively owns 56 fresh bytes" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 68:5 | "value exclusively owns a new initialized IOError with two owned text children; error lends the caller's pointer slot" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 85:5 | "value lends five initialized context words; result exclusively owns its header and cloned nullable E4 children" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 101:5 | "value transfers its 40-byte header and two independently owned nullable text children" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 117:9 | "the optional code box borrows one initialized exact-Int word" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 123:5 | "nullable option boxes lend one initialized payload word; value owns cloned texts and the complete new context" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 134:5 | "value borrows an initialized context header" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 138:5 | "value borrows a context; some lends the writable pointer output for a present field" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 150:5 | "value borrows a context; some lends the writable exact-Int word output for a present field" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 158:5 | "value borrows an initialized error header" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 162:5 | "value borrows a complete error header; the selected payload must match its canonical tag" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 173:5 | "context transfers its initialized five words and text owners into the fresh error; only the context header is then freed" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 187:5 | "value borrows an initialized ResourceLimit error" |
| Compiler | Compiler/JetRuntime/Source/HostIO.jet | 202:5 | "value exclusively owns seven initialized words; ResourceLimit has no text children" |
| Compiler | Compiler/JetRuntime/Source/HostNet.jet | 8:5 | "value exclusively owns four initialized words; inputs lend exact Int owners and a valid E4 message" |
| Compiler | Compiler/JetRuntime/Source/HostNet.jet | 18:5 | "value lends the complete immutable carrier and all three owned children" |
| Compiler | Compiler/JetRuntime/Source/HostNet.jet | 25:5 | "value transfers its 32-byte header, two independently retained Int words and owned E4 message" |
| Compiler | Compiler/JetRuntime/Source/HostNet.jet | 34:5 | "value lends the immutable carrier; return retains its exact Int field" |
| Compiler | Compiler/JetRuntime/Source/HostNet.jet | 38:5 | "value lends the initialized presence word" |
| Compiler | Compiler/JetRuntime/Source/HostNet.jet | 42:5 | "value lends the immutable carrier; return retains its exact Int field" |
| Compiler | Compiler/JetRuntime/Source/HostNet.jet | 46:5 | "value lends the immutable carrier; return deep-clones its owned E4 message" |
| Compiler | Compiler/JetRuntime/Source/HostNetAddress.jet | 5:5 | "value lends a complete initialized network header" |
| Compiler | Compiler/JetRuntime/Source/HostNetAddress.jet | 8:5 | "value exclusively owns the network header or its mutex" |
| Compiler | Compiler/JetRuntime/Source/HostNetAddress.jet | 38:5 | "value owns sixteen octets; bytes lends length initialized U8 elements" |
| Compiler | Compiler/JetRuntime/Source/HostNetAddress.jet | 50:5 | "value lends IP24; data owns length initialized native U8 elements; output receives ownership" |
| Compiler | Compiler/JetRuntime/Source/HostNetAddress.jet | 83:5 | "bytes lends width initialized little-endian bytes (width&lt;=8)" |
| Compiler | Compiler/JetRuntime/Source/HostNetAddress.jet | 93:5 | "bytes owns width writable little-endian bytes (width&lt;=8)" |
| Compiler | Compiler/JetRuntime/Source/HostNetPoller.jet | 24:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/HostNetPoller.jet | 62:5 | "the root lock owns a packed 12B epoll_event; state retains the native fd through this synchronous call" |
| Compiler | Compiler/JetRuntime/Source/HostNetPoller.jet | 88:9 | "events owns 64 packed epoll_event entries; epoll_wait retains no buffer pointer" |
| Compiler | Compiler/JetRuntime/Source/HostNetResolver.jet | 106:5 | "host lends length E4 bytes; outputs lend their writable pointer/count words" |
| Compiler | Compiler/JetRuntime/Source/HostNetResolver.jet | 127:5 | "name owns a trailing NUL byte" |
| Compiler | Compiler/JetRuntime/Source/HostParallelism.jet | 451:5 | "sched_getaffinity writes at most the owned 128-byte mask; pid0 names this thread" |
| Compiler | Compiler/JetRuntime/Source/HostParallelism.jet | 526:9 | "the probe answer is fixed for the process; a racing probe stores the same word" |
| Compiler | Compiler/JetRuntime/Source/HostParallelism.jet | 534:5 | "cpuid and xgetbv only report processor identification and the OS-enabled state" |
| Compiler | Compiler/JetRuntime/Source/HostPipe.jet | 163:13 | "the FD72 lock protects this initialized refill-buffer byte" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 7:5 | "the audited caller supplies the Linux syscall's valid word arguments" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 102:5 | "the caller lends the C-ABI tuple's Bool byte and two writable Int words" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 182:5 | "buffer owns the sixteen-byte struct rlimit64 the kernel writes; the caller lends the tuple's Bool byte and three Int words" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 202:5 | "buffer owns the sixteen-byte struct rlimit64 the kernel reads and does not retain" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 218:5 | "buffer owns the 144-byte struct rusage the kernel writes; the caller lends the tuple's Bool byte and four Int words" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 232:5 | "some lends a writable C-ABI Option success word" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 242:5 | "status lends an eight-byte out slot; wait4 writes its low four bytes and retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 254:5 | "read_fd lends eight writable bytes to pipe's two u32 results; split both facts into the caller's separate word slots" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 13:9 | "the FD72 lock prevents native descriptor close/reuse throughout this nonblocking syscall" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 55:13 | "accept4 retains no address pointers and atomically sets CLOEXEC/NONBLOCK on the accepted socket" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 62:13 | "private scratch lends sockaddr bytes and its exact 4B socklen; accept4 retains neither pointer" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 97:9 | "private scratch lends one native int and its 4B socklen; getsockopt retains neither pointer" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 112:9 | "private scratch lends exactly one native int; setsockopt retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 136:5 | "data lends length native U8 elements; output owns length compact bytes" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 150:9 | "the held FD72 lock lends its live nonblocking socket; packed lends exactly length bytes; no pointer survives recvfrom/sendto" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 155:13 | "recvfrom initialized result compact bytes; data owns the corresponding mutable native U8 slots" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 186:9 | "the FD72 lock retains the socket; private scratch lends sockaddr bytes; packed lends length bytes; sendto retains no pointers" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 203:9 | "private scratch lends sockaddr/socklen and packed lends length mutable bytes; MSG_TRUNC returns the complete datagram length while writing at most length bytes" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 212:17 | "recvfrom initialized used compact bytes; the caller owns used mutable native U8 elements" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 236:5 | "path lends its immutable E4 bytes for std-compatible sockaddr_un validation" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 225:17 | "Linux TCGETS initialized the 36-byte termios buffer; cfmakeraw sets VMIN=1, VTIME=0 in its control-character array" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 304:13 | "rt_sigaction borrows four initialized stack words; no allocator, mutex or TLS is touched after fork" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 393:13 | "rt_sigaction writes the four-word old action into this spawn's block; no allocator, mutex or TLS is touched in the child" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 403:5 | "rt_sigprocmask reads the parent's saved mask word in this spawn's block" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 447:5 | "mprotect makes the lowest page of this spawn's fresh mapping the guard; the masks are words of this spawn's block; the child runs only jet_rt_process_spawn_child on the mapped stack and leaves through execve or exit_group before the parent resumes" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 800:9 | "ioctl borrows one stack word of source-validated winsize dimensions; no pointer escapes" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 815:5 | "source lends a NUL-terminated raw path including non-Unicode bytes and a writable 128-byte stat fact row" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 826:5 | "the source-approved native NUL path and Jet-owned PATH_MAX buffer are live across the mechanical realpath call" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 831:9 | "libc __errno_location returns this calling thread's live errno int; read only its initialized four bytes" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 842:5 | "successful realpath NUL-terminates its result within the PATH_MAX byte buffer" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 34:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 48:5 | "bytes borrows one live NUL-terminated startup string" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 80:5 | "host argument registration is single-threaded and precedes user code" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 90:5 | "host argument registration is single-threaded and precedes user code" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 104:5 | "startup lends the immutable, null-terminated envp pointer array and its NUL-terminated entries" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 118:5 | "pending exclusively owns raw_count + 2 words; registration is single-threaded and precedes user code" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 136:5 | "rows exclusively owns raw_count index records; startup lends every pointed-to NUL string" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 157:5 | "storage exclusively owns total_bytes; each source row lends its NUL string for this capture call, and published rows point only into the owned block" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 169:5 | "the index is published once: by single-threaded registration, or inside host_environment_require's once before it completes" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 199:5 | "bytes lends one live NUL-terminated startup string" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 213:5 | "startup lends length bytes; data exclusively owns length initialized packed bytes" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 226:5 | "output lends a 24-byte native [U8] header and receives ownership of data" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 240:5 | "row borrows a complete immutable startup index record" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 255:5 | "row borrows its immutable record; data_out lends one writable pointer word" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 264:5 | "row borrows its immutable record; data_out lends one writable pointer word" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 280:5 | "startup lends argc initialized argv pointers and their immutable NUL strings" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 294:5 | "argv[1] is an immutable startup string of `length` bytes; data lends the job name's static bytes" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 339:5 | "startup lends argc initialized argv pointers and their immutable NUL strings" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 357:5 | "the startup index and key bytes are immutable borrowed data" |
| Compiler | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 21:5 | "value borrows a complete immutable startup NUL string; output lends one writable U64 word" |
| Compiler | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 48:5 | "value borrows length immutable captured startup bytes; no pointer escapes" |
| Compiler | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 82:5 | "the held Host lock lends 16 writable bytes of its existing ioctl scratch; the canonical lookup retains no key pointer" |
| Compiler | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 92:5 | "output exclusively borrows the paired initialized empty Source out-record; no owner is overwritten" |
| Compiler | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 115:5 | "Host startup already initialized its process-lifetime mapping; the held lock owns ioctl scratch32..47 and FIRST raw lookup only borrows the immutable captured index" |
| Compiler | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 147:13 | "the paired Source owner receives the sole native U8 buffer with exact LEN/CAP/DATA; no raw pointer is cast into a List value" |
| Compiler | Compiler/JetRuntime/Source/HostStdio.jet | 56:5 | "text lends an E4 header and its bytes for this synchronous write; writev borrows its own stack LF" |
| Compiler | Compiler/JetRuntime/Source/HostStdio.jet | 82:5 | "the caller lends the tuple's exact-Int word and Bool byte outputs" |
| Compiler | Compiler/JetRuntime/Source/HostStdio.jet | 92:5 | "some lends the route's zeroed Text out slot" |
| Compiler | Compiler/JetRuntime/Source/HostTime.jet | 14:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/HostTime.jet | 29:5 | "clock_gettime synchronously writes the calling thread's two scratch words" |
| Compiler | Compiler/JetRuntime/Source/HostTime.jet | 58:5 | "before_out lends the tuple's one writable Bool byte" |
| Compiler | Compiler/JetRuntime/Source/HostTime.jet | 151:5 | "path lends length bytes; std file open rejects embedded NUL" |
| Compiler | Compiler/JetRuntime/Source/HostTime.jet | 159:5 | "name owns its trailing NUL byte" |
| Compiler | Compiler/JetRuntime/Source/HostTime.jet | 195:13 | "bytes lends used initialized bytes; data owns used native U8 elements" |
| Compiler | Compiler/JetRuntime/Source/Int.jet | 10:5 | "borrow an initialized word in a live exact-Int carrier" |
| Compiler | Compiler/JetRuntime/Source/Int.jet | 16:5 | "write an initialized word in a uniquely owned exact-Int workspace" |
| Compiler | Compiler/JetRuntime/Source/Int.jet | 22:5 | "borrow four initialized bytes in a live limb buffer; no adjacent limb is read" |
| Compiler | Compiler/JetRuntime/Source/Int.jet | 28:5 | "write four bytes in a uniquely owned limb buffer; value is an unsigned 32-bit limb" |
| Compiler | Compiler/JetRuntime/Source/Int.jet | 386:9 | "the caller holds an existing owner; atomic retain cannot race final-owner retirement" |
| Compiler | Compiler/JetRuntime/Source/Int.jet | 407:9 | "the caller consumes one owner; atomic decrement publishes refs0 before shared-domain retirement" |
| Compiler | Compiler/JetRuntime/Source/Int.jet | 412:13 | "the final owner has reached zero; the remaining shared hazard domain decides immediate free or retirement" |
| Compiler | Compiler/JetRuntime/Source/IntHazard.jet | 13:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/IntHazard.jet | 92:5 | "the native numeric forwarder lends a pure reload callback and its live closure context for this synchronous call" |
| Compiler | Compiler/JetRuntime/Source/JourneyTLS.jet | 22:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/LogicalEnvironment.jet | 79:5 | "host argument registration is single-threaded and precedes user code" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 23:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 46:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 62:5 | "an idempotent flag: every writer stores the same 1" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 77:5 | "a whole-word setting: each setter stores one aligned word and the last setter wins, which is what setting it means" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 118:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 123:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 185:5 | "only the main thread running jet_rt_main writes its root control" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 246:5 | "only the main thread running jet_rt_main writes its root control" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 249:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 252:5 | "entry is the loader/object's checked executable System V void(void) address, not a closure header" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 258:5 | "only the main thread running jet_rt_main writes its root control" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 280:5 | "the entry has returned and drained its workers; only the main thread runs here" |
| Compiler | Compiler/JetRuntime/Source/MemRegistry.jet | 20:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/MemStorage.jet | 23:5 | "address borrows an initialized word in a native allocator header, descriptor, drop record, or Fixed metadata span" |
| Compiler | Compiler/JetRuntime/Source/MemStorage.jet | 26:5 | "address exclusively owns one aligned writable allocator or Fixed metadata word" |
| Compiler | Compiler/JetRuntime/Source/MemStorage.jet | 84:5 | "code is the Lower-generated address-based lazy.drop thunk for the initialized value at pointer; its SysV result is Unit and discarded, and no allocator lock is held" |
| Compiler | Compiler/JetRuntime/Source/Memo.jet | 10:5 | "address borrows an aligned word of a live memo cell or its entry array" |
| Compiler | Compiler/JetRuntime/Source/Memo.jet | 13:5 | "address is an aligned word of a memo cell or entry array the caller's lock owns" |
| Compiler | Compiler/JetRuntime/Source/NativeLayoutABI.jet | 11:5 | "thread-confined live Layout storage lends this initialized aligned word" |
| Compiler | Compiler/JetRuntime/Source/NativeLayoutABI.jet | 14:5 | "thread-confined native Layout storage owns this aligned word" |
| Compiler | Compiler/JetRuntime/Source/NativeLayoutABI.jet | 78:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/NativeLayoutText.jet | 62:5 | "runtime-init (jet_rt_layout_init) runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/NetRoutes.jet | 120:9 | "the owned terminated host copy truncates at its validated scope suffix" |
| Compiler | Compiler/JetRuntime/Source/NetRoutes.jet | 438:9 | "output lends the C ABI bool result byte, not an eight-byte word" |
| Compiler | Compiler/JetRuntime/Source/NetUnix.jet | 189:5 | "private scratch lends one native int and its 4B socklen; getsockopt retains neither pointer" |
| Compiler | Compiler/JetRuntime/Source/Observe.jet | 18:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Observe.jet | 25:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Observe.jet | 147:5 | "an idempotent flag: every writer stores the same 1" |
| Compiler | Compiler/JetRuntime/Source/Observe.jet | 453:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/ObserveLive.jet | 70:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/ObserveLive.jet | 118:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/ObserveLive.jet | 247:5 | "vetted Observe Linux file/clock syscalls borrow native buffers only for their duration" |
| Compiler | Compiler/JetRuntime/Source/ObserveLive.jet | 515:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Parallel.jet | 43:9 | "checked System V (env,start,end)-&gt;Unit callback; env remains borrowed and each range has exclusive output slots" |
| Compiler | Compiler/JetRuntime/Source/PluginHost.jet | 105:5 | "the linked guest entry reads the live instance and argument area only for this synchronous call" |
| Compiler | Compiler/JetRuntime/Source/ProcessExecutionCollect.jet | 311:9 | "ok is the canonical writable Bool output slot" |
| Compiler | Compiler/JetRuntime/Source/ProcessExecutionCollect.jet | 319:5 | "ok lends exactly the C ABI Bool byte" |
| Compiler | Compiler/JetRuntime/Source/ProcessExecutionCollect.jet | 422:13 | "the child mutex holds the capture builder's initialized bytes" |
| Compiler | Compiler/JetRuntime/Source/ProcessRoutes.jet | 112:5 | "supported_out lends exactly one writable C bool byte; Linux implements F_DUPFD_CLOEXEC" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 33:9 | "written under the scheduler root mutex" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 245:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/ProveJournal.jet | 97:5 | "the harness main arms the journal before any test, so before any other thread reads it" |
| Compiler | Compiler/JetRuntime/Source/ProveJournal.jet | 165:5 | "into lends `capacity` writable bytes for a raw Linux read of stdin" |
| Compiler | Compiler/JetRuntime/Source/ProveJournal.jet | 181:5 | "cell lends 24 writable bytes to TIOCGPGRP and getpgrp takes none" |
| Compiler | Compiler/JetRuntime/Source/ProveJournal.jet | 189:9 | "cell lends the new and old signal masks and the group id to rt_sigprocmask and TIOCSPGRP" |
| Compiler | Compiler/JetRuntime/Source/ProveJournal.jet | 197:9 | "cell lends the group id to TIOCSPGRP and the old mask to rt_sigprocmask" |
| Compiler | Compiler/JetRuntime/Source/Reactive.jet | 34:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Reactive.jet | 74:5 | "the observer id is a label nothing reads back; a racing creator can only repeat a label" |
| Compiler | Compiler/JetRuntime/Source/ReceiptSections.jet | 235:5 | "the terminated path and the 128-byte stat fact row are live for the syscall" |
| Compiler | Compiler/JetRuntime/Source/ReceiptSections.jet | 325:13 | "the path copy owns its terminator byte and every byte before it" |
| Compiler | Compiler/JetRuntime/Source/ReceiptSections.jet | 342:13 | "the path copy owns every byte before its terminator" |
| Compiler | Compiler/JetRuntime/Source/Refcount.jet | 31:5 | "count addresses the aligned prefix word of a live refcounted block the caller holds" |
| Compiler | Compiler/JetRuntime/Source/Refcount.jet | 37:5 | "count addresses the aligned prefix word of a block the caller just allocated and still owns exclusively" |
| Compiler | Compiler/JetRuntime/Source/Refcount.jet | 43:5 | "LOCK XADD on the aligned prefix word of a live refcounted block; the caller's reference keeps it live" |
| Compiler | Compiler/JetRuntime/Source/Refcount.jet | 70:9 | "count addresses the aligned prefix word of a live refcounted block whose only reference the caller holds" |
| Compiler | Compiler/JetRuntime/Source/Refcount.jet | 90:9 | "LOCK CMPXCHG on the aligned prefix word of a live refcounted block; the caller's reference keeps it live" |
| Compiler | Compiler/JetRuntime/Source/RouteACore.jet | 21:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteACore.jet | 29:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteAEncoding.jet | 93:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteAEncoding.jet | 467:5 | "the terminated raw path and the 128-byte stat fact row are live for the syscall" |
| Compiler | Compiler/JetRuntime/Source/RouteAFiles.jet | 31:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteAFiles.jet | 488:5 | "the audited caller supplies the Linux syscall's valid word arguments and live buffers" |
| Compiler | Compiler/JetRuntime/Source/RouteAFiles.jet | 494:5 | "the audited caller supplies the Linux syscall's valid word arguments and live buffers" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 46:5 | "the raylib canvas belongs to the one thread that drives the window" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 51:9 | "the raylib canvas belongs to the one thread that drives the window" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 56:5 | "the raylib canvas belongs to the one thread that drives the window" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 223:5 | "each row start in [at, stop) lends count writable canvas pixels of four bytes each, inside the live canvas" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 272:5 | "each row start in [at, stop) lends tail + 8 writable canvas bytes, inside the live canvas" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 320:5 | "each row start in [at, stop) lends count readable and writable canvas pixels of four bytes each, inside the live canvas" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 393:5 | "the raylib canvas belongs to the one thread that drives the window" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 405:5 | "the raylib textures belong to the one thread that drives the window" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 463:9 | "the raylib textures belong to the one thread that drives the window" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 470:5 | "the raylib textures belong to the one thread that drives the window" |
| Compiler | Compiler/JetRuntime/Source/RouteAGame.jet | 479:5 | "the raylib textures belong to the one thread that drives the window" |
| Compiler | Compiler/JetRuntime/Source/RouteALog.jet | 36:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteAMath.jet | 13:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteSys.jet | 21:5 | "the caller lends the C-ABI tuple's Bool byte and two writable Int words" |
| Compiler | Compiler/JetRuntime/Source/RouteSys.jet | 29:5 | "some lends a writable C-ABI Option success word" |
| Compiler | Compiler/JetRuntime/Source/RouteSys.jet | 102:5 | "averages owns three initialized f64 words; the caller lends the tuple's Bool byte, Int word and three Float words" |
| Compiler | Compiler/JetRuntime/Source/RouteSys.jet | 136:5 | "buffer owns an initialized struct tms; the caller lends the tuple's Bool byte and six Int words" |
| Compiler | Compiler/JetRuntime/Source/RouteTerm.jet | 60:5 | "the caller lends the tuple's Bool byte, Text word and Option&lt;Text&gt; tag/payload words" |
| Compiler | Compiler/JetRuntime/Source/RouteTerm.jet | 231:5 | "the caller lends the tuple's Text word and Option&lt;Text&gt; tag/payload words" |
| Compiler | Compiler/JetRuntime/Source/RouteTerm.jet | 334:13 | "written under the host lock" |
| Compiler | Compiler/JetRuntime/Source/RouteTesting.jet | 84:13 | "written under the host lock" |
| Compiler | Compiler/JetRuntime/Source/RouteTesting.jet | 93:9 | "written under the host lock" |
| Compiler | Compiler/JetRuntime/Source/ScalarReflectGlue.jet | 4:5 | "borrows an initialized aligned word in a live scalar/reflection carrier" |
| Compiler | Compiler/JetRuntime/Source/ScalarReflectGlue.jet | 7:5 | "owns a writable aligned word in a native scalar/reflection carrier" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 59:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 132:5 | "the cold OS leaf borrows its initialized stack word; a successful nonzero base is this attached thread's live Jet TCB or the detached zero word" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 205:5 | "written under the scheduler root mutex" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 216:9 | "written under the scheduler root mutex" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 232:9 | "written under the scheduler root mutex" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 535:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 796:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 931:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 1186:9 | "sched_yield retains no memory and yields this OS worker" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 1293:9 | "sched_yield retains no memory and yields this OS thread" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 13:5 | "address names bytes bytes of live storage the caller owns or is freeing; effectful asm stores wipe every byte before release or reuse" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 38:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 110:17 | "secret is a registered live Secret header (Close unregisters before it clears); its buffer holds capacity bytes" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 128:5 | "start..start+length covers the Secret buffer's mapped pages; madvise and mlock change only their dump and residency attributes" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 147:5 | "bytes is the moved native [U8] header; secret is fresh aligned24B storage and takes over its buffer reference without copying secret material" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 169:5 | "secret is fresh aligned24B storage this call owns exclusively" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 188:5 | "secret is a live exclusively borrowed native24B Secret header; len/cap/data follow the native List layout and all byte owners remain inside its buffer" |
| Compiler | Compiler/JetRuntime/Source/Sentry.jet | 14:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Shared.jet | 62:5 | "x86 pause only hints that this thread is waiting in a spin loop; no memory is touched" |
| Compiler | Compiler/JetRuntime/Source/Shared.jet | 437:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/SharedTransactions.jet | 17:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/SharedTransactions.jet | 23:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 6:5 | "borrows an aligned live synchronization word" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 10:5 | "publishes an aligned live synchronization word with x86 release ordering" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 16:5 | "atomic exchange on an aligned live synchronization word" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 22:5 | "atomic fetch-add on an aligned live synchronization word" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 28:5 | "atomic compare-exchange on an aligned live synchronization word" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 34:5 | "futex wake borrows a live aligned word and retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 42:5 | "futex borrows live state; timed wait uses this attached thread's reserved TCB scratch until the syscall returns" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 54:5 | "clock_gettime writes two words of thread-owned TCB scratch; no pointer escapes" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 108:5 | "sync_sole_inflight is this sole thread's own count until sync_sole is cleared; sync_sole_end reads it only after a membarrier" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 125:5 | "an elided section: only this thread has written the lock word and the count since sync_sole_enter" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 137:5 | "membarrier and sched_yield retain no memory; sync_sole only ever goes from 1 to 0" |
| Compiler | Compiler/JetRuntime/Source/Task.jet | 511:9 | "written once, zeroed first, under the TLS registry mutex" |
| Compiler | Compiler/JetRuntime/Source/TaskGroup.jet | 254:9 | "the canonical task selector yields without consuming any child result" |
| Compiler | Compiler/JetRuntime/Source/TerminalColor.jet | 8:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/TerminalInput.jet | 22:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/TerminalProcessRoutes.jet | 58:9 | "TIOCSWINSZ borrows one initialized stack winsize word; the Child72 lock prevents close/recycling until ioctl returns" |
| Compiler | Compiler/JetRuntime/Source/Test.jet | 132:9 | "the OS leaf borrows at most eight initialized bytes from its own stack word" |
| Compiler | Compiler/JetRuntime/Source/TestCapture.jet | 31:9 | "written under the TLS registry mutex" |
| Compiler | Compiler/JetRuntime/Source/TestContracts.jet | 28:5 | "the harness main opens contracts before any test, so before any other thread observes one" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 26:9 | "startup lends the immutable, null-terminated envp pointer array" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 36:13 | "the startup entry is a live NUL-terminated path for open" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 40:17 | "startup is single-threaded and precedes user code" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 86:13 | "buffer lends 4096 writable bytes after the kind byte to getcwd" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 93:13 | "link is a live NUL-terminated /proc path and buffer lends 4096 writable bytes after the kind byte" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 153:5 | "data lends `length` initialized bytes to one write" |
| Compiler | Compiler/JetRuntime/Source/TestSuite.jet | 24:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/TestSuite.jet | 83:5 | "`test: fn` calls `t.run()` on the harness's main thread, before any test task exists" |
| Compiler | Compiler/JetRuntime/Source/TestSuite.jet | 92:5 | "the pinned named-fn box code takes its box and writable inline 16B result storage" |
| Compiler | Compiler/JetRuntime/Source/Text.jet | 16:5 | "address borrows an initialized word in a live text or iterator header" |
| Compiler | Compiler/JetRuntime/Source/Text.jet | 22:5 | "address is an exclusively owned writable header word" |
| Compiler | Compiler/JetRuntime/Source/Text.jet | 26:5 | "address borrows an initialized byte inside the caller's live buffer" |
| Compiler | Compiler/JetRuntime/Source/Text.jet | 32:5 | "address is one exclusively owned writable byte and value is a byte" |
| Compiler | Compiler/JetRuntime/Source/TextGlue.jet | 4:5 | "borrows an initialized aligned word in a live native text diagnostic" |
| Compiler | Compiler/JetRuntime/Source/TextGlue.jet | 7:5 | "owns a writable aligned word in a fresh native text diagnostic" |
| Compiler | Compiler/JetRuntime/Source/TextMethods.jet | 146:9 | "item0 lends the C-ABI tuple's one writable Bool byte" |
| Compiler | Compiler/JetRuntime/Source/TextMethods.jet | 151:5 | "item0 lends the C-ABI tuple's one writable Bool byte" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 62:5 | "every caller has attached before running Jet TLS code; an unattached thread's GS names the readable zero word, which the allocator alone treats as no TCB" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 72:5 | "GS is reserved for Jet and base is a live self-addressed TCB or the detached zero word" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 79:5 | "gettid identifies this thread without reading its inherited GS" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 123:5 | "getpid retains no memory" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 193:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 287:9 | "the registry owns this live closure header whose code accepts header and one I64 value" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 377:5 | "the caller transferred a System V I64-&gt;I64 code address and one argument to this thread" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 394:5 | "getrlimit(RLIMIT_STACK) writes its two-word record into the owned 16-byte scratch and retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 461:5 | "fresh raw clone stack mapping with a PROT_NONE first-page guard; child may call only Jet/syscall code, never musl/Rust with inherited FS" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 478:5 | "guarded stack is seeded with Jet-only entry/arg; TID word is live until join; flags share VM, FS, FILES, SIGHAND, THREAD, SYSVSEM and enable parent-set/child-clear TID" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 493:9 | "kernel CHILD_CLEARTID clears this live word and wakes a non-private futex; recheck handles exit-before-wait and EINTR" |
| Compiler | Compiler/JetRuntime/Source/TimeGlue.jet | 3:5 | "borrows an initialized aligned word inside a native time carrier" |
| Compiler | Compiler/JetRuntime/Source/TimeGlue.jet | 6:5 | "owns a writable aligned word inside a fresh native time carrier" |
| Compiler | Compiler/JetRuntime/Source/Trap.jet | 8:5 | "the caller borrows an initialized word of a live JetText or static require descriptor" |
| Compiler | Compiler/JetRuntime/Source/Trap.jet | 14:5 | "the caller borrows exactly one initialized byte of a live UTF-8 carrier" |
| Compiler | Compiler/JetRuntime/Source/Trap.jet | 20:5 | "the caller owns eight writable bytes in a scratch mapping or allocated JetText storage" |
| Compiler | Compiler/JetRuntime/Source/Trap.jet | 26:5 | "address is one writable byte in the allocated debug-text buffer; value is a byte" |
| Compiler | Compiler/JetRuntime/Source/Trap.jet | 32:5 | "audited runtime syscall: write borrows live text, mmap creates scratch, munmap releases it, exit_group ends the process" |
| Compiler | Compiler/JetRuntime/Source/Trap.jet | 89:9 | "the OS leaf borrows a stack word; generated calls have 1..8 valid bytes" |
| Compiler | Compiler/JetRuntime/Source/UiBackends.jet | 468:5 | "a web page's module runs on one thread" |
| Compiler | Compiler/JetRuntime/Source/UiBackends.jet | 514:5 | "only an attached web build records, on its one thread" |
| Compiler | Compiler/JetRuntime/Source/UiBackends.jet | 524:5 | "a web page's module runs on one thread" |
| Compiler | Compiler/JetRuntime/Source/UiBackends.jet | 905:9 | "written under the host lock" |
| Compiler | Compiler/JetRuntime/Source/UiCarriersEnums.jet | 5:5 | "address borrows an initialized native UI carrier or E4 header word" |
| Compiler | Compiler/JetRuntime/Source/UiCarriersEnums.jet | 10:5 | "address exclusively owns one writable native UI carrier word" |
| Compiler | Compiler/JetRuntime/Source/Unicode.jet | 145:5 | "some is the caller's writable aligned Option&lt;Int&gt; payload slot, written only on success" |
| Compiler | Compiler/JetRuntime/Source/VaultTestHooks.jet | 22:5 | "written under the TLS registry mutex" |
| Compiler | Compiler/JetRuntime/Source/VaultTestHooks.jet | 27:5 | "written under the TLS registry mutex" |
| Compiler | Compiler/JetRuntime/Source/VaultTestHooks.jet | 182:5 | "written under the vault test backup mutex" |
| Compiler | Compiler/JetRuntime/Source/World.jet | 76:5 | "registered provider code remains live at this world/lease boundary; output lends one writable word" |
| Compiler | Compiler/JetRuntime/Source/WsRoutes.jet | 500:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/XMLGlue.jet | 4:5 | "borrows an initialized aligned word in a live XML carrier or optional input" |
| Compiler | Compiler/JetRuntime/Source/XMLGlue.jet | 7:5 | "owns a writable aligned word in a fresh XML carrier or caller output" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 11:5 | "a failed allocator witness terminates this isolated test process" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 27:5 | "both allocations own count bytes; touching their endpoints must not alias each other or their metadata" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 67:5 | "first's private header names its live slab, whose arena word is fixed" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 84:5 | "the flushed cache left the slab linked; its live count and the class cell are stable while this thread is the heap's only user" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 94:9 | "pointer's private header identifies its still-live slab, which must be the same mapping" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 99:9 | "no other thread touches the slab or its class list" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 113:5 | "first's private header holds its mapping base and length" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 137:9 | "the test owns the full payload and can read its allocator header" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 154:9 | "the chain word remains readable until its payload is freed" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 161:5 | "after all payloads are freed, exactly one empty slab is retained; any second empty slab was unlinked onto the free-slab stack" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 179:9 | "the test owns the full payload and keeps its chain word there" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 187:9 | "the chain word remains readable until its payload is freed" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 194:5 | "this thread is the heap's only user; the stack word is read once and the stacked slab's link word stays mapped" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 202:5 | "the retired slab is the stack's top again; its pages were never unmapped" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 229:5 | "the test owns this fresh align-8 block and its count word" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 258:5 | "the test owns this fresh align-8 block of size bytes" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 267:5 | "the test holds both owners of this block and its count word" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 273:5 | "the test still owns this block" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 279:5 | "a cached small block stays mapped; its payload bytes are not cache links" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 294:5 | "the test owns these fresh align-8 blocks and their count words" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 298:5 | "the test owns these fresh align-8 blocks and their count words" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 327:9 | "the test owns the payload and keeps its chain word there" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 336:5 | "row is this thread's own cache row" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 340:9 | "the chain word remains readable until its payload is freed" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 346:9 | "row is this thread's own cache row" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 353:5 | "after the flush the row is empty" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 363:5 | "row is this thread's own cache row" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 372:5 | "the test owns this fresh align-8 block and its header words" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 454:5 | "the exact success line is two bounded stack words written to stdout" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocationPolicyTests.jet | 4:5 | "the single-threaded fixture reads the process-lifetime tracked-node count after all its payloads were freed" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/EnvOverlayKernel.jet | 6:5 | "failed witness terminates its isolated real-runtime process" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet | 5:5 | "an isolated failed scheduler witness exits with no further runtime cleanup" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet | 209:5 | "fixture byte writes stay within owned 19/20-byte text buffers" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet | 213:5 | "fixture byte writes stay within owned 19/20-byte text buffers" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet | 217:5 | "fixture byte writes stay within owned 19/20-byte text buffers" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet | 221:5 | "fixture byte writes stay within owned 19/20-byte text buffers" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet | 230:5 | "fixture byte writes stay within an owned one-byte text buffer" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet | 234:5 | "fixture byte writes stay within an owned one-byte text buffer" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 6:5 | "a failed isolated Secret witness terminates this test process" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 14:5 | "header is fresh aligned24B storage owned by the test; data owns cap times the host U8 stride bytes or is zero for an empty List" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 23:5 | "the pointer is a live small allocation; this isolated test is the sole allocator user and can inspect its retained slab metadata" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 36:5 | "input and secret are distinct live headers; adoption leaves one live buffer owner and borrowed bytes alias only that parent's header" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 57:5 | "the test owns data and checks the initialized U8 slots, including epoch padding, before any release" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 68:5 | "secret still owns its live24B header after Close; the buffer was released, and no pointer into it is used" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 80:5 | "reused is newly allocated live32B storage exclusively owned by this witness, not a read of freed storage" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 112:5 | "data is this test's live List allocation; every capacity byte is dirtied before the wipe" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 120:5 | "the same live allocation remains exclusively owned while this test checks all wiped bytes" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 140:5 | "data is this test's fresh live allocation of bytes bytes" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 152:5 | "secret is a live Secret header over its own buffer; data stays live through the retained reference" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 166:5 | "data is still live: the other holder's reference was never released" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 177:5 | "frozen is a live Secret header over its own buffer" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 217:5 | "both buffers are this test's fresh live allocations of bytes bytes" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 235:5 | "kept is a live Secret header and kept_data its live buffer; the scrub only zeroes the buffer" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 258:5 | "this isolated native witness writes its17B success literal and exits without introducing any text or collection runtime dependency" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SentryInlineTests.jet | 8:1 | "the raw pointer deliberately escapes its frame for the sentry witness" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SentryInlineTests.jet | 14:1 | "an expired inline raw read must stop before any pointee byte is loaded" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SentryTests.jet | 211:5 | "the checked Source callback uses the exact two-word native ABI" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/TextTests.jet | 6:5 | "a failed text runtime witness terminates only its isolated process" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 8:5 | "a failed isolated runtime witness ends the process without allocation or libc" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 36:5 | "gettid verifies identity without trusting the TCB under test" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 45:5 | "ARCH_GET_GS writes a caller-owned word; the inherited pointer is compared, never dereferenced" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 204:13 | "sched_yield widens both sides of the check-to-wait race without timing assumptions" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 284:5 | "heap-free fixture success line is one eight-byte write" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/UnicodeTests.jet | 8:5 | "a failed Unicode runtime witness ends only its isolated test process" |
| Compiler | Compiler/JetSema/Tests/ScalarGlobalFixtures.jet | 21:5 | "single-threaded test: plain module writes of each scalar type" |
| Compiler | Compiler/JetSema/Tests/XmlCheckerRootsFixtures.jet | 10:5 | "no raw operations in this regression" |
| Compiler | Compiler/JetSema/Tests/XmlCheckerRootsFixtures.jet | 11:9 | "nested tail retains its value grammar" |
| Compiler | Compiler/JetTestKit/Source/Exec.jet | 345:5 | "the testkit stops its own step child at the resident-memory guard" |
| Compiler | Compiler/JetTestKit/Source/Exec.jet | 498:9 | "the testkit stops a process its own case left running in the case scratch" |
| Compiler | Compiler/JetTestKit/Source/StepsControl.jet | 133:5 | "kill(pid, 0) observes the case-owned child without sending a signal" |
| Compiler | Compiler/JetTestKit/Source/StepsSessions.jet | 363:5 | "the case explicitly authorizes signalling its own spawned child" |
| Core | Core/Auth/Auth.jet | 113:5 | "auth PASETO v4.public verification uses the fixed Ed25519 primitive" |
| Core | Core/Crypto/Crypto.jet | 320:5 | "vault key import: validate a serialized wrapped carrier at the typed crypto boundary" |
| Core | Core/Crypto/Crypto.jet | 326:5 | "vault key import: expose the authenticated wrapped carrier, not plaintext key material" |
| Core | Core/Crypto/Crypto.jet | 331:5 | "vault key import: construct the typed recipient unlock request" |
| Core | Core/Crypto/Crypto.jet | 336:5 | "vault key import: construct the typed passphrase unlock request" |
| Core | Core/Crypto/Crypto.jet | 715:5 | "the expert API explicitly authorizes authenticated legacy-v1 migration" |
| Core | Core/Crypto/Crypto.jet | 1260:5 | "crypto expert: Argon2id password hashing" |
| Core | Core/Crypto/Crypto.jet | 1287:5 | "crypto expert: Argon2id password verification" |
| Core | Core/Crypto/Crypto.jet | 2131:5 | "crypto expert: the authenticated envelope owns the one-time Poly1305 key and MAC framing" |
| Core | Core/Crypto/Crypto.jet | 2244:5 | "crypto expert: typed key agreement validates key widths and rejects all-zero shared secrets" |
| Core | Core/Crypto/Crypto.jet | 2253:5 | "crypto expert: typed signing keys validate the Ed25519 seed width" |
| Core | Core/Crypto/Crypto.jet | 2259:5 | "crypto expert: typed verification validates public key and signature widths before strict verification" |
| Core | Core/Crypto/ExpertCt.jet | 13:5 | "Core fixed-limb workspace reserves its complete operation-owned word capacity before filling" |
| Core | Core/Crypto/ExpertCt.jet | 39:5 | "Core fixed-limb extraction reserves its exact public output width before filling" |
| Core | Core/Crypto/ExpertCt.jet | 325:5 | "Core Ed25519 hashing reserves the exact combined public part lengths before filling" |
| Core | Core/Crypto/Jetc.jet | 98:5 | "Core JETC2 applies the fixed XChaCha20-Poly1305 suite and protocol-owned nonce/AAD domains" |
| Core | Core/Crypto/Jetc.jet | 106:5 | "Core JETC2 authenticates a bounded record before releasing its plaintext" |
| Core | Core/Crypto/Jetc.jet | 114:5 | "Core JETC2 borrows shared material through expert, wiping its copy before propagating HKDF success or failure" |
| Core | Core/Crypto/Jetc.jet | 129:5 | "Core JETC2 exposes only an operation-scoped key copy and wipes it before either seal outcome escapes" |
| Core | Core/Crypto/Jetc.jet | 144:5 | "Core JETC2 exposes only an operation-scoped key copy and wipes it before either authentication outcome escapes" |
| Core | Core/Crypto/Jetc.jet | 159:5 | "Core JETC2 copies its derived wrap nonce and file key only for sealing, wiping both copies on either outcome" |
| Core | Core/Crypto/Jetc.jet | 177:5 | "Core JETC2 copies its derived wrap nonce only for authentication, wiping the copy on either outcome" |
| Core | Core/Crypto/Jetc.jet | 235:5 | "Core JETC2 checks secret draw widths through the audited exposure and wipes the temporary copy" |
| Core | Core/Crypto/Jetc.jet | 273:5 | "Core JETC2 copies a validated ephemeral Secret into a zeroizing X25519SecretKey; both owners retain their Close duty" |
| Core | Core/Crypto/Vault.jet | 230:5 | "vault typed load provider boundary" |
| Core | Core/Crypto/Vault.jet | 237:5 | "vault typed current provider boundary" |
| Core | Core/Crypto/Vault.jet | 243:5 | "vault typed status provider boundary" |
| Core | Core/Crypto/Vault.jet | 250:5 | "vault typed versions provider boundary" |
| Core | Core/Crypto/Vault.jet | 257:5 | "vault typed generation provider boundary" |
| Core | Core/Crypto/Vault.jet | 264:5 | "vault typed store provider boundary" |
| Core | Core/Crypto/Vault.jet | 272:5 | "vault key import: signing key material" |
| Core | Core/Crypto/Vault.jet | 280:5 | "vault key import: X25519 secret material" |
| Core | Core/Crypto/Vault.jet | 288:5 | "vault wrapped import provider boundary" |
| Core | Core/Crypto/Vault.jet | 295:5 | "vault typed rotation provider boundary" |
| Core | Core/Crypto/Vault.jet | 302:5 | "vault typed retirement provider boundary" |
| Core | Core/Crypto/Vault.jet | 309:5 | "vault typed revocation provider boundary" |
| Core | Core/Crypto/Vault.jet | 316:5 | "vault write authorization provider boundary" |
| Core | Core/Crypto/Vault.jet | 323:5 | "vault wrapped authorization provider boundary" |
| Core | Core/Crypto/Vault.jet | 329:5 | "vault typed generation commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 335:5 | "vault typed store commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 341:5 | "vault key import commit: signing key material" |
| Core | Core/Crypto/Vault.jet | 347:5 | "vault key import commit: X25519 secret material" |
| Core | Core/Crypto/Vault.jet | 353:5 | "vault wrapped import commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 359:5 | "vault typed rotation commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 365:5 | "vault typed retirement commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 371:5 | "vault typed revocation commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 377:5 | "vault passphrase export provider boundary" |
| Core | Core/Crypto/Vault.jet | 384:5 | "vault recipient export provider boundary" |
| Core | Core/Data/Data.jet | 2441:9 | "native path bytes originate from the host environment and are split at ASCII boundaries" |
| Core | Core/Data/Data.jet | 3340:5 | "inspect existing private Decode carriers only inside checked Core data source" |
| Core | Core/Data/Data.jet | 3674:5 | "inspect existing private Decode carriers only inside checked Core data source" |
| Core | Core/Db/Db.jet | 1468:9 | "this consuming Close owns the sole SQLite database pointer, including open-error handles; close_v2 permits live statements to finalize later" |
| Core | Core/Db/Db.jet | 1475:9 | "this consuming Close owns the sole statement pointer; SQLite permits finalize(NULL) and rusqlite drops finalize's status" |
| Core | Core/Db/Db.jet | 1503:1 | "SQLite owns a live NUL-terminated error/name buffer; strlen and unpack finish before any following engine call" |
| Core | Core/Db/Db.jet | 1509:5 | "copy this exclusively borrowed connection's error before reset/close can invalidate or overwrite it" |
| Core | Core/Db/Db.jet | 1525:5 | "NUL-free filename goes through the canonical C String adapter; the output pointer slot belongs to this stack frame" |
| Core | Core/Db/Db.jet | 1549:5 | "close the exclusively borrowed connection's sole SQLite pointer once; zeroing it below makes the later Close a no-op" |
| Core | Core/Db/Db.jet | 1556:5 | "exclusive live database, constant transaction SQL, canonical C String adapter, and no callback/error output allocation" |
| Core | Core/Db/Db.jet | 1565:5 | "read the exclusively borrowed SQLite connection's autocommit flag" |
| Core | Core/Db/Db.jet | 1589:5 | "key stays alive and unmodified while SQLite borrows its UTF-8 data+byte length; the output slots are stack-owned" |
| Core | Core/Db/Db.jet | 1622:5 | "reset this exclusively owned statement and clear the TRANSIENT bindings before returning it to its owning cache" |
| Core | Core/Db/Db.jet | 1633:5 | "exclusive statement loan; scalar values are copied, borrowed String lives unmodified until SQLITE_TRANSIENT finishes, and packed blobs are freed only after SQLite copies them" |
| Core | Core/Db/Db.jet | 1685:5 | "step/reset the exclusive statement, matching rusqlite execute_with_bound_parameters, then copy any SQLite error before another operation" |
| Core | Core/Db/Db.jet | 1716:1 | "value is alive and unchanged while reading its UTF-8 byte count; no borrowed pointer escapes" |
| Core | Core/Db/Db.jet | 1740:5 | "copy every SQLite column into an owned value while its row remains live, before the next step/reset/finalize; byte unpack reads only column_bytes bytes" |
| Core | Core/Db/Db.jet | 1915:5 | "SQLite's allocator counter is read-only; this test owns its connections and retains no foreign column pointers" |
| Core | Core/Db/Db.jet | 1919:5 | "all test handles have reached Close before reading SQLite's allocator counter" |
| Core | Core/Db/Db.jet | 1924:5 | "failed opens, including non-null error handles, have already reached Close" |
| Core | Core/Encoding/Cbor.jet | 99:5 | "inspect private typed carriers without changing public Dyn variants" |
| Core | Core/Encoding/Cbor.jet | 168:5 | "validated private carriers retain byte strings for typed Encode" |
| Core | Core/Encoding/CborStream.jet | 218:9 | "the retained key copies exactly the already validated text bytes" |
| Core | Core/Encoding/CborStream.jet | 321:17 | "the exact CBOR text allocation passed strict UTF-8 validation" |
| Core | Core/Encoding/Csv.jet | 141:9 | "input is a String; original complete UTF-8 scalars and ASCII quote escapes entered the field" |
| Core | Core/Encoding/Csv.jet | 379:5 | "the exact owned query bytes passed the shared strict UTF-8 validator" |
| Core | Core/Encoding/Csv.jet | 399:5 | "a String's bytes cut at scalar boundaries, plus ASCII quotes, are complete UTF-8" |
| Core | Core/Encoding/Csv.jet | 871:9 | "each completed UTF-8 scalar was validated before its original bytes entered this field" |
| Core | Core/Encoding/Eetf.jet | 549:9 | "inspect private typed carriers without changing public Dyn variants" |
| Core | Core/Encoding/Json.jet | 113:5 | "preserve the shipped decoder's hidden stderr effect at its existing host boundary" |
| Core | Core/Encoding/Json.jet | 121:5 | "preserve the shipped decoder's hidden wall-clock snapshot at its existing host boundary" |
| Core | Core/Encoding/Json.jet | 726:13 | "preserve the native compiler-private Dyn carriers in Core JSON event projection" |
| Core | Core/Encoding/Json.jet | 731:13 | "preserve the native compiler-private Dyn carriers in Core JSON event projection" |
| Core | Core/Encoding/Stream.jet | 16:9 | "the Object variant was checked immediately before borrowing its ordered backing" |
| Core | Core/Encoding/Stream.jet | 24:5 | "transfer the owned ordered entries allocation into the native Object carrier" |
| Core | Core/Encoding/Stream.jet | 30:5 | "check the private Bytes variant immediately before borrowing its backing" |
| Core | Core/Encoding/Stream.jet | 37:5 | "the shared parser validated this exact numeric lexeme" |
| Core | Core/Encoding/Stream.jet | 43:5 | "retain decoded JSON text until the target Decode contract is known" |
| Core | Core/Encoding/Stream.jet | 324:9 | "the shared UTF-8 validator just accepted this owned byte allocation" |
| Core | Core/Encoding/Stream.jet | 395:9 | "the shared UTF-8 validator just accepted this owned ASCII token" |
| Core | Core/Encoding/Stream.jet | 400:13 | "retain a validated number lexeme in the private event carrier until typed decoding" |
| Core | Core/Encoding/Stream.jet | 602:9 | "transfer already validated UTF-8 storage to inspect its actual owned capacity" |
| Core | Core/Encoding/Stream.jet | 613:9 | "the private numeric event is projected only into the matching typed tree carrier" |
| Core | Core/Encoding/Stream.jet | 989:5 | "the shared UTF-8 validator just accepted this owned allocation" |
| Core | Core/Encoding/Stream.jet | 1001:5 | "the byte position was checked against this String's storage length" |
| Core | Core/Encoding/Stream.jet | 1565:5 | "the shared UTF-8 validator accepted this exact owned formatter buffer" |
| Core | Core/Encoding/Stream.jet | 1576:5 | "transfer the owned UTF-8 String buffer before re-spelling its shortest digits" |
| Core | Core/Encoding/Stream.jet | 1722:13 | "retain a validated number lexeme in the private event carrier" |
| Core | Core/Encoding/Stream.jet | 1727:13 | "project the native compiler-private Bytes carrier into its event" |
| Core | Core/Encoding/Stream.jet | 1793:13 | "classify the private Bytes carrier without borrowing or exposing its payload" |
| Core | Core/Encoding/Stream.jet | 1936:5 | "owned validated UTF-8 allocation transfers in the encoding source seam" |
| Core | Core/Encoding/Stream.jet | 1973:5 | "exercise the typed Encode byte carrier at the JSON preflight boundary" |
| Core | Core/Encoding/XmlEngine.jet | 132:5 | "all callers iterate or check the UTF-8 byte bounds before borrowing this byte" |
| Core | Core/Encoding/XmlEngine.jet | 170:5 | "the caller selected a scalar-aligned window of validated UTF-8 bytes" |
| Core | Core/Encoding/XmlEngine.jet | 2180:5 | "attribute whitespace compaction preserves the already validated UTF-8 byte sequence" |
| Core | Core/Encoding/Yaml.jet | 440:5 | "inspect compiler-private Bytes and typed carriers; public Dyn variants are unchanged" |
| Core | Core/Files/Files.jet | 498:1 | "the compiler store passes a live FileReader and Directory, with one NUL-free plain destination name; no source pathname is reopened" |
| Core | Core/Files/Files.jet | 504:1 | "compiler store owns the explicit child descriptor slot and retains the held Directory until the process leaf duplicates its descriptor" |
| Core | Core/Http/Client.jet | 655:5 | "checked resolver octets and port transferred into the socket carrier" |
| Core | Core/Jobs/Jobs.jet | 133:5 | "queue preflight reads final-component metadata without following a symlink; metadata failures are ignored exactly as the SQLite provider does" |
| Core | Core/Jobs/Jobs.jet | 138:5 | "create the lexical parent with the existing raw recursive mkdir kernel before SQLite open" |
| Core | Core/Jobs/Jobs.jet | 363:13 | "match the queue's existing raw wall-clock wait without advancing controlled scheduler time" |
| Core | Core/Jobs/Jobs.jet | 544:5 | "publish already committed queue observations through the existing validated Devtools event/sink boundary" |
| Core | Core/Jobs/Jobs.jet | 559:5 | "publish source-projected queue status through the existing validated Devtools event/sink boundary" |
| Core | Core/Mem/Mem.jet | 36:1 | "`addr` is the address of a live, aligned `T` that stays valid for every use of the returned pointer" |
| Core | Core/Mem/Mem.jet | 40:1 | "`pointer` names a live, aligned, readable `T` for the whole access" |
| Core | Core/Mem/Mem.jet | 44:1 | "`pointer` names a live, aligned, writable `T` for the whole access" |
| Core | Core/Net/Net.jet | 137:5 | "checked IPv4/IPv6 octets transferred into the native address carrier" |
| Core | Core/Net/Net.jet | 143:5 | "read native address octets; no parsing or formatting policy in the carrier leaf" |
| Core | Core/Net/Net.jet | 149:5 | "read the native IP address discriminant" |
| Core | Core/Net/Net.jet | 206:5 | "validated IP octets, port and IPv6 numeric scope transferred into the socket carrier" |
| Core | Core/Net/Net.jet | 213:5 | "read socket IP representation; formatting stays in Jet" |
| Core | Core/Net/Net.jet | 216:5 | "read native socket port" |
| Core | Core/Net/Net.jet | 228:5 | "read native IPv6 socket scope identifier" |
| Core | Core/Net/Net.jet | 430:5 | "read the native UDP packet's socket-address carrier" |
| Core | Core/Net/Tls.jet | 70:9 | "this consuming Close owns the sole jet_tls context; jet_tls_free accepts 0 and never closes the socket" |
| Core | Core/Net/Tls.jet | 549:5 | "the output slot belongs to this frame; jet_tls_new stores 0 or a fresh context that __TLSContext owns" |
| Core | Core/Net/Tls.jet | 942:9 | "the packed copy lives until the matching free below; Mbed TLS only reads it during jet_tls_write" |
| Core | Core/Net/Tls.jet | 948:17 | "frees the packed chunk exactly once after its last jet_tls_write" |
| Core | Core/Net/Tls.jet | 954:9 | "frees the packed chunk exactly once after its last jet_tls_write" |
| Core | Core/Net/Tls.jet | 1483:5 | "configures the live context before setup" |
| Core | Core/Net/Tls.jet | 1487:5 | "configures the live context before setup" |
| Core | Core/Net/Tls.jet | 1492:5 | "NUL-free PEM text is borrowed for this parse; Mbed TLS copies the certificates" |
| Core | Core/Net/Tls.jet | 1498:5 | "the NUL-free path is borrowed for this parse; Mbed TLS copies the certificates" |
| Core | Core/Net/Tls.jet | 1504:5 | "the NUL-free path is borrowed for this parse; Mbed TLS copies the certificates" |
| Core | Core/Net/Tls.jet | 1510:5 | "reads the live context's root list" |
| Core | Core/Net/Tls.jet | 1516:5 | "NUL-free PEM texts are borrowed for this parse; Mbed TLS copies the chain and the key" |
| Core | Core/Net/Tls.jet | 1522:5 | "the NUL-free protocol is copied by jet_tls_alpn_add" |
| Core | Core/Net/Tls.jet | 1528:5 | "binds the live context to the TCPStream's descriptor, which the stream state keeps open for the context's lifetime" |
| Core | Core/Net/Tls.jet | 1534:5 | "one nonblocking handshake step on the live context" |
| Core | Core/Net/Tls.jet | 1540:5 | "one nonblocking read into the live context's own buffer" |
| Core | Core/Net/Tls.jet | 1546:5 | "jet_tls_read just filled `length` bytes of the context's buffer; they are copied before the next engine call" |
| Core | Core/Net/Tls.jet | 1554:5 | "the caller's packed chunk holds `length` readable bytes for this call" |
| Core | Core/Net/Tls.jet | 1560:5 | "one nonblocking close_notify step on the live context" |
| Core | Core/Net/Tls.jet | 1566:5 | "reads the live context's verification flags" |
| Core | Core/Net/Tls.jet | 1572:5 | "reads the live context's negotiated version" |
| Core | Core/Net/Tls.jet | 1578:5 | "the suite name is a static NUL-terminated string; its length and unpack finish before the next engine call" |
| Core | Core/Net/Tls.jet | 1589:5 | "the live context keeps its verified peer chain while the DER is copied out; the output slots belong to this frame" |
| Core | Core/Net/Tls.jet | 1599:5 | "getenv returns a NUL-terminated string or NULL; it is copied before any environment change" |
| Core | Core/Net/Tls.jet | 1606:1 | "the caller guarantees a live NUL-terminated string at `address`" |
| Core | Core/Net/Tls.jet | 1616:5 | "reads the run's journal mode word; no argument is borrowed" |
| Core | Core/Net/Tls.jet | 1623:5 | "the packed copy lives until the matching free below; the runtime copies it into the journal during the call" |
| Core | Core/Net/Tls.jet | 1634:5 | "the packed request lives until the matching free below; the served address points into the run's loaded journal, copied out before return" |
| Core | Core/Process/Process.jet | 148:5 | "transfer a fully initialized Jet process plan into the temporary native ProcessSpec carrier; paired environment columns have equal length" |
| Core | Core/Service/Service.jet | 1427:5 | "read the final path component's OS metadata kind without opening an identity handle" |
| Core | Core/Sys/Sys.jet | 423:1 | "posix umask: the caller owns the process file-creation mask and restores it when other code relies on it" |
| Core | Core/Sys/Sys.jet | 592:5 | "posix getpgid: inspect a process group" |
| Core | Core/Sys/Sys.jet | 598:5 | "posix getsid: inspect a process session" |
| Core | Core/Sys/Sys.jet | 603:1 | "posix getpriority: the caller vouches that `which` names this process or one it controls" |
| Core | Core/Sys/Sys.jet | 610:1 | "posix setpriority: the caller owns the scheduling priority of the process it changes" |
| Core | Core/Sys/Sys.jet | 614:1 | "posix utime: the caller owns the file whose timestamps it rewrites" |
| Core | Core/Sys/Sys.jet | 621:1 | "posix stop: the caller accepts that the process ends here after the shared cleanup boundary" |
| Core | Core/Sys/Sys.jet | 631:1 | "posix pipe: the caller owns both returned descriptors and closes each exactly once" |
| Core | Core/Sys/Sys.jet | 637:1 | "posix close_fd: the caller owns `fd` and nothing uses it after this call" |
| Core | Core/Sys/Sys.jet | 652:1 | "posix mkfifo: the caller owns `path` and removes the named pipe it creates" |
| Core | Core/Sys/Sys.jet | 657:1 | "posix fork: the caller handles both the parent and the child return and reaps the child" |
| Core | Core/Sys/Sys.jet | 661:1 | "posix setsid: the caller owns the process session topology it changes" |
| Core | Core/Sys/Sys.jet | 665:1 | "posix wait: the caller owns the child processes it reaps" |
| Core | Core/Sys/Sys.jet | 671:1 | "posix waitpid: the caller owns the child process it reaps" |
| Core | Core/Sys/Sys.jet | 677:1 | "posix setuid: the caller drops or changes process credentials deliberately" |
| Core | Core/Sys/Sys.jet | 681:1 | "posix setgid: the caller drops or changes process credentials deliberately" |
| Core | Core/Sys/Sys.jet | 685:1 | "posix setpgrp: the caller owns the process group topology it changes" |
| Core | Core/Sys/Sys.jet | 689:1 | "posix setpgid: the caller owns the process group topology it changes" |
| Core | Core/Sys/Sys.jet | 693:1 | "posix kill: the caller owns or is entitled to signal the target process" |
| Core | Core/Sys/Sys.jet | 697:1 | "posix initgroups: the caller changes supplementary groups deliberately" |
| Core | Core/Testing/Testing.jet | 1355:5 | "the caller bounded this byte position by the String storage length" |
| Core | Core/Testing/Testing.jet | 1376:5 | "all appended bytes came from complete UTF-8 strings or ASCII" |
| Core | Core/Tui/Tui.jet | 104:5 | "Core TUI reads owned encoded OS environment bytes from the current host; no UTF-8 assumption" |
| Core | Core/Tui/Tui.jet | 130:5 | "Core TUI samples the same stdout terminal fact consumed by terminal style/progress" |
| Core | Core/Tui/Tui.jet | 656:5 | "Core TUI test: normalized style words and explicit absent UI slots" |
| Core | Core/Ui/Ui.jet | 159:5 | "Core UI text input: the provider sets the explicitly requested IME slot" |
| Core | Core/Ui/Ui.jet | 196:5 | "Core creates the real retained GTK renderer through the registered host allocation boundary" |
| Core | Core/Ui/Ui.jet | 201:5 | "Core creates the real retained terminal renderer through the registered host allocation boundary" |
| Core | Core/Ui/Ui.jet | 206:5 | "Core creates the real retained null renderer through the registered host allocation boundary" |
| Core | Core/Ui/Ui.jet | 217:5 | "Core UI accessibility slot transfer: explicitly present validated metadata" |
| Core | Core/Ui/Ui.jet | 234:5 | "Core UI shortcut slot transfer: explicitly present validated shortcut" |
| Core | Core/Ui/Ui.jet | 261:5 | "Core UI host slot transfer: audited kind, role, explicit absent slots, and normalized style words" |
| Core | Core/Vendor/Sqlite3/Fixture/run.jet | 19:5 | "exclusive out-slots hold SQLite pointers; zero words mean null, SQL is NUL-terminated for each borrowed call, and each live pointer is finalized/closed once" |
| Docs | Docs/audits/g15-2026-11/probes/c4599_28_comptime_asm.jet | 10:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Docs | Docs/audits/g15-2026-11/probes/c4599_28_comptime_asm.jet | 25:5 | "call the audited register-only assembly contract" |
| Docs | Docs/audits/g15-2026-11/probes/c4599_28_comptime_asm_reach.jet | 10:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Docs | Docs/audits/g15-2026-11/probes/c4599_28_comptime_asm_reach.jet | 22:9 | "call the audited register-only assembly contract" |
| Docs | Docs/audits/g15-2026-11/probes/c4599_28_runtime_asm_reach.jet | 10:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Docs | Docs/audits/g15-2026-11/probes/c4599_28_runtime_asm_reach.jet | 22:9 | "call the audited register-only assembly contract" |
| Docs | Docs/proposals/jai-style-sample/Data.jet | 1274:9 | "native path bytes originate from the host environment and are split at ASCII boundaries" |
| Docs | Docs/proposals/jai-style-sample/Data.jet | 1886:5 | "inspect existing private Decode carriers only inside checked Core data source" |
| Examples | Examples/features/crypto/crypto_migration.jet | 33:5 | "AES-256-GCM protocol interoperability" |
| Examples | Examples/features/crypto/crypto_suite.jet | 38:5 | "known-answer check reads the RFC 7748 shared secret" |
| Examples | Examples/features/crypto/expert_projection.jet | 35:5 | "expert cryptography is an explicit opt-in" |
| Examples | Examples/features/crypto/primitive_matrix.jet | 104:5 | "known-answer check reads the RFC 7748 shared secret" |
| Examples | Examples/features/crypto/primitive_matrix.jet | 166:5 | "expert known-answer vectors need raw key bytes" |
| Examples | Examples/features/crypto/random_api_split.jet | 35:5 | "compare the typed and raw HKDF rungs" |
| Examples | Examples/features/crypto/vault_keys.jet | 43:5 | "restore audited raw signing-key material" |
| Examples | Examples/features/effects/audited_gate_ladder.jet | 6:5 | "the audited block has no low-level operation" |
| Examples | Examples/features/effects/single_use_discard.jet | 35:5 | "event cancelled; the ticket admits to nothing, so voiding it is correct" |
| Examples | Examples/features/errors/transact_os_stop.jet | 33:5 | "the stop runs the shared cleanup boundary" |
| Examples | Examples/features/io/cleanup_matrix.jet | 35:5 | "the stop runs the shared cleanup boundary" |
| Examples | Examples/features/io/cleanup_matrix.jet | 80:5 | "the exit callbacks have no captured state" |
| Examples | Examples/features/io/os_process_control.jet | 31:5 | "POSIX process and pipe control for the core.sys surface" |
| Examples | Examples/features/io/os_stop_cleanup.jet | 26:5 | "the stop callback uses the shared cleanup boundary" |
| Examples | Examples/features/io/process_exit_cleanup.jet | 22:5 | "the exit callback has no captured state" |
| Examples | Examples/features/io/system_privileged_entry.jet | 10:5 | "run only inside a disposable user namespace with a dedicated test identity" |
| Examples | Examples/features/io/system_projection.jet | 21:5 | "the clock file was created above in this test-owned directory" |
| Examples | Examples/features/io/system_projection.jet | 27:5 | "fork and reap both children before returning" |
| Examples | Examples/features/lowlevel/inline_asm.jet | 8:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Examples | Examples/features/lowlevel/inline_asm.jet | 19:5 | "call the audited register-only assembly contract" |
| Examples | Examples/features/lowlevel/inline_c.jet | 6:1 | "the scalar ABI contract matches the C definition" |
| Examples | Examples/features/lowlevel/inline_c.jet | 13:1 | "the scalar ABI contract deliberately returns a mismatched result" |
| Examples | Examples/features/lowlevel/inline_c.jet | 19:5 | "call the audited inline C contract" |
| Examples | Examples/features/lowlevel/inline_c.jet | 26:9 | "call the audited inline C contract with bounded Int values" |
| Examples | Examples/features/lowlevel/inline_c.jet | 32:9 | "call the audited inline C contract with a deliberately wrong C result" |
| Examples | Examples/features/lowlevel/lowlevel.jet | 8:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| Examples | Examples/features/lowlevel/mem_from_addr_direct.jet | 11:5 | "the borrowed value is live in the caller and the pointer never escapes" |
| Examples | Examples/features/lowlevel/mmio_board_write.jet | 9:5 | "stand-in MMIO cell stays on this stack frame" |
| Examples | Examples/features/lowlevel/opaque_handles/run.jet | 36:5 | "fixed headers define handle close ownership, exclusive buffers, and bounded output lengths" |
| Examples | Examples/features/lowlevel/pointer_cast_deref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
| Examples | Examples/features/lowlevel/unsafe_obligations.jet | 9:5 | "cell stays live and the pointer remains local" |
| Examples | Examples/features/memory/expiring_secret.jet | 22:5 | "vault key import: expiring secret lifetime" |
| Examples | Examples/features/memory/pin.jet | 43:5 | "node storage is fixed for the returned pin; self_addr names this place" |
| Examples | Examples/features/memory/rawptr.jet | 7:5 | "`cell` is a live Int on this stack frame; the pointer never escapes" |
| Examples | Examples/features/memory/uninit.jet | 12:5 | "uninitialized plain-data storage is filled before every read" |
| Examples | Examples/features/memory/unsafe_sentries.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| Examples | Examples/features/memory/unsafe_sentries_package_off/run.jet | 7:5 | "the local cell is live for this package-policy proof" |
| Examples | Examples/features/memory/unsafe_sentries_provenance.jet | 6:5 | "external address must be a live allocation" |
| Examples | Examples/features/memory/unsafe_sentries_source_off.jet | 7:5 | "the local cell is live for this raw read" |
| Examples | Examples/features/text/checked_text_runtime_hole.jet | 24:5 | "checked text example output" |
| Examples | Examples/features/text/checked_text_runtime_hole_stop.jet | 23:5 | "checked text example output" |
| Examples | Examples/features/tooling/checked_text_type.jet | 20:5 | "checked text example output" |
| Jetpack | Jetpack/Foundation/Source/TreeHash.jet | 569:9 | "the authority retains the held directory until the process provider duplicates its descriptor; the child descriptor slot is supplied by the compiler" |
| Jetpack | Jetpack/Foundation/Source/TreeHash.jet | 745:5 | "source-store linkat uses the live held reader and directory, a validated plain name, and no source pathname; raw syscall facts are classified here" |
| Jetpack | Jetpack/Provider/Batteries/embedded/embedded.jet | 181:9 | "host replay uses a stack cell for the CMSDK UART0 control register and never dereferences the target address" |
| Jetpack | Jetpack/Publication/Source/Sign.jet | 67:5 | "export a fresh Core signing seed only to descriptor-relative owner-only key publication; the Core key owner is wiped on every outcome" |
| Jetpack | Jetpack/Publication/Source/Sign.jet | 122:5 | "import the protected seed into Core signing without a second raw-secret copy; close the temporary Core Secret on success and failure" |
| Jetpack | Jetpack/Publication/Source/Sign.jet | 146:5 | "package author signatures require Core's strict Ed25519 verification; no raw verifier or helper process" |
| Jetpack | Jetpack/Publication/Tests/Index.jet | 291:5 | "the test owns the planted lock file it ages" |
| Jetpack | Jetpack/Store/Tests/ClosureGraph.jet | 59:13 | "the test owns the journal file it just wrote" |
| Jetpack | Jetpack/Store/Tests/Seal.jet | 31:5 | "the store test owns the fixture file whose mtime it pins" |
| Tools | Tools/agent-eval/domain-foundations/area-embedded/pkg/mmio_fixed.jet | 4:5 | "board.sensor_v1 GPIO register 0x40000100 is a volatile MMIO word" |
| Tools | Tools/agent-eval/domain-foundations/area-embedded/pkg/run.jet | 111:5 | "host replay uses a stack cell as a stand-in for the board register" |
| Tools | Tools/agent/cache/Lanes.jet | 305:21 | "the guard signals only compiler, build and test jobs of this user, chosen by name from /proc" |
| Tools | Tools/ci/CompiledWorkloadPeerLauncher.jet | 589:9 | "posix kill: every process in the sandbox's private PID namespace except its init and this supervisor belongs to the supervised command" |
| Tools | Tools/ci/CompiledWorkloadPeerLauncher.jet | 601:5 | "libc signal + posix kill: restore SIG_DFL for one signal number, then re-raise it on this process so the caller observes the supervised command's signal" |
| Tools | Tools/ci/CompiledWorkloadPeerProbe.jet | 98:5 | "posix kill: the probe ends itself with SIGTERM so the check observes the launcher propagating a signal death" |
| Tools | Tools/packaging/NativeCCompiler.jet | 120:9 | "re-raise the compiler's terminating signal on this process, as the parent shell expects" |
| tests | tests/cases/cffi/cffi_raw_status_out_pointer_reads_only_on_success.fixture/main.jet | 16:5 | "store_load receives a live non-null slot; bytes are read only after status zero" |
| tests | tests/cases/cffi/concurrent_bridges_share_one_target.fixture/take0/main.jet | 7:5 | "the foreign call may write through the borrowed value" |
| tests | tests/cases/cffi/ffi_capability_boundary_is_safe_only_inside_unsafe/input/o2/run.jet | 7:5 | "the foreign call may write through the borrowed value" |
| tests | tests/cases/cffi/ffi_capability_boundary_is_safe_only_inside_unsafe/input/o3/run.jet | 7:5 | "the foreign call may write through the borrowed value" |
| tests | tests/cases/cffi_native_matrix/fixtures/required_native_c_abi_matrix/default/matrix.jet | 46:5 | "live non-null out slot; read only after status zero" |
| tests | tests/cases/cffi_native_matrix/fixtures/required_native_c_abi_matrix/sysv64/matrix.jet | 46:5 | "live non-null out slot; read only after status zero" |
| tests | tests/cases/cli_parts_core/fixtures/gate_policy/main.jet | 2:5 | "org refusal" |
| tests | tests/cases/cli_parts_inspect/fixtures/foreign/app/main.jet | 3:5 | "calls the audited dependency" |
| tests | tests/cases/cli_parts_inspect/fixtures/foreign/dep/dep.jet | 2:1 | "intentionally invalid address" |
| tests | tests/cases/cli_parts_inspect/fixtures/release/hardened/main.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| tests | tests/cases/cli_parts_inspect/fixtures/release/release/main.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| tests | tests/cases/cli_parts_inspect/fixtures/team/unsafe/main.jet | 2:5 | "audit" |
| tests | tests/cases/cli_parts_inspect/fixtures/test_hardened/hardened/main.jet | 4:5 | "test deliberately reads after arena reset" |
| tests | tests/cases/cli_parts_inspect/fixtures/test_hardened/release/main.jet | 4:5 | "test deliberately reads after arena reset" |
| tests | tests/cases/collections_and_methods/sentry_typed_int_store_keeps_exact_carrier.fixture/main.jet | 6:5 | "the typed pointer refers to the live local cell for these stores" |
| tests | tests/cases/comptime_cli_parity/comptime_child_is_reaped_before_the_program_runs.fixture/run.jet | 15:5 | "fork and reap the child before returning" |
| tests | tests/cases/comptime_cli_parity/fixtures/tier_parity_examples_run_through_aot_jit_and_interpreter/tooling_checked_text_type/checked_text_type.jet | 20:5 | "checked text example output" |
| tests | tests/cases/comptime_core_pure_parity/crypto_expert_aead_sign_argon_aot_stdout_matches_known_answers.fixture/main.jet | 8:5 | "fixed RFC interop vectors" |
| tests | tests/cases/comptime_core_pure_parity/crypto_values_match_aot_comptime_forced_interpreter_and_default_dev/input/o1/run.jet | 11:5 | "fixed RFC interop vectors" |
| tests | tests/cases/comptime_core_pure_parity/crypto_values_match_aot_comptime_forced_interpreter_and_default_dev/input/o2/run.jet | 11:5 | "fixed RFC interop vectors" |
| tests | tests/cases/control_and_data/explicit_process_exit_cleanup_golden_matches_all_execution_tiers.fixture/process_exit_cleanup.jet | 22:5 | "the exit callback has no captured state" |
| tests | tests/cases/core_crypto_argon2id/argon2id_parallel_lanes_match_rfc_vector_across_tiers/input/run.jet | 9:5 | "fixed Argon2id parallel-lane known-answer vector" |
| tests | tests/cases/custom/fixtures/from_addr_sentry_is_tier_parity/fixture.jet | 6:5 | "value is live on this stack frame and the pointer never escapes" |
| tests | tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_alignment/unsafe_sentries_alignment.jet | 4:5 | "the tuple storage is live but this raw Int is misaligned" |
| tests | tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_expired_recursive_stack/unsafe_sentries_expired_recursive_stack.jet | 11:5 | "the recursive address outlives its owning Jet frame" |
| tests | tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_expired_stack/unsafe_sentries_expired_stack.jet | 8:5 | "the returned address outlives its owning Jet frame" |
| tests | tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_misaligned_list/unsafe_sentries_misaligned_list.jet | 5:5 | "the list storage is live but this raw Int is misaligned" |
| tests | tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_provenance/unsafe_sentries_provenance.jet | 6:5 | "external address must be a live allocation" |
| tests | tests/cases/custom/fixtures/sentry_faults_are_tier_parity/unsafe_sentries_quarantine/unsafe_sentries_quarantine.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| tests | tests/cases/custom/fixtures/sentry_source_policy_off_is_tier_parity/fixture.jet | 7:5 | "the local cell is live for this raw read" |
| tests | tests/cases/dev_parts_routine/jit_1216_adversarial_regressions.fixture/raw_alias/run.jet | 5:5 | "the pointer stays inside this stack frame" |
| tests | tests/cases/dev_parts_routine/unsafe_blocks_are_evaluated_with_live_sentries.fixture/fault.jet | 3:5 | "mapped address is valid and aligned" |
| tests | tests/cases/diagnostics_coverage/sentry_metadata_uses_the_returned_fault_exit_code.fixture/app/main.jet | 3:5 | "calls the audited dependency" |
| tests | tests/cases/diagnostics_coverage/sentry_metadata_uses_the_returned_fault_exit_code.fixture/dep/dep.jet | 2:1 | "intentionally invalid address" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/argon2id_rfc_and_expert_known_answers_kill_argon2i_mutation.fixture/run.jet | 39:5 | "original public expert Argon2id KAT" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/expert_aes256gcm_matches_nist_cavs_vector.fixture/run.jet | 39:5 | "original published known-answer vector" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/expert_surface_matches_reference_vectors_and_bounds.fixture/run.jet | 44:5 | "original expert interoperability vectors" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/expert_xchacha20poly1305_matches_cfrg_vector.fixture/run.jet | 39:5 | "original published known-answer vector" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/jetc_v1_expert_open_accepts_only_the_pinned_grammar.fixture/run.jet | 42:5 | "original pinned JETC v1 migration grammar" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/jetc_v1_migration_reseals_to_verified_v2_without_touching_source.fixture/run.jet | 23:5 | "crypto expert: the fixture seals a pinned JETC v1 envelope and migrates it" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/keygen_zeroizes_provider_copy_and_stack_seed.fixture/run.jet | 41:5 | "crypto expert: the test reads the generated seed to prove it is live and 32 bytes" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/prior_crypto_suite_still_uses_live_entropy.fixture/run.jet | 44:5 | "crypto expert: the test reads the generated seed length" |
| tests | tests/cases/fixtures_crypto_bridge_entropy/typed_secret_wrap_sign_kdf_and_password_paths_preserve_roles.fixture/run.jet | 51:5 | "crypto expert: the test derives from the secret's raw bytes" |
| tests | tests/cases/fmt/fmt_preserves_checked_text_type/input/run.jet | 19:5 | "formatter fixture" |
| tests | tests/cases/fmt/fmt_preserves_unsafe_site_modes_and_postfix_obligations/input/run.jet | 3:1 | "caller keeps address live" |
| tests | tests/cases/fmt/fmt_preserves_unsafe_site_modes_and_postfix_obligations/input/run.jet | 13:5 | "calling audited reader" |
| tests | tests/cases/fmt/fmt_unsafe_reasons_escape_strings/input/run.jet | 3:1 | "caller says \"ok\"" |
| tests | tests/cases/fmt/fmt_unsafe_reasons_escape_strings/input/run.jet | 8:5 | "path C:\\tmp" |
| tests | tests/cases/fmt_project/raw_c_imports_and_module_bindings_remain_clean_through_cli.fixture/runtime.jet | 7:1 | "audited C boundary" |
| tests | tests/cases/fmt_project/raw_c_imports_and_module_bindings_remain_clean_through_cli.fixture/runtime.jet | 10:1 | "audited C boundary" |
| tests | tests/cases/fmt_round_trip_regressions/raw_c_imports_and_module_binding_mutability_are_byte_exact_and_idempotent/input/run.jet | 7:1 | "audited C boundary" |
| tests | tests/cases/fmt_round_trip_regressions/raw_c_imports_and_module_binding_mutability_are_byte_exact_and_idempotent/input/run.jet | 10:1 | "audited C boundary" |
| tests | tests/cases/gate_ledger/fixtures/gates/gates.jet | 2:5 | "test unsafe reason" |
| tests | tests/cases/liveness_lints/fallback_err_is_never_an_unused_local.fixture/main.jet | 3:1 | "probe" |
| tests | tests/cases/liveness_lints/fallback_err_is_never_an_unused_local.fixture/main.jet | 8:5 | "probe" |
| tests | tests/cases/memory_semantics/closure_boxes_free_at_their_last_owner.fixture/main.jet | 318:5 | "the exit callback only reads its captured List" |
| tests | tests/cases/memory_semantics/unsafe_address_keeps_string_alive_to_block_end.fixture/main.jet | 9:5 | "label is live on this frame until this block ends" |
| tests | tests/cases/observe/panic_context_uses_only_lexically_live_locals.fixture/unsafe/panic_lexical_unsafe.jet | 3:5 | "the region only binds and prints a local; it reads and writes no raw memory" |
| tests | tests/cases/output_callable/runnable_contracts_and_selection_fail_in_sema.fixture/probe3/main.jet | 2:1 | "raw boundary" |
| tests | tests/cases/pin/fixtures/library_wires_self_ref_under_unsafe_and_exposes_safe_pin_api/fixture.jet | 10:5 | "node storage is fixed for the returned pin; self_addr names this place" |
| tests | tests/cases/place_window_lifetimes/borrowed_window_stays_caller_owned.fixture/main.jet | 5:5 | "the borrowed value is live in the caller" |
| tests | tests/cases/place_window_lifetimes/heap_element_window_stays_allocator_owned.fixture/main.jet | 6:5 | "the list element remains heap-owned" |
| tests | tests/cases/place_window_lifetimes/stack_window_uses_expiring_sentry_address.fixture/main.jet | 6:5 | "cell is live on this stack frame" |
| tests | tests/cases/policy_scope/unsafe_obligations_are_checked_before_codegen.fixture/run.jet | 5:5 | "local pointer" |
| tests | tests/cases/process_sessions/subjects/native/session_fixture.jet | 53:5 | "the fixture's descendant rejoins the process group of the tree that spawned it" |
| tests | tests/cases/process_sessions/subjects/native/session_fixture.jet | 95:9 | "kill -0 only asks whether the killed descendant still exists" |
| tests | tests/cases/run_entry/module_write_from_fn_is_e0111.fixture/ok.jet | 8:5 | "this program starts no task, so nothing else writes `raw`" |
| tests | tests/cases/single_use/drop_erases_no_unsafe_in_codegen.fixture/source-1.jet | 14:5 | "gone" |
| tests | tests/cases/single_use/drop_inside_unsafe_block_satisfies_single_use.fixture/source-1.jet | 15:5 | "the resource is already gone; nothing to release" |
| tests | tests/cases/single_use/drop_inside_unsafe_fn_satisfies_single_use.fixture/source-1.jet | 13:1 | "voids a freshly-acquired lock whose resource is already gone" |
| tests | tests/cases/single_use/drop_inside_unsafe_fn_satisfies_single_use.fixture/source-1.jet | 19:5 | "calling the audited voider" |
| tests | tests/cases/single_use/reuse_after_drop_is_e0121.fixture/source-1.jet | 15:5 | "done with it" |
| tests | tests/cases/unsafe_and_runtime/every_unsafe_region_kind_enters_its_own_sentry_scope.fixture/regions.jet | 1:1 | "the cell is live on this frame" |
| tests | tests/cases/unsafe_and_runtime/every_unsafe_region_kind_enters_its_own_sentry_scope.fixture/regions.jet | 9:5 | "call the gated reader" |
| tests | tests/cases/unsafe_and_runtime/every_unsafe_region_kind_enters_its_own_sentry_scope.fixture/regions.jet | 15:5 | "test region calls the gated reader" |
| tests | tests/cases/unsafe_and_runtime/fixtures/ffi_capability_edge_uses_shared_sentry_prelude/main.jet | 7:5 | "the foreign call may write through the borrowed value" |
| tests | tests/cases/unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/paren/main.jet | 2:1 | "reads through a raw pointer; addr must be a live, valid Int" |
| tests | tests/cases/unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/paren/main.jet | 10:5 | "addr is the address of `cell`, a live Int on this stack frame" |
| tests | tests/cases/unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/plain/main.jet | 2:1 | "reads through a raw pointer; addr must be a live, valid Int" |
| tests | tests/cases/unsafe_and_runtime/fixtures/mem_address_of_never_folds_plain_or_parenthesized/plain/main.jet | 10:5 | "addr is the address of `cell`, a live Int on this stack frame" |
| tests | tests/cases/unsafe_and_runtime/fixtures/sentry_kernel_is_shared_by_aot_and_runtime_adapters/main.jet | 7:5 | "the local cell is live for this raw read" |
| tests | tests/cases/unsafe_and_runtime/fixtures/unsafe_fn_block_and_ptr_ops/main.jet | 2:1 | "reads through a raw pointer; addr must be a live, valid Int" |
| tests | tests/cases/unsafe_and_runtime/fixtures/unsafe_fn_block_and_ptr_ops/main.jet | 10:5 | "addr is the address of `cell`, a live Int on this stack frame" |
| tests | tests/cases/unsafe_and_runtime/fixtures/unsafe_tier_emit_is_byte_exact/main.jet | 2:1 | "reads through a raw pointer; addr must be valid" |
| tests | tests/cases/unsafe_and_runtime/fixtures/unsafe_tier_emit_is_byte_exact/main.jet | 10:5 | "safe: cell is live" |
| tests | tests/cases/unsafe_and_runtime/fixtures/volatile_write_emit_is_byte_exact/main.jet | 2:1 | "UART TX register is mapped by the target machine" |
| tests | tests/cases/web_build/module_local_dev_is_validated_as_web_runtime.fixture/main.jet | 4:9 | "exercise the web gate" |
| tests | tests/conformance/corpus/core/crypto/aes/nist_gcm.jet | 13:5 | "published KAT or frozen seed AES-GCM fixture" |
| tests | tests/conformance/corpus/core/crypto/aes/seed_differential.jet | 13:5 | "published KAT or frozen seed AES-GCM fixture" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 15:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 19:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 32:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 36:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 49:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 53:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 66:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 70:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 83:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 87:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 98:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 109:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 120:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 131:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 142:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 153:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 164:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 175:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 186:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 197:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 208:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 219:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 230:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 241:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 252:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 263:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 274:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 285:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 296:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 307:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 323:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 334:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 350:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 361:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 377:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 388:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 404:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 415:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 431:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 442:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 458:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 469:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 485:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 496:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 512:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 523:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 539:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 550:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 566:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 577:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 593:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 604:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 620:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 631:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 647:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 658:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 674:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 685:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 701:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 712:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 728:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 739:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 755:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 766:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 782:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 793:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 809:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 820:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 836:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 847:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 863:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 874:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 890:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 901:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 917:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 928:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 944:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 955:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 971:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 982:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 998:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1009:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1025:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1036:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1052:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1063:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1079:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1090:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1106:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1117:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1133:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1144:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1160:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1171:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1187:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1198:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1214:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1225:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1241:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1252:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1268:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1279:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1295:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1306:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1322:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1333:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1344:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1355:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1366:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1377:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1388:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1399:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1410:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1426:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1442:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1458:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1476:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1480:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1493:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1497:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1510:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1514:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1527:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1531:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1544:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1548:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1561:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1565:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1578:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1582:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1595:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1599:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1612:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1616:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1629:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1633:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1646:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1650:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1663:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1667:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1680:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1684:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1697:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1701:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1714:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1718:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1731:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1735:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1748:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1752:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1765:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1769:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1782:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1786:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1799:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1803:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1816:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1820:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1833:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1837:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1850:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1854:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1867:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1871:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1884:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1888:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1901:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1905:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1918:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1922:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1935:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1939:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1952:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1956:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1969:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1973:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1986:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 1990:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2003:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2007:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2020:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2024:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2037:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2041:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2054:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2058:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2071:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2075:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2088:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2092:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2105:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2109:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2122:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2126:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2139:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2143:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2156:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2160:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2173:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2177:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2190:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2194:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2207:9 | "frozen Ed25519 sign witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2211:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2222:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_ed25519.jet | 2238:9 | "public Ed25519 acceptance witness" |
| tests | tests/conformance/corpus/core/crypto/ct_x25519.jet | 5:5 | "published/frozen X25519 test inputs" |
| tests | tests/conformance/corpus/core/crypto/expert/aes256gcm_open.jet | 8:5 | "fixed NIST AES-256-GCM vector" |
| tests | tests/conformance/corpus/core/crypto/expert/aes256gcm_seal.jet | 7:5 | "fixed NIST AES-256-GCM vector" |
| tests | tests/conformance/corpus/core/crypto/expert/argon2id.jet | 10:5 | "fixed Argon2id known-answer vector" |
| tests | tests/conformance/corpus/core/crypto/expert/ed25519_sign.jet | 6:5 | "fixed RFC 8032 Ed25519 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/ed25519_verify_strict.jet | 7:5 | "fixed RFC 8032 Ed25519 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/hkdf_sha256_raw.jet | 8:5 | "fixed RFC 5869 HKDF vector" |
| tests | tests/conformance/corpus/core/crypto/expert/migrate_v1.jet | 12:5 | "fixed JETC v1 source failure" |
| tests | tests/conformance/corpus/core/crypto/expert/open_v1.jet | 7:5 | "fixed JETC v1 AES vector" |
| tests | tests/conformance/corpus/core/crypto/expert/secret_bytes.jet | 9:5 | "inspect fixed secret bytes" |
| tests | tests/conformance/corpus/core/crypto/expert/shared_secret_bytes.jet | 11:5 | "inspect shared-secret length" |
| tests | tests/conformance/corpus/core/crypto/expert/signing_key_bytes.jet | 9:5 | "inspect generated signing-key bytes" |
| tests | tests/conformance/corpus/core/crypto/expert/x25519_raw.jet | 7:5 | "fixed RFC 7748 X25519 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/x25519_secret_bytes.jet | 9:5 | "inspect generated X25519 secret bytes" |
| tests | tests/conformance/corpus/core/crypto/expert/xchacha20poly1305_open.jet | 9:5 | "fixed CFRG XChaCha20-Poly1305 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/xchacha20poly1305_seal.jet | 9:5 | "fixed CFRG XChaCha20-Poly1305 vector" |
| tests | tests/conformance/corpus/core/crypto/vault/commit_import_signing.jet | 5:5 | "restore a fixed signing key into the vault" |
| tests | tests/conformance/corpus/core/crypto/vault/commit_import_x25519.jet | 5:5 | "restore a fixed X25519 secret into the vault" |
| tests | tests/conformance/corpus/core/crypto/vault/prepare_import_signing.jet | 5:5 | "restore a fixed signing-key vector" |
| tests | tests/conformance/corpus/core/crypto/vault/prepare_import_x25519.jet | 5:5 | "restore a fixed X25519 secret vector" |
| tests | tests/conformance/corpus/core/crypto/x25519.jet | 11:5 | "inspect shared-secret length" |
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
| tests | tests/fixtures/explicit_cleanup_order.jet | 33:5 | "exercise the explicit process-stop cleanup boundary" |
| tests | tests/fixtures/ffi_stack4_i64.jet | 2:1 | "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return" |
| tests | tests/fixtures/ffi_stack4_i64.jet | 49:5 | "Linux getpid syscall 39 ignores all argument registers and retains no stack pointer" |
| tests | tests/fixtures/fmt_import_c_bindings.jet | 7:1 | "audited C boundary" |
| tests | tests/fixtures/fmt_import_c_bindings.jet | 10:1 | "audited C boundary" |
| tests | tests/fixtures/native_global_word.jet | 6:5 | "the fixture's one-threaded caller (run, or global-word.c) is the only writer" |
| tests | tests/fixtures/native_inline_c_shared_state.jet | 4:1 | "updates this program's private counter" |
| tests | tests/fixtures/native_inline_c_shared_state.jet | 13:1 | "reads the counter defined by fixture_bump" |
| tests | tests/fixtures/native_inline_c_shared_state.jet | 25:5 | "the counter is private to these two bodies" |
| tests | tests/fixtures/unsafe_obligations/run.jet | 5:5 | "local pointer" |
| tests | tests/fuzz/sema/differential/ex_crypto_crypto_migration.jet | 28:5 | "AES-256-GCM protocol interoperability" |
| tests | tests/fuzz/sema/valid/ex_lowlevel_lowlevel.jet | 10:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| tests | tests/fuzz/sema/valid/ex_lowlevel_pointer_cast_deref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
| tests | tests/run/core_crypto_foundations/crypto_aead_tampering_is_rejected_across_tiers.jet | 16:5 | "fixed AEAD tamper vector" |
| tests | tests/run/patterns_and_fields/fixed_size_list_widens_at_core_call.jet | 5:5 | "fixed signature vector" |
| tests | tests/ui/audited_gate_ladder_forbidden/run.jet | 3:5 | "the organization policy refuses this audited escape" |
| tests | tests/ui/cffi_out_pointer_requires_unsafe.jet | 14:1 | "the local slot remains live for the complete C call" |
| tests | tests/ui/checked_text_raw_requires_unsafe.jet | 22:5 | "audited literal" |
| tests | tests/ui/crypto_argon2id_literal_policy.jet | 9:5 | "fixed password-hash vectors" |
| tests | tests/ui/crypto_argon2id_literal_policy_type_precedence.jet | 9:5 | "fixed password-hash vector" |
| tests | tests/ui/crypto_argon2id_literal_policy_valid.jet | 9:5 | "protocol-selected password policy" |
| tests | tests/ui/crypto_argon2id_literal_policy_valid.jet | 17:5 | "fixed password-hash vectors" |
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
| tests | tests/ui/crypto_expert_nonce_region_authority_precedence.jet | 11:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_trait_dispatch_precedence.jet | 9:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_length.jet | 9:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_valid.jet | 8:5 | "protocol-selected output length" |
| tests | tests/ui/crypto_hkdf_output_valid.jet | 16:5 | "fixed interop vectors" |
| tests | tests/ui/discarded_pure_quiet.jet | 48:5 | "the test deliberately keeps this stderr write behind its audit boundary" |
| tests | tests/ui/e3106_unsafe_skip_without_per_site.jet | 3:5 | "reads a checked constant" |
| tests | tests/ui/ffi_asm_duplicate_clobber.jet | 3:1 | "each discarded register is named once" |
| tests | tests/ui/ffi_asm_float_signature.jet | 3:1 | "register-only increment" |
| tests | tests/ui/ffi_asm_missing_return_anchor.jet | 3:1 | "cycle counter" |
| tests | tests/ui/ffi_asm_no_mem.jet | 1:1 | "register-only increment" |
| tests | tests/ui/ffi_asm_return_clobber.jet | 3:1 | "the result register is overwritten by the body" |
| tests | tests/ui/ffi_asm_unknown_clobber.jet | 3:1 | "the body declares only audited registers" |
| tests | tests/ui/ffi_body_not_string.jet | 1:1 | "demo" |
| tests | tests/ui/ffi_inline_asm_call_needs_unsafe_region.jet | 3:1 | "the operands are scalar registers and r10 is a declared clobber" |
| tests | tests/ui/ffi_inline_asm_call_needs_unsafe_region.jet | 14:5 | "call the audited register-only assembly contract" |
| tests | tests/ui/ffi_inline_c_call_needs_unsafe_region.jet | 1:1 | "the scalar ABI contract matches the C definition" |
| tests | tests/ui/ffi_inline_c_call_needs_unsafe_region.jet | 7:5 | "call the audited inline C contract" |
| tests | tests/ui/ffi_inline_c_mismatch.jet | 1:1 | "deliberate mismatch fixture" |
| tests | tests/ui/ffi_inline_cpp_call_needs_unsafe_region.jet | 1:1 | "the scalar ABI contract matches the C++ definition" |
| tests | tests/ui/ffi_inline_cpp_call_needs_unsafe_region.jet | 7:5 | "call the audited inline C++ contract" |
| tests | tests/ui/ffi_unknown_language.jet | 1:1 | "demo" |
| tests | tests/ui/gates_load_package_ceiling/run.jet | 2:5 | "audited load-only gate" |
| tests | tests/ui/lowlevel_e3102.jet | 2:5 | "addr is valid" |
| tests | tests/ui/lowlevel_e3103.jet | 3:1 | "reads through caller-provided raw pointer" |
| tests | tests/ui/lowlevel_volatile_write_without_mem.jet | 2:5 | "addr is valid" |
| tests | tests/ui/lowlevel_volatile_write_wrong_ptr.jet | 4:5 | "plain ints are not raw pointers" |
| tests | tests/ui/lowlevel_volatile_write_wrong_value.jet | 6:5 | "slot is live and pointer never escapes" |
| tests | tests/ui/module_write_from_fn_is_e0111.jet | 11:5 | "this program starts no task, so nothing else writes `raw`" |
| tests | tests/ui/secrets_expiry/expiring_secret_loan_rejects_move_variadic_and_drop_paths_3.jet | 13:9 | "attempt to discard a loan" |
| tests | tests/ui/unsafe_and_runtime/volatile_write_emit_is_byte_exact/run.jet | 2:1 | "UART TX register is mapped by the target machine" |
| tests | tests/ui/unsafe_forbidden/run.jet | 2:5 | "not allowed" |
| tests | tests/ui/unsafe_obligation_missing.jet | 5:5 | "local pointer" |
| tests | tests/ui/unsafe_obligation_no_bleed.jet | 6:5 | "two independent pointer operations" |
| tests | tests/ui/unsafe_per_site/run.jet | 2:5 | "selection required" |
| tests | tests/ui_lint/inline_asm_reads_its_core_mem_gate.jet | 8:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| tests | tests/ui_lint/inline_asm_reads_its_core_mem_gate.jet | 19:5 | "call the audited register-only assembly contract" |
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
  written `#Unsafe("<caller contract>") name: fn …` instead, so every call needs its own
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
  {"file": "Core/Auth/Auth.jet", "function": "verify_paseto", "proof": "The strict Ed25519 verifier only reads `key`, the PAE-encoded message and the 64-byte signature, all owned byte lists sliced and length-checked above; it returns a Bool and exposes no key material."},
  {"file": "Core/Crypto/Crypto.jet", "function": "password_hash_with_salt", "proof": "Password and salt lengths are bounded before Argon2id runs with fixed parameters (m=65536, t=3, p=1, 32-byte output); the derived key stays in a Secret that only feeds the PHC encoding."},
  {"file": "Core/Crypto/Crypto.jet", "function": "password_verify", "proof": "Every PHC field is parsed and bounded (memory, iterations, lanes, salt and digest lengths) before Argon2id runs; the derived key is compared in constant time and never leaves the function."},
  {"file": "Core/Crypto/Jetc.jet", "function": "seal", "proof": "Expert AEAD seal is safe here because the nonce is unique per (key, record): writer (:265-313) seals under a fresh random file key and 16-byte prefix that the coordinator draws for every container (crypto.jet:915-919); each record's nonce = prefix ∥ LE64(index) (:62-65), with the header tag at index 0 (:306) and chunks at index 1.. (:312) strictly increasing (:326), bounded by MAX_RECORDS (:317), and a final record ends the state (:317, :327). Per-recipient wraps use their own HKDF-derived key and nonce (seal_wrap). AAD is a fixed domain per record kind (:73-94). Key / nonce lengths are enforced by expert (expert.jet:273-276)."},
  {"file": "Core/Crypto/Jetc.jet", "function": "open_record", "proof": "The gate (:104-106) calls expert.xchacha20poly1305_open, which bounds key/nonce/cipher/AAD widths and returns plaintext only after the Poly1305 tag verifies in constant time (expert.jet:279-285, 195-197); every failure becomes FileCryptoError.OpenFailed. Its only caller open_key (:144) lends an operation-scoped key copy that it wipes on both outcomes. open_key's callers bound their input first: reader checks the header length, recipient count and stanza layout (:340-346) before the wrap and header-tag opens (:374-377), and open_chunk checks record shape, cipher width and remaining length (:385-389) before a record open (:390); the plaintext becomes a Secret, and the coordinator writes it only after authentication and publishes only after finish sees the final record (crypto.jet:672-677)."},
  {"file": "Core/Crypto/Jetc.jet", "function": "derive_key", "proof": "expert.shared_secret_bytes (expert.jet:351) copies the SharedSecret; the copy only feeds hkdf_sha256 and is zeroized on both the Ok and Err paths before return (:113-123), while the SharedSecret keeps its own zeroizing Close (crypto.jet:285-289). The result is a Secret. jetc.jet is the private part __core_crypto_jetc (CoreSourceParts.rs:144-150), so its callers are Core: writer (:293, :297) and reader (:372-373), which derive the 32-byte wrap key and 24-byte wrap nonce with distinct domain labels over ephemeral ∥ recipient public key and the file id as salt."},
  {"file": "Core/Crypto/Jetc.jet", "function": "seal_key", "proof": "expert.secret_bytes (expert.jet:349) copies the key; the copy only feeds seal (the registered nonce-uniqueness proof above) and is zeroized on both outcomes before return (:128-138); the Secret owner keeps its Close. Its Core callers supply the protocol nonces that proof covers: the header tag at nonce(prefix, 0) (:306), each chunk at nonce(prefix, index ≥ 1) (:321-322), and seal_wrap's per-recipient HKDF nonce under that recipient's own derived key (:160)."},
  {"file": "Core/Crypto/Jetc.jet", "function": "open_key", "proof": "expert.secret_bytes (expert.jet:349) copies the key; the copy only feeds open_record (authenticated open, proof above) and is zeroized on both outcomes before return (:143-153); the Secret owner keeps its Close. Callers are Core only (private part __core_crypto_jetc): reader's header tag (:377), open_wrap (:177) and open_chunk (:390), each after the bounds checks listed for open_record."},
  {"file": "Core/Crypto/Jetc.jet", "function": "seal_wrap", "proof": "Copies the derived wrap nonce and the file key through expert.secret_bytes, passes them to seal_key, and zeroizes both copies on Ok and Err (:157-171); the Secret owners keep their Close duty. Its only caller writer (:301) runs it once per recipient with a key and nonce from derive_key over a fresh ephemeral X25519 key (:270-273) and that recipient's public key, so each derived key seals exactly one message under one nonce. A wrong nonce width is refused by expert (expert.jet:274; test :257)."},
  {"file": "Core/Crypto/Jetc.jet", "function": "open_wrap", "proof": "Copies the derived wrap nonce through expert.secret_bytes, passes it to open_key (authenticated open), and zeroizes the copy on both outcomes (:175-187). Its only caller reader (:374) supplies the stanza bytes after the header-shape checks (:340-346); the opened file key is wrapped in a Secret and must be exactly 32 bytes (:376)."},
  {"file": "Core/Crypto/Jetc.jet", "function": "secret_length", "proof": "The gate (:233-238) copies the Secret through expert.secret_bytes, reads only its length, and zeroizes the copy before returning that Int; no byte of the secret leaves the function and the original owner keeps its Close (#Test :241-246). Callers are writer (:269) and reader (:376) width checks."},
  {"file": "Core/Crypto/Jetc.jet", "function": "writer", "proof": "The gate (:271-273) copies the ephemeral Secret through expert.secret_bytes straight into X25519SecretKey.from_bytes, which takes the copy and accepts exactly 32 bytes (crypto.jet:244-247); :269 already checked that width, so the refusal path is unreachable. The copy then lives only in the zeroizing X25519SecretKey (Close wipes, crypto.jet:254-258) and the moved ephemeral Secret keeps its own Close. Only the derived public key (:274) and per-recipient wraps leave; the coordinator draws the four inputs fresh for each container (crypto.jet:913-919)."},
  {"file": "Core/Crypto/Crypto.jet", "function": "__migrate_v1", "proof": "Reachable only from expert.migrate_v1 (expert.jet:624, the sole caller), which the checker restricts to an importing core.crypto.expert caller inside #Unsafe (CheckerCoreLib/core_call.rs:2553-2577). The legacy buffer is bounded (≤1 GiB + 34, :666-675); the plaintext is a Secret only after authentication (:676-680); the staged v2 output is verified (:919-929) before publication."},
  {"file": "Core/Crypto/Crypto.jet", "function": "__vault_wrapped_from_bytes", "proof": "The gate (:314-317) forwards the moved bytes to vault.__wrapped_from_bytes (vault.jet:182-186), which returns a WrappedVaultKey only after wrapped_metadata (vault.jet:487-) bounds the blob to 16..8192 bytes and checks the JVKW magic and header fields; failures become KeyWrapError. The carrier holds the wrapped (encrypted) container, never plaintext key material. The `__` name is reserved (E0067 outside Core), so users reach it only as the typed constructor WrappedVaultKey.from_bytes (BuiltinStatics.jet:145, 148)."},
  {"file": "Core/Crypto/Crypto.jet", "function": "__vault_wrapped_bytes", "proof": "vault.__wrapped_bytes (vault.jet:187) returns an owned copy of the carrier's `encoded` field: the same authenticated wrapped container that WrappedVaultKey.from_bytes validated or the vault export produced. No unwrapped key or Secret is read, and the carrier stays intact. The `__` name is reserved (E0067 outside Core), so only Core and compiler lowering call it."},
  {"file": "Core/Crypto/Crypto.jet", "function": "__vault_unlock_recipient", "proof": "vault.__key_unlock_recipient (vault.jet:203-205) builds KeyUnlock{kind: 1, secret: identity.bytes[0..<len]}: a read View of the caller's X25519SecretKey bytes, not a copy, so no second copy of the identity exists and the owner keeps its zeroizing Close. The gate performs no raw memory operation; the view's lifetime is ordinary type checking of the view-carrying KeyUnlock (vault.jet:198-201; emitted as the borrowing JetVaultKeyUnlock<'_>, Emit/Types.jet:3201). Its consumer is vault.prepare_import_wrapped (vault.jet:275). Users reach it only as KeyUnlock.Recipient(identity) (BuiltinStatics.jet:146, 149)."},
  {"file": "Core/Crypto/Crypto.jet", "function": "__vault_unlock_passphrase", "proof": "vault.__key_unlock_passphrase (vault.jet:206-208) builds KeyUnlock{kind: 2, secret: passphrase.bytes[0..<len]}: a read View of the caller's Secret, not a copy, so the passphrase owner keeps its zeroizing Close. The gate performs no raw memory operation; the view's lifetime is ordinary type checking of the view-carrying KeyUnlock (vault.jet:198-201; emitted as the borrowing JetVaultKeyUnlock<'_>, Emit/Types.jet:3201). Its consumer is vault.prepare_import_wrapped (vault.jet:275). Users reach it only as KeyUnlock.Passphrase(secret) (BuiltinStatics.jet:147, 150)."},
  {"file": "Core/Crypto/Crypto.jet", "function": "poly1305", "proof": "Not a core.crypto export (Core.jet:44), so its callers are crypto.jet's chacha_poly_seal (:2146) and chacha_poly_open (:2165). Both pass otk = chacha20_block(key, nonce, 0) (:2137, :2157), the RFC 8439 §2.6 one-time key, over the AEAD framing aad ∥ pad16 ∥ ct ∥ pad16 ∥ le64(aad) ∥ le64(ct); the 64-byte block covers the 32 key bytes fixedcrypto.poly1305 reads (expert_ct.jet:628). The key is therefore one-time per (key, nonce): xchacha_seal's callers use a random 24-byte nonce for the payload (:466, :506) and HKDF nonces under fresh ephemeral keys for wraps (:486-496, :1161-1174, :1190). open compares tags in constant time before decrypting (:2166)."},
  {"file": "Core/Crypto/Crypto.jet", "function": "x25519_scalarmult", "proof": "Not a core.crypto export (Core.jet:44). Its callers are x25519 (:1012), which first requires 32-byte secret and public keys (:1009-1010) and rejects an all-zero shared secret (:1013), x25519_public (:1019) after a 32-byte check (:1018), and X25519SecretKey.public_key (:251) with the fixed 32-byte base point. fixedcrypto.x25519 panics on any other width rather than reading past its inputs (expert_ct.jet:150), wipes its workspace (:201), and returns a fresh 32-byte list."},
  {"file": "Core/Crypto/Crypto.jet", "function": "ed25519_sign_raw", "proof": "Not a core.crypto export (Core.jet:44). Its only caller sign (:998) requires a 32-byte SigningKey seed first (:997). fixedcrypto.ed_sign hashes the seed into a private workspace, wipes it (expert_ct.jet:355-385), and returns the 64-byte signature; the seed is never copied out."},
  {"file": "Core/Crypto/Crypto.jet", "function": "ed25519_verify_raw", "proof": "Not a core.crypto export (Core.jet:44). Its only caller verify (:1005) requires a 32-byte public key and a 64-byte signature (:1002-1004); fixedcrypto.ed_verify rejects other widths itself (expert_ct.jet:512). Only public inputs are read and only a Bool returns."},
  {"file": "Core/Crypto/ExpertCt.jet", "function": "arena", "proof": "__list_reserve_exact grows the exclusively lent, freshly created empty list to exactly `words` I64 elements of capacity and returns false, leaving the allocation intact, on a negative or overflowing request or no memory (JetRuntime Collections.jet:20-23); arena panics on false (:12). Length stays 0, and the loop then pushes `words` zeros (:14), so every later bounds-checked word/byte access sees initialized elements. Callers pass constants (26, 204, 684, 696). Sema admits this leaf only in the private part __core_crypto_ct (Sema/Calls/Core.jet:962; CoreSourceParts.rs:155-161)."},
  {"file": "Core/Crypto/ExpertCt.jet", "function": "extract", "proof": "__list_reserve_exact reserves exactly `count` U8 capacity on a fresh empty list and the function panics on failure (:37-39); the length grows only through the following `count` pushes of bytes copied from the workspace (:40), so no uninitialized element is ever readable. Callers pass constant widths 32 (:200, :350), 64 (:382) and 16 (:651). Sema admits the leaf only in __core_crypto_ct (Sema/Calls/Core.jet:962)."},
  {"file": "Core/Crypto/ExpertCt.jet", "function": "hash_parts", "proof": "__list_reserve_exact reserves exactly an + bn + msg.len() U8 capacity on a fresh empty list and the function panics on failure (:323-325); exactly that many bytes are then pushed (:326-328), so the length never exceeds what was written. Callers pass an = 32 and bn ∈ {0, 32} (:361, :366, :487). The hashed buffer and digest are zeroized before return (:331-332). Sema admits the leaf only in __core_crypto_ct (Sema/Calls/Core.jet:962)."},
  {"file": "Core/Crypto/Vault.jet", "function": "load", "proof": "`validate_key` rejects a forged or foreign key reference before the typed provider runs; the provider returns the typed key value, never raw key bytes."},
  {"file": "Core/Crypto/Vault.jet", "function": "current", "proof": "`validate_name` rejects an invalid vault name before the typed provider runs; the result is a typed key reference, not key material."},
  {"file": "Core/Crypto/Vault.jet", "function": "status", "proof": "`validate_key` rejects a forged or foreign key reference before the provider runs; the result is a KeyStatus value."},
  {"file": "Core/Crypto/Vault.jet", "function": "versions", "proof": "`validate_name` rejects an invalid vault name before the provider runs; the result is a list of typed key references."},
  {"file": "Core/Crypto/Vault.jet", "function": "prepare_generate", "proof": "`validate_name` runs first; the provider returns a MutationPlan that only `authorize_write` and a commit can act on."},
  {"file": "Core/Crypto/Vault.jet", "function": "prepare_store", "proof": "`validate_name` runs first and the key moves into the provider (`key: &&T`), so the caller keeps no copy; the result is a MutationPlan."},
  {"file": "Core/Crypto/Vault.jet", "function": "prepare_import_signing", "proof": "`validate_name` runs and the material must be exactly 32 bytes; the bytes move into the provider (`bytes: &&[U8]`), so no raw copy remains with the caller."},
  {"file": "Core/Crypto/Vault.jet", "function": "prepare_import_x25519", "proof": "`validate_name` runs and the material must be exactly 32 bytes; the bytes move into the provider (`bytes: &&[U8]`), so no raw copy remains with the caller."},
  {"file": "Core/Crypto/Vault.jet", "function": "prepare_import_wrapped", "proof": "`validate_name` runs and the wrapped blob is bounded to 16..8192 bytes before the provider decrypts it under `unlock`; the result is a WrappedImportPlan."},
  {"file": "Core/Crypto/Vault.jet", "function": "prepare_rotate", "proof": "`validate_name` runs first; the provider returns a MutationPlan that only `authorize_write` and a commit can act on."},
  {"file": "Core/Crypto/Vault.jet", "function": "prepare_retire", "proof": "`validate_key` and `validate_reason` run first; the provider returns a MutationPlan, not key material."},
  {"file": "Core/Crypto/Vault.jet", "function": "prepare_revoke", "proof": "`validate_key` and `validate_reason` run first; the provider returns a MutationPlan, not key material."},
  {"file": "Core/Crypto/Vault.jet", "function": "authorize_write", "proof": "`validate_plan` and `validate_reason` run first; the plan is lent read-only and the result is a single-use VaultWrite token."},
  {"file": "Core/Crypto/Vault.jet", "function": "authorize_wrapped_import", "proof": "`validate_wrapped_plan` and `validate_reason` run first; the result is a single-use VaultWrite token."},
  {"file": "Core/Crypto/Vault.jet", "function": "commit_generate", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/Crypto/Vault.jet", "function": "commit_store", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/Crypto/Vault.jet", "function": "commit_import_signing", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/Crypto/Vault.jet", "function": "commit_import_x25519", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/Crypto/Vault.jet", "function": "commit_import_wrapped", "proof": "The write token and wrapped plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/Crypto/Vault.jet", "function": "commit_rotate", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a Rotation of typed key references."},
  {"file": "Core/Crypto/Vault.jet", "function": "commit_retire", "proof": "The write token and plan are validated and both move into the provider, so each is used once; nothing is returned but the error."},
  {"file": "Core/Crypto/Vault.jet", "function": "commit_revoke", "proof": "The write token and plan are validated and both move into the provider, so each is used once; nothing is returned but the error."},
  {"file": "Core/Crypto/Vault.jet", "function": "export_to_passphrase", "proof": "`validate_key` runs first and the passphrase is lent read-only; the result is passphrase-encrypted wrapped bytes, never the plain key."},
  {"file": "Core/Crypto/Vault.jet", "function": "export_to_recipients", "proof": "`validate_key` runs and the recipient list is bounded to 1..16; the result is recipient-encrypted wrapped bytes, never the plain key."},
  {"file":"Core/Data/Data.jet","function":"bridge_tool_on_path","proof":"__host_path_is_file invokes OsStr::from_encoded_bytes_unchecked (Top/DataStats.rs:11-13), so its input must retain native encoded-byte provenance. data.jet:1090 obtains PATH from the current host's encoded environment; :1092-1108 split only at ASCII colon/semicolon and remove Windows ASCII quotes. :1111-1117 append only ASCII separators and valid UTF-8 tool text, preserving that host encoding. The metadata leaf borrows the owned list and returns Bool/error, not a pointer; the caller cannot supply an unrelated byte allocation to this helper or retain a host borrow."},
  {"file":"Core/Data/Data.jet","function":"data_loader_render","proof":"The gate at data.jet:1729 only matches the compiler-private Dyn Number, TypedText and Bytes variants. Each payload is accessed after its matching discriminant at :1730, :1734 or :1738, then read through ordinary String/List operations into the caller's exclusively borrowed output; no unchecked memory, forged carrier or raw pointer is created. Other variants leave the gate for normal recursive rendering. Any typed Dyn input remains a valid enum; callers cannot select a mismatched native payload."},
  {"file":"Core/Data/Data.jet","function":"data_schema_value_type","proof":"Not a core.data export (Compiler/Prelude/Core.jet:69). Its #Unsafe block (Data.jet:3674-3677) only pattern-matches the compiler-private Dyn carriers .Number and .Bytes to name their schema type; it reads no memory, builds or mutates no carrier and returns a static String, so no caller can break those carriers' invariants through it. Every other Dyn shape is matched through the public variants (:3678-3688)."},
  {"file":"Core/Process/Process.jet","function":"cmd","proof":"The __spec_fields leaf is layout transfer into ProcessSpec, not process execution (Top/ProcessSpec.rs:6-37). process.jet:92 takes all fields from default_fields(:419-428): paired environment columns are both empty; optional cwd, stdin, timeout, resource limits, terminal and policy are accompanied by their actual presence flags at :95-104, with valid typed placeholders. The sole present numeric limit is 67108864. argv is a typed owned String list; even empty argv is deferred to checked spawn, not indexed here. No descriptor or memory address is supplied or returned, and callers cannot alter the hidden initialized field set."},
{"file":"Core/Db/Db.jet","function":"close","proof":"SqliteDB.close and SqliteStmt.close (Db.jet:1466-1478) consume their Close resource, so each runs at most once per value. Both raw fields are priv (:1464-1465), so no literal outside core.db can forge a pointer; inside core.db the only SqliteDB is built from open_v2's output slot (:1533-1534) and the only SqliteStmt from prepare_v3's (:1605). close_v2 and finalize accept NULL (a failed open's zero, or the zero __driver_shutdown leaves, :1552), and close_v2 defers the free while statements live, so a cached statement finalized later (__Driver.close, :1491-1497) never touches a freed connection."},
{"file":"Core/Db/Db.jet","function":"error","proof":"Its parameter is the reserved __Driver type (a __ name is refused outside Core), so callers are core.db and core.jobs. Every driver comes from __driver_open, which returns only a successfully opened non-NULL handle (Db.jet:1522-1539); DBConnection paths check raw != 0 before any driver call (:695, :702, :711, :745, :756), and pool and queue drivers are never shut down (only :719 calls __driver_shutdown). sqlite3_errmsg returns a NUL-terminated buffer the connection owns; c_string copies it with strlen and unpack (:1504-1507) before any other engine call, and only the owned String escapes."},
{"file":"Core/Db/Db.jet","function":"__driver_open","proof":"Takes only a String path; a path with a NUL byte returns None first (Db.jet:1523), so open_v2 receives the complete C string. The single-thread probe frees the mutex it allocates (:1529-1531). open_v2 writes its handle into a stack slot of this frame (:1524, :1533), which is wrapped in the Close-owning SqliteDB before the status check (:1534), so the early returns (:1535, :1537) close a failed handle. The only escaping value is a __Driver owning that live pointer with an empty cache (:1538); no raw address is returned."},
{"file":"Core/Db/Db.jet","function":"__driver_shutdown","proof":"Its only caller lends the DBConnection's driver exclusively under the state guard (Db.jet:718-719). A zero raw returns false before any engine call (:1544). Cached statements are finalized through their consuming Close first (:1545-1548), close_v2 runs once on the live pointer (:1549-1551) and the field is zeroed immediately after (:1552), so the driver's later Close hands close_v2 NULL, a no-op, and every DBConnection operation sees raw == 0 and refuses (:695, :702, :711, :745, :756)."},
{"file":"Core/Db/Db.jet","function":"control","proof":"Reachable only with the reserved __Driver type, whose handle every caller keeps live (see error: __driver_open :1522-1539, raw checks :695, :702, :711; queue drivers in Jobs.jet are never shut down). Its callers __driver_begin, __driver_begin_immediate, __driver_commit and __driver_rollback (:1560-1563) pass constant SQL, which the C String adapter lends NUL-terminated for the call, and exec receives NULL callback, context and error-output pointers (:1557), so SQLite allocates nothing for the caller to free. Only a Bool returns."},
{"file":"Core/Db/Db.jet","function":"__driver_reset","proof":"Called on a pool driver being returned (Db.jet:1213); pool drivers come from __driver_open (:1198, :1270) and are never shut down, so raw is a live connection. get_autocommit only reads that connection's flag (:1566); otherwise the fixed ROLLBACK runs through control (:1568). Only a Bool returns and no pointer escapes."},
{"file":"Core/Db/Db.jet","function":"prepare","proof":"Takes an exclusive &__Driver whose handle is live (see error). Cache hits return an owned CachedStatement removed from the cache (Db.jet:1574-1586). On a miss, key is an owned trimmed String that stays alive and unmodified while prepare_v3 borrows its data with an explicit byte length checked below 2147483647 (:1590-1591, :1602); empty text lends a packed one-byte buffer freed right after the call (:1596-1599). The handle and tail output slots belong to this frame (:1587-1588), the new handle is wrapped in the Close-owning SqliteStmt before the status check (:1605), and a failure copies the error text out through error (:1607). No raw address escapes."},
{"file":"Core/Db/Db.jet","function":"release","proof":"Takes the statement by consuming move, so the caller cannot reuse it; a zero handle returns at once (Db.jet:1621). reset and clear_bindings run on that exclusively owned live statement (:1622-1625) before it re-enters the 16-entry cache, which evicts and closes its oldest entry through the consuming Close (:1626-1630). The driver loan only touches the cache list; no pointer escapes."},
{"file":"Core/Db/Db.jet","function":"bind","proof":"Its callers __driver_execute (Db.jet:1679) and __driver_query (:1731) pass statement_raw of the CachedStatement prepare just returned, which they own until release, so the statement is live; the reserved __Driver type keeps other callers out. The parameter count must match first (:1634-1635). Scalars are copied by value (:1640-1643). Text binds the String's data with its byte length, checked below 2147483647 (:1645-1646), as SQLITE_TRANSIENT (-1), so SQLite copies it during the call while the String is alive and unmodified; empty text and blobs bind packed buffers freed only after that TRANSIENT call returns (:1651-1654, :1666-1668). Failures are copied out through error (:1672)."},
{"file":"Core/Db/Db.jet","function":"__driver_execute","proof":"Takes an exclusive &__Driver; DBConnection callers check raw != 0 first (Db.jet:745) and queue drivers (Jobs.jet:144, :942-943) are never shut down. The statement is prepared and owned here until release (:1678, :1697), and a bind failure releases it before returning (:1679-1682), so step, reset and changes run on a live statement and connection (:1686-1690). Error text is copied out through error before release; only an Int count or an owned DBError escapes."},
{"file":"Core/Db/Db.jet","function":"__driver_query","proof":"Takes an exclusive &__Driver; DBConnection callers check raw != 0 first (Db.jet:756) and queue drivers (Jobs.jet:948-949) are never shut down. The statement is prepared and owned here until release (:1730, :1788), and a bind failure releases it first (:1731-1734). Column names are NULL-checked and copied by c_string before the first step (:1745-1749); each cell is copied into an owned DBValue while its row is current, calling column_text or column_blob before column_bytes and unpacking exactly that many bytes (:1762-1776), before the next step or reset. Only owned rows or an owned DBError escape."},
{"file":"Core/Net/Net.jet","function":"ip_addr","proof":"__ip_from_bytes expects exactly four or sixteen octets. net.jet:136 accepts only parse_ip_bytes success: parse_ipv4_bytes(:960-979) checks four decimal octets <=255, and the IPv6 parser(:982-1048) checks eight 16-bit words including compression before expanding to sixteen bytes. The exact owned list moves at :138; callers cannot substitute a differently sized list after validation."},
{"file":"Core/Net/Net.jet","function":"ip_to_string_impl","proof":"net.jet:143-144 call __ip_bytes on a borrowed typed IPAddr; Top/NetHTTP.rs:1944-1948 discriminant-matches its native IPv4/IPv6 enum and copies exactly its initialized octets into an owned list. Formatting only reads that list. No caller-selected index, raw pointer or borrowed native storage escapes."},
{"file":"Core/Net/Net.jet","function":"ip_is_ipv4","proof":"__ip_is_v4 at net.jet:149 only matches the valid native IPAddr discriminant (Top/NetHTTP.rs:1951-1952). Any typed address is admitted; no payload reinterpretation, mutation or memory borrow is returned, only Bool."},
{"file":"Core/Net/Net.jet","function":"socket_addr_parse","proof":"At net.jet:182-189 parsing produces exactly sixteen IPv6 bytes or four IPv4 bytes, with the IPv6 length checked at :183. parse_port(:846-856) bounds decimal port to 0..65535 and parse_scope(:1144-1154) bounds scope to 0..4294967295 before :193-194 consume the octets and IP into __ip_from_bytes/__socket_from_parts. Flow is the constant zero. Callers cannot select uninitialized representation fields or overflow the U16/U32 transfer."},
{"file":"Core/Net/Net.jet","function":"socket_host","proof":"__socket_ip at net.jet:199 borrows a valid SocketAddr and copies its native IP enum (Top/NetHTTP.rs:2038-2039). ip_to_string_impl then reads copied octets for source formatting; the caller receives owned text, not a reference into the socket carrier. No raw address is accepted."},
{"file":"Core/Net/Net.jet","function":"socket_port","proof":"__socket_port at net.jet:202 borrows a typed SocketAddr and widens its initialized native u16 port to Int (Top/NetHTTP.rs:2042-2043). This scalar projection accepts every valid carrier and neither exposes a borrow nor changes socket ownership."},
{"file":"Core/Net/Net.jet","function":"socket_scope","proof":"__socket_scope at net.jet:215 matches the valid socket enum, returning None for IPv4 or widening the IPv6 u32 scope (Top/NetHTTP.rs:2046-2050). Source substitutes zero for None. No reinterpretation or caller-controlled pointer is involved and only an owned scalar escapes."},
{"file":"Core/Net/Net.jet","function":"udp_packet_addr","proof":"__udp_packet_addr at net.jet:413 borrows a typed UDPPacket and clones its stored SocketAddr (Top/NetHTTP.rs:2851-2852); the packet remains live during the projection. The return carrier owns its address rather than pointing into packet storage, so releasing the packet cannot invalidate the result."},
{"file":"Core/Net/Tls.jet","function":"close","proof":"__TLSContext.close (Tls.jet:68-73) consumes the context, so jet_tls_free runs at most once per context. __TLSContext is a reserved __ type, so only Core can build one, and its sole constructor __context_new (:546-555) stores either 0 or the fresh handle jet_tls_new wrote. jet_tls_free returns on 0 (jet_tls.c:61-64) and never closes the socket, which the TCPStream owns."},
{"file":"Core/Net/Tls.jet","function":"__context_new","proof":"Reserved __ name; callers pass the constant endpoint 0 or 1 (Tls.jet:167, :559, :678, :690). The output slot belongs to this frame (:547, :550); jet_tls_new zeroes it first and stores a context only on success, freeing its own partial context on failure (jet_tls.c:80-104). The handle is wrapped in the Close-owning __TLSContext before the status check (:552), so the failure return frees it and success returns that sole owner."},
{"file":"Core/Net/Tls.jet","function":"__write_all","proof":"Reserved __ name; TLSStream.write_all calls it under the state guard (Tls.jet:183-188), and a closed stream is refused before any engine call (:936). Each chunk is a bounds-checked __slice of at most C_RECORD_BYTES (:939-940), packed into a fresh buffer of exactly `size` bytes (:943) that __write_packed lends to jet_tls_write with that length (:964-966), and freed exactly once after its last write on either the error or the success path (:948-950, :954-956)."},
{"file":"Core/Net/Tls.jet","function":"__c_authmode","proof":"Reserved __ name, so its callers are Tls.jet's own: __client_context (:573) and the server accept path (:149) pass the raw of a context __context_new just created and still owns, before setup, with the constant modes 2 and 0. jet_tls_authmode only stores the mode in that live config (jet_tls.c:112-115). Nothing is returned."},
{"file":"Core/Net/Tls.jet","function":"__c_versions","proof":"Reserved __ name; its only caller __client_context (Tls.jet:570) passes the raw of the context it created and owns, before setup, with __version_code values after checking min <= max (:567). I32{} stops with E3010 rather than wrap (core-library.md:4172-4173). jet_tls_versions only stores the bounds in that live config (jet_tls.c:118-121)."},
{"file":"Core/Net/Tls.jet","function":"__c_ca_pem","proof":"Reserved __ name; callers (Tls.jet:604, :679) pass the raw of a context they own and PEM text checked NUL-free first (:603, :677). The C String adapter lends it NUL-terminated for the call; mbedtls_x509_crt_parse copies the certificates into the context's own chain (jet_tls.c:126-131). Only a status Int returns."},
{"file":"Core/Net/Tls.jet","function":"__c_ca_file","proof":"Reserved __ name; callers (Tls.jet:609, :632, :640) pass the raw of a context they own and a path checked NUL-free first (:608, :632) or a constant system path (__system_root_files). The path is lent NUL-terminated for the call and mbedtls_x509_crt_parse_file copies the certificates into the context's chain (jet_tls.c:136-143). Only a status Int returns."},
{"file":"Core/Net/Tls.jet","function":"__c_ca_dir","proof":"Reserved __ name; callers (Tls.jet:634, :642) pass the raw of a context they own and a directory checked NUL-free first (:634) or the constant /etc/ssl/certs. The path is lent NUL-terminated for the call and mbedtls_x509_crt_parse_path copies the certificates into the context's chain (jet_tls.c:146-153). Only a status Int returns."},
{"file":"Core/Net/Tls.jet","function":"__c_ca_count","proof":"Reserved __ name; callers (Tls.jet:600, :613, :639, :642, :645) pass the raw of a context they created and own. jet_tls_ca_count only walks that context's own root list (jet_tls.c:156-160) and returns a count; nothing is borrowed or written."},
{"file":"Core/Net/Tls.jet","function":"__c_own_pem","proof":"Reserved __ name; callers (Tls.jet:169, :564, :691) pass the raw of a context they own and certificate and key PEM checked NUL-free first (:168, :563, :689). Both texts are lent NUL-terminated for the call; Mbed TLS parses copies of the chain and key into the context (jet_tls.c:166-175). Only a status Int returns."},
{"file":"Core/Net/Tls.jet","function":"__c_alpn_add","proof":"Reserved __ name; its only caller __add_alpn (Tls.jet:577-583) passes the raw of a context it owns after __alpn_problem bounds the list to 16 protocols and validates each (:361-364). jet_tls_alpn_add refuses a full table and mallocs its own copy of the protocol before registering it (jet_tls.c:184-197), so the borrowed String is not retained."},
{"file":"Core/Net/Tls.jet","function":"__c_setup","proof":"Reserved __ name; callers (Tls.jet:134, :150) pass the raw of a configured context they own, the descriptor of the TCPStream that the stream state then keeps beside the context for its whole life (:153), and a server name validated by __valid_server_name (:558) or the empty string. mbedtls_ssl_set_hostname copies the name (jet_tls.c:208-213); I64{fd} is a checked conversion. Only a status Int returns."},
{"file":"Core/Net/Tls.jet","function":"__c_handshake","proof":"Reserved __ name; its caller __handshake (Tls.jet:840) passes the raw of a context that was set up on a live TCPStream and is owned by the caller or the stream state. jet_tls_handshake performs one nonblocking step on that context (jet_tls.c:219-221) and returns a status Int; nothing is borrowed beyond the call."},
{"file":"Core/Net/Tls.jet","function":"__c_read","proof":"Reserved __ name; callers (Tls.jet:898, :919) pass the raw of the stream state's live context after refusing a closed stream (:889) and with a positive limit (TLSStream.read refuses max_bytes <= 0, :176; C_RECORD_BYTES otherwise). jet_tls_read reads at most min(limit, 16384) bytes into the context's own 16384-byte buffer (jet_tls.c:225-228)."},
{"file":"Core/Net/Tls.jet","function":"__c_buffer_bytes","proof":"Reserved __ name; callers (Tls.jet:899, :921) pass the same live context and the positive count the immediately preceding jet_tls_read returned, which is at most the 16384-byte buffer size (jet_tls.c:225-233). __bytes_unpack copies exactly that many bytes into an owned list before any other engine call, so no borrow of the buffer escapes."},
{"file":"Core/Net/Tls.jet","function":"__c_write","proof":"Reserved __ name; its only caller __write_packed (Tls.jet:966) passes the stream state's live context and the packed chunk address and length __write_all allocated and keeps alive until after this call (:943, :948-956). jet_tls_write only reads those bytes during the call (jet_tls.c:242-244)."},
{"file":"Core/Net/Tls.jet","function":"__c_close_notify","proof":"Reserved __ name; its caller (Tls.jet:980) passes the raw of the stream state's live context, which the state owns until its own Close. jet_tls_close_notify performs one nonblocking close_notify step (jet_tls.c:246-248) and returns a status Int."},
{"file":"Core/Net/Tls.jet","function":"__c_verify_result","proof":"Reserved __ name; its caller __handshake_message (Tls.jet:853) passes the raw of the context whose handshake just ran and is still owned. jet_tls_verify_result only reads that context's verification flags (jet_tls.c:251-253)."},
{"file":"Core/Net/Tls.jet","function":"__c_version","proof":"Reserved __ name; its caller __peer_identity (Tls.jet:1206) passes the raw of the context whose handshake completed and is still owned. jet_tls_version only reads the negotiated version number (jet_tls.c:255-257)."},
{"file":"Core/Net/Tls.jet","function":"__c_string_ciphersuite","proof":"Reserved __ name; its caller __peer_identity (Tls.jet:1204) passes the raw of a live context after its handshake. jet_tls_ciphersuite returns Mbed TLS's static suite name or a static empty string, never NULL (jet_tls.c:259-262), so __c_text's strlen and unpack read a valid NUL-terminated string and copy it into an owned String."},
{"file":"Core/Net/Tls.jet","function":"__c_peer_cert","proof":"Reserved __ name; its caller __peer_identity (Tls.jet:1184-1188) passes the raw of a context whose handshake completed and which it owns for the whole loop. The data and length output slots belong to this frame; jet_tls_peer_cert fills them only with a certificate's raw DER held by that context's session, or returns an error past the chain's end (jet_tls.c:278-289), and the DER is copied out by __bytes_unpack before the next engine call."},
{"file":"Core/Net/Tls.jet","function":"__getenv","proof":"Reserved __ name; callers pass the constant names SSL_CERT_FILE and SSL_CERT_DIR (Tls.jet:628-629). getenv returns NULL, which is checked (:1601), or a NUL-terminated string in the environment; neither JetRuntime nor Core calls setenv, putenv or unsetenv, so the string stays valid while __c_text copies it into an owned String."},
{"file":"Core/Net/Tls.jet","function":"__prove_net_mode","proof":"Reserved __ name, called by core.http's client (Http/Client.jet:1467). jet_prove_net_mode takes no argument and only reads the journal mode word (JetRuntime ProveJournal.jet:208-213), returning 0, 1 or 2."},
{"file":"Core/Net/Tls.jet","function":"__prove_net_record","proof":"Reserved __ name, called by core.http's client with an owned tape (Http/Client.jet:1477). An empty tape returns first (Tls.jet:1622); otherwise the packed copy of exactly tape.len() bytes stays alive until the matching free (:1624-1626), and jet_prove_net_record copies those bytes into the journal under its lock during the call (JetRuntime ProveJournal.jet:217-223)."},
{"file":"Core/Net/Tls.jet","function":"__prove_net_take","proof":"Reserved __ name, called by core.http's replay path (Http/Client.jet:1567). The packed request of exactly request.len() bytes lives until the matching free (Tls.jet:1635, :1641); jet_prove_net_take copies it into the journal and, on a hit, stores the address and length of a record in the run's loaded journal, which stays put for the run (JetRuntime ProveJournal.jet:225-253). The output slots belong to this frame, and the record is copied out by __bytes_unpack before return."},
{"file":"Core/Http/Client.jet","function":"__build_socket","proof":"Reserved __ name; it copies the resolver octets into a fresh owned list (Client.jet:653-654) and moves it into core.net.__ip_from_bytes, whose runtime refuses any length but 4 or 16 before allocating (JetRuntime HostNetAddress.jet:31-32), so a malformed address cannot overrun the native carrier. U16{port} stops with E3010 when the port does not fit (core-library.md:4172-4173), and jet_http_build_get already rejects ports outside 0..65535 (:590). It returns an owned SocketAddr; no pointer or borrow escapes."},
{"file":"Core/Jobs/Jobs.jet","function":"queue","proof":"The gates at jobs.jet:130-136 call __path_kind and __create_dir_all_raw with owned String paths computed by queue_default_path/files.dirname, after endpoint authority validation at :124-126. Metadata is a scalar kind query and recursive mkdir returns owned IOError; neither leaf accepts or returns memory addresses. The kind check at :131-132 rejects observed symlinks before the checked SQLite open. Path races do not create a borrowed descriptor or dangling pointer because these are independent pathname operations, not identity-handle transfers."},
{"file":"Core/Jobs/Jobs.jet","function":"wait","proof":"The unsafe __sleep_ms leaf casts signed milliseconds to unsigned host Duration (Top/ServiceAuthority.rs:3792-3793). jobs.jet:386-387 reject negative/oversized timeout and :393 clamps the exact delay passed at :395 to 1..25, independently of wall-clock movement. No caller can reach the cast with a negative or oversized delay; the leaf only sleeps and exports no memory."},
{"file":"Core/Jobs/Jobs.jet","function":"queue_publish_transition","proof":"jobs.jet:486 constructs owned JSON fields and :487 clamps negative timestamps before __publish_parts(:489). The provider Top/ServiceAuthority.rs:3772-3787 validates JetDevtoolsEvent::from_parts before publication, owns entity/fields and borrows source/kind only during the call. No native pointer, callback or mutable queue storage is transferred to a sink; caller-chosen observation text cannot escape that validation boundary."},
{"file":"Core/Jobs/Jobs.jet","function":"queue_publish_status","proof":"jobs.jet:499-502 assemble owned status fields/entity and nonnegative timestamp, then __publish_parts(:504) invokes the existing validating JetDevtoolsEvent::from_parts gate (Top/ServiceAuthority.rs:3779-3786). The sink gets an owned validated event rather than references into JobQueueStatus, so callers cannot make event storage outlive a borrowed source value."},
{"file":"Core/Testing/Testing.jet","function":"testing_byte","proof":"__string_byte performs an unchecked native byte read. Its local helper testing.jet:353-355 is reached by testing_boundary only with 0 < position < text.count_bytes()(:362-363), by testing_diff_lines with cursor < end <= length(:387-393), by testing_append_bytes over a bounded prefix(:408-409,412-457), and testing_ascii_name over 0..<count_bytes(:508-509). These establish nonnegative in-storage indices before U64 conversion; none expose the index-taking helper as a public Core member or allow the String borrow to escape."},
{"file":"Core/Testing/Testing.jet","function":"testing_owned_text","proof":"__bytes_into_text at testing.jet:376 requires valid UTF-8. The only callers are testing_unified_diff(:503) and testing_ascii_name(:517). Diff output appends complete String bytes or scalar-aligned prefixes chosen by testing_boundary(:362-366,423-424,450-451); marker fragments are ASCII. Name output(:508-515) appends only ASCII alphanumerics, hyphen or underscore. Each exact owned list moves into String with no mutation between construction and transfer; arbitrary caller bytes cannot reach this local helper."},
{"file":"Core/Tui/Tui.jet","function":"capability_env_bytes","proof":"__cap_env_bytes at tui.jet:101 borrows a String name and returns an owned optional byte list. Its provider Ui.rs:514-515 obtains var_os and consumes OsString into encoded bytes; no borrowed environment storage is returned and source does not assume UTF-8 (capability_text uses checked String.from_bytes at :112). Arbitrary names cannot manufacture a native pointer or invalidate returned storage."},
{"file":"Core/Tui/Tui.jet","function":"capability_stdout_terminal","proof":"The gate at tui.jet:126-127 invokes __stdout_terminal with no caller-supplied values and returns only a copied Bool host terminal fact at :129. It lends no memory, descriptor ownership or callback to the caller; host terminal presence or absence cannot change the typed result's validity."},
{"file":"Core/Tui/Tui.jet","function":"normalized_style_test_node","proof":"The local test helper at tui.jet:651-655 calls __node_transfer with initialized text/dimensions, fixed kind/role, an empty child list, explicitly absent accessibility slots and present style slots. Its sole caller(:659-663) obtains StyleWords from style_words(:416-421), whose three flags are bounded bits 1/2/4 and whose colors are normalized by color_words. No arbitrary numeric representation or uninitialized optional slot is passed; the returned retained UINode owns its payload."},
  {"file":"Core/Encoding/Cbor.jet","function":"validate","proof":"The cbor.jet:95 gate only matches Dyn's private Number/TypedText variants to reject them(:97-98), and recursively validates initialized Array/Object payloads(:99-104). Discriminant matching precedes payload access; ordered Object storage is borrowed through object_entries while value remains live. No raw address, variant transmute or payload reference escapes in the error/unit result."},
  {"file":"Core/Encoding/Cbor.jet","function":"encode_value","proof":"The cbor.jet:157 gate grants access to private Dyn.Bytes only after its matching branch(:171); all byte reads iterate the owned list length(:173). Object entry views are taken only in the Object branch(:179-180) and retain the value borrow throughout bounded index ordering. encode(:111) validates the tree first, rejecting internal Number/TypedText. The caller's exclusively borrowed output receives copied bytes, never native payload references."},
  {"file":"Core/Encoding/CborStream.jet","function":"retain_key","proof":"__bytes_into_text at cbor_stream.jet:208 consumes stored, which :205 fills with exactly text.count_bytes() bytes obtained through bounds-checked stream.byte_at from a valid String. It neither truncates a scalar nor edits its encoding. Only the exact owned copy moves into the retained key; arbitrary wire bytes cannot reach this transfer."},
  {"file":"Core/Encoding/CborStream.jet","function":"next_inner","proof":"For a CBOR text item cbor_stream.jet:303 obtains the owned payload, :306 strictly validates that exact list's UTF-8 and returns Syntax on failure, then :308 immediately moves it into __bytes_into_text. No mutation intervenes, and binary items(:304) never use the text transfer. Host/wire callers cannot bypass the validator with malformed payload bytes."},
  {"file":"Core/Encoding/Eetf.jet","function":"term","proof":"A method of the private TermWriter (Eetf.jet:521), which is not among core.encoding.eetf's exports (Compiler/Prelude/Core.jet:103), and depth-bounded (:548). Its #Unsafe region (:549-603) only pattern-matches the compiler-private Dyn carriers .Number and .Bytes to encode them; every byte is written through the bounds-checked put, put_u32 and payload helpers after item() length limits, and object entries come from stream.object_entries, itself a recorded safe abstraction. No carrier is constructed or mutated and no memory is addressed."},
  {"file":"Core/Encoding/Csv.jet","function":"close_field","proof":"__bytes_into_text at csv.jet:124 consumes the whole-value parser's field. typed_records(:213-222) obtains complete scalar byte sequences from a valid String, and TypedCSV.push(:138-200) either appends the whole sequence or replaces/removes ASCII CSV syntax; doubled quotes and CRLF are ASCII. Thus field is empty or concatenated complete UTF-8 scalars at every close; it is moved and reset before reuse(:124-126), never exposed as arbitrary caller bytes."},
  {"file":"Core/Encoding/Csv.jet","function":"query_text","proof":"csv.jet:315-316 enforce the bounded read's size/list limits and :317 validates sample.bytes with the strict shared UTF-8 validator. The gate at :318-319 immediately moves that same owned list into __bytes_into_text; failures return FieldError before conversion. A caller's arbitrary file contents cannot produce an invalid String or retain a native read-buffer borrow."},
  {"file":"Core/Encoding/Csv.jet","function":"finish_field","proof":"CSVReader.character(:725-744) validates lead width, all continuation bytes, overlong/surrogate/out-of-range cases before next_inner(:880-882) calls push. push(:798-874) appends each entire validated scalar or only ASCII quote/CRLF escapes. Consequently finish_field's __bytes_into_text(:769) moves only complete UTF-8 storage, then resets field(:771). Partial/invalid wire scalars fail without reaching this gate."},
  {"file":"Core/Encoding/Csv.jet","function":"field_text","proof":"Not a core.encoding.csv export (Compiler/Prelude/Core.jet:105-106); its callers are parse_rows_with's loop (Csv.jet:461, :470, :480, :486, :513, :515). raw is the input String's bytes (:411), and each argument is raw.slice(start, i) or a quoted field of such bytes plus ASCII 0x22 for doubled quotes, cut only at an ASCII quote, CR or LF, or at a whole match of the delimiter, which rows checks is one character of a String (:64-67) and delimiter_at matches in full (:385-391). A UTF-8 continuation byte never equals an ASCII or lead byte, so every cut is a scalar boundary and __bytes_into_text receives complete UTF-8."},
  {"file":"Core/Encoding/Json.jet","function":"emit_coercion","proof":"json.jet:101-102 calls __coerce_emit with ordinary borrowed String text; it supplies no pointer, descriptor or callback. The reserved leaf preserves the existing stderr emission boundary, and the helper returns no borrowed storage. Caller-selected message bytes remain valid UTF-8 and cannot become an unchecked address or outlive the call as a native reference."},
  {"file":"Core/Encoding/Json.jet","function":"coerce_line","proof":"The gate at json.jet:109-110 calls __coerce_now with no arguments and assigns its owned Int clock snapshot. Subsequent interpolation(:112) is safe String construction. No caller can choose a raw memory operand or receive a host borrow through this parameterless scalar query."},
  {"file":"Core/Encoding/Json.jet","function":"append_events","proof":"json.jet:707-709 match each private Dyn.Number/TypedText/Bytes discriminant before projecting its initialized String/list into the corresponding typed DataEvent. Only owned enum payloads are returned; no mismatched variant access, pointer formation or borrowed backing view escapes. Public variants are recursively handled outside the gate(:712-735)."},
  {"file":"Core/Encoding/Stream.jet","function":"object_entries","proof":"__object_entries_view requires an Object and returns a borrowed ordered backing view. stream.jet:11 checks that exact discriminant immediately before :13; other variants panic before the intrinsic. The result's `from value` lifetime at :10 ties the view to the same borrowed Dyn, so callers cannot keep it after releasing its owner or reinterpret another payload."},
  {"file":"Core/Encoding/Stream.jet","function":"object_from_entries","proof":"__object_from_entries at stream.jet:21 receives a consuming list of typed String/Dyn entries, moving its sole allocation into an initialized Object carrier. No pointer, length or discriminant is supplied separately, and the caller cannot reuse the moved list. Arbitrary duplicate keys affect encoding policy, not the validity or ownership of the native payload."},
  {"file":"Core/Encoding/Stream.jet","function":"bytes_payload","proof":"stream.jet:27 checks private Dyn.Bytes before __bytes_view on the same value; all other variants panic(:29). The View<U8> result is declared `from value`(:25), retaining the owner borrow rather than exporting an independent pointer. Callers cannot select a different variant's backing or use the view after the carrier's lifetime."},
  {"file":"Core/Encoding/Stream.jet","function":"typed_number","proof":"The gate at stream.jet:33-34 constructs the compiler-private Number enum from a consuming typed String; it does not reinterpret memory. Its source caller json.jet:562 is in Parser.number after numeric grammar validation and checked String construction(:561). The owned lexeme is retained for typed decoding, never used as an address or unchecked index; consumers use normal checked numeric parsing. Callers cannot corrupt enum layout or retain the moved String's storage."},
  {"file":"Core/Encoding/Stream.jet","function":"typed_string","proof":"__typed_text at stream.jet:40 consumes an initialized String into the private typed-text carrier. UTF-8 is already guaranteed by the String type; this operation neither changes its bytes nor accepts a separately forgeable pointer/length. The caller loses ownership of the moved text and receives an owning Dyn enum, not an escaping borrow."},
  {"file":"Core/Encoding/Stream.jet","function":"read_string","proof":"stream.jet:294 validates the exact decoded byte list, returning Syntax and releasing tracked capacity on failure(:295-296). The __bytes_into_text gate(:299-300) immediately consumes that same list without intervening mutation. Escaped or raw wire bytes therefore cannot bypass UTF-8 validation or expose a dangling parser buffer."},
  {"file":"Core/Encoding/Stream.jet","function":"number","proof":"Both gates discharge separate obligations: stream.jet:360 rejects invalid JSON numeric grammar and :365 rejects invalid UTF-8 before :371 consumes that exact byte list into String. Only after those checks does :375-376 construct DataEvent.Number when typed_numbers is enabled. The valid initialized lexeme moves into its matching carrier; no unchecked numeric payload reinterpretation or raw buffer reference reaches callers."},
  {"file":"Core/Encoding/Stream.jet","function":"fold_text","proof":"stream.jet:555 consumes a valid String into __text_into_bytes solely to measure allocation capacity. That byte list is not modified; :557 can fail before reconstruction, otherwise :559 immediately moves the unchanged UTF-8 list into __bytes_into_text. Budget checks affect retention, not validity; no caller receives the intermediate storage or can mutate it between conversions."},
  {"file":"Core/Encoding/Stream.jet","function":"fold","proof":"stream.jet:566 checks the private DataEvent.Number discriminant before moving its initialized String through fold_text(:567) into matching Dyn.Number(:568). fold_text preserves UTF-8 while accounting allocation ownership. No other event variant reaches Dyn.Number; recursion and ordinary event handling occur outside the gate and no native payload borrow escapes."},
  {"file":"Core/Encoding/Stream.jet","function":"text_from_bytes","proof":"stream.jet:834 strictly validates the caller's exact owned byte list, returning EncodingError on failure; :836 then immediately moves it into __bytes_into_text. No mutation or alternate allocation occurs between validation and transfer. Even arbitrary external byte input cannot return a String violating UTF-8 or retain a pointer into the moved list."},
  {"file":"Core/Encoding/Stream.jet","function":"byte_at","proof":"The unchecked __string_byte at stream.jet:845 is dominated by :841-842, which reject index < 0 or index >= text.count_bytes() before U64 conversion. The borrowed String remains live for the one-byte read; only copied U8 returns. Callers cannot cause a negative, out-of-storage or dangling pointer read through this helper."},
  {"file":"Core/Encoding/Stream.jet","function":"ascii_text","proof":"Despite its formatter-oriented name, ascii_text admits no unchecked caller assumption: stream.jet:1402 runs strict __utf8_validate on the exact owned list and panics on invalid UTF-8. Only the succeeding path at :1404-1405 consumes it into __bytes_into_text, with no intervening write. Malformed formatter output cannot create an invalid String."},
  {"file":"Core/Encoding/Stream.jet","function":"wire_float","proof":"The gate at stream.jet:1415-1416 only transfers the owned valid String produced by Float interpolation(:1413) into __text_into_bytes. It is the safe direction of the UTF-8 transfer and accepts no raw bytes or caller pointer. Later digit editing uses a separate list and checked ascii_text reconstruction; no alias to the moved String is retained."},
  {"file":"Core/Encoding/Stream.jet","function":"jsonl_tree_size","proof":"The gate at stream.jet:1648-1653 runs only in the fallback branch, after every public variant was sized (:1620-1644). It tests the private Dyn.Bytes discriminant without binding its payload (`.Bytes(_)`) and only chooses the error reason and kind; the branch always returns Err (:1654-1656). No payload, pointer or backing view is read or escapes."},
  {"file":"Core/Encoding/Stream.jet","function":"write_tree","proof":"Stream.jet is the Core-internal __core_encoding_stream package, whose reserved name keeps it out of user imports. The two #Unsafe regions (Stream.jet:1722-1724, :1727-1729) build the private DataEvent.Number and DataEvent.Bytes carriers from the payload of the Dyn .Number or .Bytes just matched, which was validated when that Dyn was built, and pass it on unchanged; nothing is read from or written to raw memory."},
  {"file":"Core/Encoding/XmlEngine.jet","function":"text_byte","proof":"__string_byte at xml_engine.jet:135 is an unchecked byte read. Its only local callers are byte_prefix_text(:139-142) and unit_prefix(:2029-2032), both iterating 0..<needle.count_bytes(), and ascii_equal(:1386-1389), which first checks equal String storage lengths then iterates 0..<left.count_bytes(). Every index is nonnegative and below the corresponding borrowed String's byte length; no pointer or view escapes the copied U8 result."},
  {"file":"Core/Encoding/XmlEngine.jet","function":"byte_text","proof":"__bytes_into_text at xml_engine.jet:174 consumes a copy of source[start..<end]. Every local caller slices bytes of an existing String (DOCTYPE/entity/declaration helpers) or Scanner.source, whose sole construction path parse_document_with(:667-669) takes String.bytes(). Boundaries are zero/end or offsets adjacent to ASCII XML delimiters, quotes and whitespace, which cannot lie inside a UTF-8 continuation sequence. Checked list accesses(:172) prevent out-of-storage copies; the exact copied scalar-aligned allocation moves with no intervening mutation or escaping borrow."},
  {"file":"Core/Encoding/XmlEngine.jet","function":"normalize_attribute_text","proof":"xml_engine.jet:2204 starts with bytes of a valid String. The in-place loop(:2207-2215) replaces only ASCII CR/LF/tab with ASCII space and removes the ASCII LF following CR; all non-ASCII bytes keep their order and membership. write never exceeds read, and :2217 truncates only the finished compacted sequence. Thus __bytes_into_text(:2219) consumes valid complete UTF-8 for every caller text, not an assumed-valid wire allocation."},
  {"file":"Core/Encoding/Yaml.jet","function":"yaml_leaf","proof":"yaml.jet:320 grants private carrier matching. Number/TypedText explicitly panic(:326); Bytes are read only after their matching discriminant over 0..<bytes.len()(:328-333). Object payload views are obtained only in the Object branch(:344-347), retaining the owner; arrays recurse on typed values. The caller receives an owned String and cannot request a mismatched payload or retain native storage references."},
  {"file":"Core/Service/Service.jet","function":"state_path_kind","proof":"__path_kind at service.jet:1365 accepts a borrowed valid String path and returns copied metadata kind or owned IOError. It performs a final-component pathname query, not a descriptor/identity ownership transfer; no caller pointer or reference into OS metadata is returned. A path race can alter the metadata result but cannot invalidate a caller memory borrow."},
  {"file":"Core/Ui/Ui.jet","function":"gtk_backend","proof":"ui.jet:126-127 invoke __gtk_backend without caller-supplied memory or callbacks. Its provider UiGtk.rs:2069-2070 calls JetGtkBackend::new(:1346-1371), allocating an owning Arc<Mutex<GtkState>> with every field initialized, empty queues and null not-yet-initialized GTK handles. Construction neither dereferences those handles nor installs/borrows the current host. The caller receives the owned backend carrier, not a pointer into a scoped provider."},
  {"file":"Core/Ui/Ui.jet","function":"tui_backend","proof":"__tui_backend at ui.jet:132 has no arguments and allocates JetTuiBackend through Ui.rs:1860-1861,1572-1591. The provider owns a fresh Arc<Mutex<State>> with initialized capability facts, absent frames/focus, zero counters and an empty grid. It performs no current-host scope dereference and exports no borrow of constructor-local storage; arbitrary safe callers cannot supply a forged native backing pointer."},
  {"file":"Core/Ui/Ui.jet","function":"null_backend","proof":"__null_backend at ui.jet:137 has no caller-selected fields and calls JetNullBackend::new (Ui.rs:1856-1857,1346-1356). That constructor owns a new Arc<Mutex<State>> containing initialized absent frame/event/focus slots and an empty command list; it neither accesses scoped host pointers nor returns references into temporary storage. The returned carrier keeps its allocation alive independently of the caller frame."},
  {"file":"Core/Ui/Ui.jet","function":"node_accessibility","proof":"ui.jet:148 calls __accessibility_transfer with typed UINode/UIAccessibility and the explicit present=true flag. Ui.rs:302-308 only assign Some(metadata) to the owning node then return that node; no discriminant reinterpretation, unchecked index, callback invocation or current-host access occurs. Caller metadata remains an initialized typed record and the result owns its Strings/lists rather than borrowing source storage."},
  {"file":"Core/Ui/Ui.jet","function":"node_shortcut","proof":"ui.jet:165 passes initialized typed node/shortcut with present=true to __shortcut_transfer. Its provider Ui.rs:311-317 only stores the typed shortcut in the node's Option and returns the owning node, with no scoped-host dispatch or raw address access. Any valid typed shortcut therefore preserves layout/lifetimes, and no native registry borrow escapes."},
  {"file":"Core/Ui/Ui.jet","function":"transfer_node","proof":"__node_transfer trusts numeric role/kind/absence/style representations (Ui.rs:1938-1985). ui.jet:173-184 derives role solely as -1 or0..3 and kind solely as0..4 from typed enums. :185-192 couples color presence to initialized text, fixes all six absent-slot bits to63, and marks style absent with zero initialized words. Native :1987-2002 own the copied label/color and typed children, never dereferencing caller addresses. Width/height are stored floats, not memory lengths; invalid geometry cannot cause an unchecked allocation transfer here."},
  {"file":"Core/Ui/Ui.jet","function":"text_input","proof":"A public core.ui export that takes only a String and a UIIMEMode. core.ui.__text_input dispatches to jet_ui_text_input (JetRuntime UiTransfers.jet:90-107), which refuses an IME word outside the two variants (:91), zero-fills a freshly allocated node (:92-97), stores a clone of the borrowed String rather than the caller's buffer (:98) and returns that owned UINode. Any String and either IME mode is a valid input, and no caller memory is retained."},
  {"file":"Core/Vendor/Sqlite3/Fixture/run.jet","function":"run","proof":"The no-argument fixture supplies only fixed SQL and an in-memory database name. Exclusive U64 out-slots start at zero; successful sqlite3_open_v2 and sqlite3_prepare_v2 checks dominate all database/statement uses. SQLite owns the resulting handles; the fixture never forges an address, lends a callback, or exposes a handle to callers. It steps the statement before reading column zero, finalizes it once before closing the database once, and uses neither afterward. Borrowed String storage remains live for each synchronous call; any failed operation stops before further handle use."},
  {"file": "Core/Sys/Sys.jet", "function": "getpgid", "proof": "A read-only POSIX query: it changes no process state, touches no memory the caller owns, and returns an Int or IOError."},
  {"file": "Core/Sys/Sys.jet", "function": "getsid", "proof": "A read-only POSIX query: it changes no process state, touches no memory the caller owns, and returns an Int or IOError."}
]
```
<!-- unsafe-abstractions:end -->
