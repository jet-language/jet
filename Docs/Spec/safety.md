# Safety

This specification defines Jet's default safety boundary and its audited
expert escape. It is for Jet users, library authors, and reviewers. The
executable spelling lives in the [syntax registry](../../Compiler/JetFoundation/Data/Syntax/);
sema checks the gate and its obligations in
[`UnsafeObligations.jet`](../../Compiler/JetSema/Source/Sema/UnsafeObligations.jet).
The focused [`unsafe_ratchet` cases](../../Tests/Cases/GuardSelftest/) and the
[`ratchet checker`](../../Tools/Agent/UnsafeRatchet.jet) provide the
repository proof; gated examples live under
[`Examples/`](../../Examples/). Terms such as stream and
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

`jet Tools/Agent/SafetyGate.jet OUT ARTIFACTS_JSON [SEED COUNT]` runs the
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

The caller is `Tools/Agent/VerifyFull.jet`: its `safety-gate` row runs the
full gate into its scratch directory when `JET_SAFETY_ARTIFACTS` names the
admitted artifacts JSON. Without it, the row runs
`SafetyGate.jet --source OUT JET [SEED COUNT]`: the two unsafe-wrapper
fixtures against the exact-head compiler VerifyFull has pinned, plus the
generated corpus checked against its model. That source leg covers no native
tier, so it never stands in for the full gate. VerifyFull pins the source tree;
there is no hosted CI, nightly job or watcher. `COUNT` sets each of the two
generated case families (default 50; `SEED` defaults to 4).

The gate records `PASS` (exit 0), findings `FAIL` (1), infrastructure error
(2), or unavailable coverage `GAP` (3). An owner-authorized gap receipt
remains non-green; it is never converted to a success. Receipts bind artifact
hash, source head, seed, corpus hash, all findings and every coverage gap.

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

[`UnsafeRatchet.jet`](../../Tools/Agent/UnsafeRatchet.jet)
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
jet Tools/Agent/UnsafeRatchet.jet --update
```

If source removes only recorded regions, the checker lowers and rewrites the
baseline automatically. A new semantic region cannot hide behind a net
decrease. A count-preserving edit that changes a recorded file, position, or
reason is stale and fails until the baseline is refreshed. The
`unsafe-ratchet` block is intentionally kept byte-stable between such
refreshes.

`--check` compares without ever writing: a pure decrease fails too, naming the
`--update` run that records it. `jet Tools/Agent/LaneCheck.jet` runs
`UnsafeRatchet.jet --check` on every lane check, so an unrecorded region, a
stale row or a failed CoreLib audit stops the check before commit.

Signal handlers stay inside the vetted runtime boundary, whatever the baseline
records. That boundary is `SIGNAL_BOUNDARY`: the runtime
(`Compiler/JetRuntime/`) and the image loader
(`Compiler/JetBackend/Source/Image/Loader.jet`), which sets the loaded program's
initial dispositions. The checker fails on every `#Unsafe` region outside it
whose code installs a signal handler. That covers `rt_sigaction` by name, by its
syscall number or by its `OS_SYS` constant; the C `signal`/`sigaction` family;
and the Core calls that register a handler. It fails with `--update` too.
`core.sys.reset_signal` only restores the default action; it stays a gated
POSIX control. Programs reach signals through `core.process.signals`,
`core.process.on_signal` and `core.sys.on_interrupt`, which the runtime's
signal hub serves.

Every region kind lowers to its own sentry (D-MEM-SENTRY1): a block, a gated
`#Unsafe fn` body, and a gated `#FFI`/`#Import(c)` declaration, whose sentry
wraps each call.
A source-level gate does not itself require a raw unsafe operation: expert
crypto calls may use safe, source-owned Core Jet bodies while sema still requires
`#Unsafe` for nonce, algorithm, and secret-exposure obligations (E0510).

The same scan counts the `#Unsafe` regions in Compiler, Core and Jetpack that
perform a raw memory action outside their package's kernel files (the files
`policy.memory` names; a file no `package.jet` holds is its own kernel). The
`memory-kernel:outside` line above the generated section records that count.
A rise fails. A fall fails `--check` and lowers the line on a plain run. The
memory kernel cutover takes it to zero. `jet Tools/Agent/UnsafeRatchet.jet
--inventory` lists each region with its actions and kernel, and
`--inventory --json` prints the same for tools. A package under a `Tests/`
directory is test-only, and the inventory marks it so: its kernel may name
only `*Tests.jet` test files, and a shipped package's kernel may not name a
file in a `Tests/` tree. The ratchet refuses either.

<!-- memory-kernel:outside 0 -->
<!-- unsafe-ratchet:begin -->
<!-- unsafe-ratchet:data
{
  "schema": 1,
  "total": 1670,
  "counts": {
    "Compiler": 825,
    "Core": 179,
    "Docs": 18,
    "Examples": 45,
    "Jetpack": 8,
    "Tests": 567,
    "Tools": 28
  },
  "regions": [
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 270,
      "column": 5,
      "reason": "`address` is the fresh mapping just made, page-rounded past the NUL"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 284,
      "column": 5,
      "reason": "memfd_create reads the NUL-terminated name at `name`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 298,
      "column": 5,
      "reason": "the caller owns `fd` and nothing uses it any more"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 308,
      "column": 9,
      "reason": "the caller mapped `count` writable bytes at `address`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 544,
      "column": 9,
      "reason": "a private read-only view of the memfd aliases nothing writable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 614,
      "column": 17,
      "reason": "loader_map_file mapped this segment private and writable at its vaddr above"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 636,
      "column": 5,
      "reason": "MAP_FIXED_NOREPLACE never replaces an existing mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 679,
      "column": 5,
      "reason": "MAP_FIXED replaces only the image's own reserved pages"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 684,
      "column": 9,
      "reason": "the bytes lie in the segment's last file page, mapped private and writable above"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 694,
      "column": 1,
      "reason": "the caller mapped [from, to) + delta private and writable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 756,
      "column": 5,
      "reason": "MAP_FIXED replaces only the view's own fresh pages"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 760,
      "column": 5,
      "reason": "the bytes lie in the view's last image page, mapped private and writable above"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 763,
      "column": 5,
      "reason": "the view's image pages are mapped private and writable above"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 767,
      "column": 9,
      "reason": "MAP_FIXED replaces only the view's own fresh pages; the memfd is never written again"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 773,
      "column": 9,
      "reason": "MAP_FIXED replaces only the view's own fresh pages past the program part; the table goes to more of them, the ELF header to the view's first page, mapped private and writable above"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1241,
      "column": 5,
      "reason": "openat reads the NUL-terminated path at `name`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1269,
      "column": 5,
      "reason": "the timespec lies in the fresh zeroed page; nanosleep only reads it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1277,
      "column": 1,
      "reason": "the caller owns the stack bytes below `at`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1287,
      "column": 1,
      "reason": "the caller owns the stack bytes below `at`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1410,
      "column": 1,
      "reason": "the map's 104 bytes lie in the caller's data page, in its mm-map room; brk(0) only reports the current break"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1502,
      "column": 1,
      "reason": "the caller's executor page is its own fresh read+write mapping of one page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1549,
      "column": 5,
      "reason": "the data page is this plan's own fresh mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1562,
      "column": 5,
      "reason": "`executor` is the one-page read+write mapping just made for it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1576,
      "column": 13,
      "reason": "F_GETFD only reads the descriptor's flags"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1588,
      "column": 9,
      "reason": "rt_sigaction with no new action writes the old one (32 bytes) to the scratch"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1606,
      "column": 5,
      "reason": "a fresh anonymous reservation aliases nothing"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1628,
      "column": 5,
      "reason": "getrandom fills the 16 stack bytes at `random`, and each string goes below them, in the fresh main stack this plan owns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1692,
      "column": 5,
      "reason": "the words lie in the stack mapping, above its guard page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1695,
      "column": 5,
      "reason": "the data page's mm-map room is this plan's own"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1718,
      "column": 5,
      "reason": "the block is this plan's own fresh mapping of plan_size bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1740,
      "column": 5,
      "reason": "the executor's records only reset process state; it then enters the loaded image on its own stack"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1820,
      "column": 5,
      "reason": "`executor` is the one-page read+write mapping just made for it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1836,
      "column": 5,
      "reason": "the block is this plan's own fresh mapping of plan_size bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1868,
      "column": 5,
      "reason": "the record block holds its count, then that many records of eight words; madvise only marks whether a fork copies the mappings the child unmaps; the executor's first record forks, and the child runs only the executor and enters the image, whose pages and the plan's it keeps"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1907,
      "column": 5,
      "reason": "landlock_create_ruleset reads the 8-byte handled-access word at the start of the fresh page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1917,
      "column": 9,
      "reason": "`page` is this function's fresh scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1922,
      "column": 5,
      "reason": "no_new_privs and landlock_restrict_self change only this thread's own rights; the ruleset fd is this function's"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1941,
      "column": 1,
      "reason": "landlock_add_rule reads the packed 12-byte rule this writes at the start of the caller's scratch `page`; openat reads the NUL-terminated path at `name`, and the O_PATH fd is this function's"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1968,
      "column": 5,
      "reason": "wait4 writes one int status to the fresh page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1978,
      "column": 5,
      "reason": "SIGKILL to a child this process started and has not reaped"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 1998,
      "column": 5,
      "reason": "a kernel sigaction with handler SIG_IGN on a fresh page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2014,
      "column": 9,
      "reason": "a zeroed kernel sigaction (SIG_DFL) and an empty signal set on a fresh page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2035,
      "column": 5,
      "reason": "execve reads the NUL-terminated strings and the two NULL-ended arrays in the block; the empty signal set at the block's start"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2075,
      "column": 5,
      "reason": "the bytes below `at` and the two arrays lie in the fresh block mapped above, sized for every string and word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2448,
      "column": 9,
      "reason": "PTRACE_SETOPTIONS on the helper's own stopped tracee"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2471,
      "column": 5,
      "reason": "PTRACE_SETOPTIONS on the helper's own stopped tracee"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2521,
      "column": 9,
      "reason": "fork copies this thread alone; the helper and the program it starts make only system calls on words computed before the fork and on the shared scratch page, allocating nothing, so no lock another thread held at the fork can stop them; execve reads the argument block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2577,
      "column": 9,
      "reason": "the program stored its refused PTRACE_TRACEME's result in the shared scratch page before it exited"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2596,
      "column": 1,
      "reason": "`call` is the call words of the trace's scratch page, which the helper shares with this process; the helper makes the system calls the words name"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2618,
      "column": 1,
      "reason": "`call` is the call words of the trace's scratch page, which the helper shares with this process; wait4 and PTRACE_GETREGS write only the scratch page's status word, rusage and register block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2643,
      "column": 5,
      "reason": "the helper's step writes the status word, the rusage and the register block of the trace's scratch page, which are read once it answers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2672,
      "column": 5,
      "reason": "socketpair and pipe2 each write their two int descriptors to the trace's scratch word, which is read before the next call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2709,
      "column": 1,
      "reason": "fcntl, dup2, dup3, close_range, rt_sigprocmask and exit_group of the helper's own descriptors and itself; `call` is the call words of the trace's scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2758,
      "column": 9,
      "reason": "fork copies this thread alone; the helper makes only system calls on words computed before the fork and on the shared scratch page, allocating nothing, so no lock another thread held at the fork can stop it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2803,
      "column": 5,
      "reason": "the helper's PTRACE_SEIZE and PTRACE_INTERRUPT of a thread of the program it is to trace"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2817,
      "column": 5,
      "reason": "the helper's PTRACE_DETACH of its own stopped tracee"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2826,
      "column": 1,
      "reason": "the helper makes the system call as given; an argument that points into memory points into the trace's scratch page, the only memory it shares with this process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2831,
      "column": 5,
      "reason": "the call's number and arguments fill the first seven words of the trace's call; sendto reads its signal byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2853,
      "column": 5,
      "reason": "read writes one byte to the trace's call signal byte; the helper stored the result before it answered"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2869,
      "column": 5,
      "reason": "the helper's wait4 writes one int status to the trace's scratch word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2890,
      "column": 9,
      "reason": "the helper's wait4 writes one int status to the trace's scratch word; poll reads and writes the one pollfd at LOADER_TRACE_POLL, in the scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2912,
      "column": 5,
      "reason": "tgkill of one thread of our own tracee"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2926,
      "column": 5,
      "reason": "PTRACE_SETOPTIONS on a stopped thread of the helper's own tracee"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2959,
      "column": 5,
      "reason": "the helper's PTRACE_CONT, PTRACE_SINGLESTEP or PTRACE_SYSCALL on its own stopped tracee"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2968,
      "column": 5,
      "reason": "the helper's PTRACE_PEEKDATA stores the tracee's word to the trace's scratch word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2979,
      "column": 5,
      "reason": "the helper's PTRACE_POKEDATA writes one word of its own stopped tracee"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 2995,
      "column": 9,
      "reason": "process_vm_readv reads the two iovecs at LOADER_TRACE_IOV and writes at most `want` bytes from LOADER_TRACE_BYTES, inside the trace's scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3027,
      "column": 9,
      "reason": "the `want` bytes are copied to LOADER_TRACE_BYTES of the trace's scratch page, inside it, and pwrite64 reads them there"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3056,
      "column": 5,
      "reason": "the helper's PTRACE_GETREGS writes the register block at the start of the trace's scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3068,
      "column": 5,
      "reason": "the helper's PTRACE_GETFPREGS writes the 512-byte block at LOADER_TRACE_BYTES of the trace's scratch page, inside it, and the block is copied from there"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3084,
      "column": 5,
      "reason": "the helper's PTRACE_SETREGS reads the register block written at the start of the trace's scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3096,
      "column": 5,
      "reason": "the helper's PTRACE_PEEKUSER stores one word of its own stopped tracee's user area to the trace's scratch word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3109,
      "column": 5,
      "reason": "the helper's PTRACE_POKEUSER writes one debug register of its own stopped tracee"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3127,
      "column": 5,
      "reason": "the helper's PTRACE_GETEVENTMSG stores the message to the trace's scratch word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3143,
      "column": 5,
      "reason": "the helper's PTRACE_GETSIGINFO writes the 128-byte siginfo at the start of the trace's scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3160,
      "column": 5,
      "reason": "the helper's wait4 writes one int status to the trace's scratch word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3183,
      "column": 9,
      "reason": "the helper's wait4 writes one int status to the trace's scratch word; nanosleep reads the timespec at LOADER_TRACE_SLEEP, in the scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3207,
      "column": 5,
      "reason": "tgkill of one thread of the helper's own tracee"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3217,
      "column": 9,
      "reason": "SIGKILL to the helper's own tracee"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3228,
      "column": 17,
      "reason": "the helper's PTRACE_CONT of a stopped thread of its own tracee, which the kill ends"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3247,
      "column": 9,
      "reason": "wait4 of our own child, the helper, keeping no status"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3393,
      "column": 5,
      "reason": "`attr` is the fresh zeroed page just mapped: the attribute block at its start, the clock's two words at its middle"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3405,
      "column": 1,
      "reason": "the caller's zeroed attribute block `attr`, LOADER_PERF_ATTR_SIZE bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3420,
      "column": 1,
      "reason": "`attr` is the caller's attribute block, which perf_event_open only reads; the ring is a fresh shared mapping of the event's own descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3446,
      "column": 1,
      "reason": "clock_gettime writes two words at the caller's `scratch`, read before the next"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3460,
      "column": 9,
      "reason": "the ring is this sampler's live shared mapping: its header page, then `size` data bytes the kernel wrote up to data_head, read before data_tail lets it write there again"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3580,
      "column": 5,
      "reason": "the attribute block goes to the command's fresh page's attribute words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3593,
      "column": 5,
      "reason": "clock_gettime writes the command's page's read words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3731,
      "column": 5,
      "reason": "each pipe2 writes its two descriptors to the fresh page's status word, which is read before the next"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3747,
      "column": 9,
      "reason": "fork copies this thread alone; the child makes only system calls on words computed before the fork and on its copy of the page, allocating nothing, so no lock another thread held at the fork can stop it; execve reads the argument block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3783,
      "column": 5,
      "reason": "the go byte of the command's page to the child's pipe"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3791,
      "column": 5,
      "reason": "read fills the failed execve's word of the command's page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3810,
      "column": 5,
      "reason": "wait4 writes the int status and the rusage to the command's page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3821,
      "column": 5,
      "reason": "the command's page holds the wait status and rusage its reap wrote"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3850,
      "column": 5,
      "reason": "the attributes go to the command's fresh page's attribute words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3870,
      "column": 5,
      "reason": "each read goes to the command's page's read words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3884,
      "column": 1,
      "reason": "pipe2 writes two int descriptors to the caller's word `word`, read at once"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3898,
      "column": 1,
      "reason": "close, read, rt_sigaction, rt_sigprocmask, execve, write and exit_group of the child's own descriptors and itself; `page` is its copy of the counted run's page, whose start is a zeroed sigaction, and execve reads the argument block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3921,
      "column": 1,
      "reason": "the attributes go to the caller's zeroed counted-run page `page`, which perf_event_open only reads"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3953,
      "column": 5,
      "reason": "the attributes go to the fresh page's attribute words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3972,
      "column": 5,
      "reason": "each read goes to the counters' own page's read words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 3985,
      "column": 1,
      "reason": "read fills three words of the caller's counted-run page `page`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 67,
      "column": 9,
      "reason": "the scratch mapping is the child's own, made above"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 144,
      "column": 9,
      "reason": "getrandom fills the first word of our scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 151,
      "column": 9,
      "reason": "getrandom wrote the scratch page's first word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 187,
      "column": 1,
      "reason": "pipe2 writes two descriptors into the first word of the caller's scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 197,
      "column": 9,
      "reason": "fcntl duplicates our own descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 211,
      "column": 5,
      "reason": "closes our own descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 227,
      "column": 5,
      "reason": "fcntl F_SETOWN names our own child as the owner of our own descriptor's signals"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 232,
      "column": 5,
      "reason": "fcntl F_SETFL sets O_ASYNC on our own descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 241,
      "column": 1,
      "reason": "the caller owns the `size` bytes at `buffer`, a multiple of 8"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 263,
      "column": 5,
      "reason": "the buffer is the child's scratch mapping past its first page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 288,
      "column": 9,
      "reason": "the poll set is our scratch page's first three entries, which poll reads and writes; the clock words lie past them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 311,
      "column": 9,
      "reason": "poll wrote the entries' returned events"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 324,
      "column": 13,
      "reason": "os_read just wrote `moved` bytes past the scratch page's first page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 340,
      "column": 9,
      "reason": "the poll set is our scratch page's first two entries"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 345,
      "column": 9,
      "reason": "poll reads and writes the two entries of our scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 353,
      "column": 9,
      "reason": "poll wrote the entries' returned events"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 373,
      "column": 5,
      "reason": "os_read just wrote `moved` bytes past the scratch page's first page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 383,
      "column": 1,
      "reason": "the caller's `count` bytes at `address` lie in our mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 391,
      "column": 1,
      "reason": "clock_gettime writes two words at scratch + 64, past the poll set of the caller's scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 404,
      "column": 9,
      "reason": "kills our own child"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 415,
      "column": 9,
      "reason": "wait4 writes the status word into our scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/Image/SessionHost.jet",
      "line": 420,
      "column": 5,
      "reason": "wait4 wrote the status word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 153,
      "column": 1,
      "reason": "a raw Linux x86-64 system call; the caller passes the call number and arguments that call expects"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 179,
      "column": 5,
      "reason": "the caller owns `count` writable bytes at `address`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 186,
      "column": 5,
      "reason": "the caller owns `count` readable bytes at `address`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 205,
      "column": 5,
      "reason": "the caller owns `address .. address + size` when it names one, so a fixed mapping replaces only its own pages; the pages mapped are the caller's alone"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 214,
      "column": 5,
      "reason": "the caller owns the mapping it changes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 222,
      "column": 5,
      "reason": "the caller owns the mapping and nothing uses it any more"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 236,
      "column": 5,
      "reason": "the caller owns `address .. address + size` when it names one, so a fixed mapping replaces only its own pages; the pages mapped are the caller's alone"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 244,
      "column": 5,
      "reason": "the caller owns the mapping and nothing uses it any more"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 254,
      "column": 5,
      "reason": "the caller owns the mapping and keeps no pointer into it past this call but the result"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 259,
      "column": 1,
      "reason": "stores one word at an address the caller owns and has mapped writable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 266,
      "column": 1,
      "reason": "reads one word at an address the caller owns and has mapped readable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 273,
      "column": 1,
      "reason": "reads one byte at an address the caller owns and has mapped readable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 280,
      "column": 1,
      "reason": "stores one byte at an address the caller owns and has mapped writable"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 291,
      "column": 1,
      "reason": "the caller owns `bytes.len()` rounded up to 8 bytes at `address`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 297,
      "column": 1,
      "reason": "the caller owns `count` rounded up to 8 bytes at `address`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 306,
      "column": 1,
      "reason": "calls machine code; the caller vouches for the code and its System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 319,
      "column": 1,
      "reason": "calls machine code; the caller vouches for the code and its two-word System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 334,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked zero-argument Unit System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 344,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked three-word Unit System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 360,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked one-word Unit System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 372,
      "column": 1,
      "reason": "calls machine code; the caller holds the mapping and its checked two-word Unit System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 389,
      "column": 1,
      "reason": "a raw Linux x86-64 syscall; the caller supplies the syscall's valid machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 406,
      "column": 1,
      "reason": "reads one aligned machine word in live allocator-owned mapped memory"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 413,
      "column": 1,
      "reason": "stores one aligned machine word in live allocator-owned mapped memory; x86 TSO makes this a release store"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 422,
      "column": 1,
      "reason": "atomically compares and exchanges one aligned live machine word; every non-atomic access to protected data holds its lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 437,
      "column": 1,
      "reason": "x86 pause only hints that this thread is waiting in a spin loop"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 447,
      "column": 1,
      "reason": "cpuid only reports processor identification; rbx is saved and restored"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 460,
      "column": 1,
      "reason": "cpuid only reports processor identification; rbx is saved and restored"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 474,
      "column": 1,
      "reason": "cpuid only reports processor identification; rbx is saved and restored"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 487,
      "column": 1,
      "reason": "cpuid only reports processor identification; rbx is saved and restored"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 502,
      "column": 1,
      "reason": "xgetbv only reads an extended control register; the caller has seen CPUID.1:ECX.OSXSAVE"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 516,
      "column": 1,
      "reason": "rdtsc only reads the time-stamp counter; the caller has seen it enabled for this process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 532,
      "column": 1,
      "reason": "reads 32 state bytes at state, 64 block bytes at data and 272 table bytes at table, and writes the 32 state bytes; the caller has seen the SHA extensions"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 729,
      "column": 1,
      "reason": "reads 176 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 807,
      "column": 1,
      "reason": "reads 208 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 895,
      "column": 1,
      "reason": "reads 240 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 993,
      "column": 1,
      "reason": "reads 176 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1071,
      "column": 1,
      "reason": "reads 208 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1159,
      "column": 1,
      "reason": "reads 240 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1261,
      "column": 1,
      "reason": "reads 176 round-key bytes at keys and 16 bytes at src, pre and post, and writes 16 bytes at chain and at out; the caller has seen AES-NI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1297,
      "column": 1,
      "reason": "reads 208 round-key bytes at keys and 16 bytes at src, pre and post, and writes 16 bytes at chain and at out; the caller has seen AES-NI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1337,
      "column": 1,
      "reason": "reads 240 round-key bytes at keys and 16 bytes at src, pre and post, and writes 16 bytes at chain and at out; the caller has seen AES-NI"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1390,
      "column": 1,
      "reason": "reads and writes 16 bytes at y and reads 16 bytes at data and at h and the 16-byte mask at table; the caller has seen PCLMULQDQ and SSSE3"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1465,
      "column": 1,
      "reason": "reads and writes 16 bytes at y and reads 64 bytes at data and at powers and the 16-byte mask at table; the caller has seen PCLMULQDQ and SSSE3"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1589,
      "column": 1,
      "reason": "reads the stack pointer register after one balanced push; no memory is read"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1605,
      "column": 1,
      "reason": "the caller owns count readable bytes at source and count writable bytes at target; an overlap has target below source"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1626,
      "column": 1,
      "reason": "the caller owns count readable bytes at source and count writable bytes at target, and count is 1 through 3"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1649,
      "column": 1,
      "reason": "the caller owns count readable bytes at source and count writable bytes at target, and count is 4 through 7"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1664,
      "column": 1,
      "reason": "the caller owns count readable bytes at source and count writable bytes at target, and count is 8 through 16"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1679,
      "column": 1,
      "reason": "the caller owns count readable bytes at source and count writable bytes at target, and count is 16 through 32"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1694,
      "column": 1,
      "reason": "the caller owns count readable bytes at source and count writable bytes at target, and count is 32 through 64"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1713,
      "column": 1,
      "reason": "the caller owns count readable bytes at source and count writable bytes at target, and count is 64 through 128"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1742,
      "column": 1,
      "reason": "the caller owns 64 readable bytes at source and 64 writable bytes at target"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1760,
      "column": 1,
      "reason": "the caller holds 64 readable bytes at left and at right"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1788,
      "column": 1,
      "reason": "the caller owns count writable words starting at target"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1808,
      "column": 1,
      "reason": "the caller owns count writable bytes at target, and count is 1 through 3"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1824,
      "column": 1,
      "reason": "the caller owns count writable bytes at target, and count is 4 through 7"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1836,
      "column": 1,
      "reason": "the caller owns count writable bytes at target, and count is 8 through 16"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1847,
      "column": 1,
      "reason": "the caller owns count writable bytes at target, and count is 16 through 32"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1860,
      "column": 1,
      "reason": "the caller owns count writable bytes at target, and count is 32 through 64"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1877,
      "column": 1,
      "reason": "the caller owns count writable bytes at target, and count is 64 through 128"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1903,
      "column": 1,
      "reason": "the caller owns 64 writable bytes at target"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1919,
      "column": 1,
      "reason": "the caller owns count writable bytes at target"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1934,
      "column": 1,
      "reason": "reads one byte from a live readable mapped address supplied by the caller"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1941,
      "column": 1,
      "reason": "writes one byte to a live writable mapped address supplied by the caller"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1951,
      "column": 1,
      "reason": "writes at most eight bytes from a stack-local word while that word is live; the balanced pushes and pops do not expose stack storage after return"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 1972,
      "column": 1,
      "reason": "bytes lends length readable bytes; the second iovec lends a stack-local LF; writev retains neither pointer and every push is balanced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2009,
      "column": 1,
      "reason": "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2056,
      "column": 1,
      "reason": "the calling thread is attached and GS points at its live Jet TCB"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2065,
      "column": 1,
      "reason": "ARCH_GET_GS writes one initialized aligned stack word only during this syscall; no TLS or heap is read, and the push/pop is balanced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2085,
      "column": 1,
      "reason": "base is a live self-addressed Jet TCB the calling thread runs on until the next switch, and the kernel enabled FSGSBASE"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2099,
      "column": 1,
      "reason": "only a signal restorer's own frame calls this, on the stack its handler returned to"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2117,
      "column": 1,
      "reason": "save is a writable word the resumer reads; target is a stack this switch or a fresh fiber seed left, owned by no running thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2142,
      "column": 1,
      "reason": "atomically adds to one aligned live word and returns its previous value"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2155,
      "column": 1,
      "reason": "atomically exchanges one aligned live word; xchg has an implicit full barrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2168,
      "column": 1,
      "reason": "calls executable System V code with one raw word argument"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2181,
      "column": 1,
      "reason": "calls a Jet closure's code with the live closure header and one raw word argument"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2199,
      "column": 1,
      "reason": "calls C code; the caller vouches for the code and its word-argument System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2226,
      "column": 1,
      "reason": "raw clone child runs only Jet/syscall code, never Rust or musl; supplied stack contains entry and argument"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2267,
      "column": 1,
      "reason": "calls checked System V Unit code; the caller lends one live raw machine-word argument"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2279,
      "column": 1,
      "reason": "calls checked System V Unit code; the caller vouches for both raw machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2293,
      "column": 1,
      "reason": "calls checked System V Unit code; the caller vouches for all three raw machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2309,
      "column": 1,
      "reason": "calls a checked native runtime callback with one machine-word argument"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2322,
      "column": 1,
      "reason": "calls a checked native runtime callback with two machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2337,
      "column": 1,
      "reason": "calls a checked native runtime callback with three machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2354,
      "column": 1,
      "reason": "calls a checked native runtime callback with four machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2373,
      "column": 1,
      "reason": "calls a checked native runtime callback with five machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2394,
      "column": 1,
      "reason": "ioctl reads one stack-local machine word with the supplied audited request and live fd; it retains no pointer and all pushes are balanced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2417,
      "column": 1,
      "reason": "object is a live fd for fstat or a terminated path for stat/lstat; output lends 128 writable bytes; the initialized stack stat frame never escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2506,
      "column": 1,
      "reason": "the audited caller lends the stat scalar row and valid syscall object"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2508,
      "column": 5,
      "reason": "packed Linux 32-bit fields become independent portable machine words without any allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2519,
      "column": 1,
      "reason": "calls a checked System V void(void) process entry that remains executable for the entire call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2529,
      "column": 1,
      "reason": "directory and path identify a source-approved statx object; output lends three writable words; the zeroed 256-byte statx stack buffer never escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 2631,
      "column": 1,
      "reason": "calls machine code; the caller vouches for its three-word System V signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetCLI/Source/CLI/Host.jet",
      "line": 2521,
      "column": 9,
      "reason": "I2: jet alone among Jet programs ends a Jet defect with status 101, which process.exit maps to 1 for every program (S36), so its ICE exit takes the unmapped process exit"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 201,
      "column": 5,
      "reason": "pointer is a fresh payload whose align >= 8 block reserved the word 24 bytes before it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 209,
      "column": 5,
      "reason": "pointer is an align >= 8 payload being freed with its allocated size; its block reserved the count word 24 bytes before it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 241,
      "column": 5,
      "reason": "anonymous private mmap owns fresh writable bytes and cannot alias a live heap block; a failed syscall produces no allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 261,
      "column": 5,
      "reason": "the caller owns this mapping and uses no pointer into it again"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 271,
      "column": 5,
      "reason": "a fresh shared mapping of the descriptor aliases no heap block; a failed syscall maps nothing"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 283,
      "column": 5,
      "reason": "the caller owns the reservation the fixed mapping replaces, and a shared mapping of the descriptor aliases no heap block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 291,
      "column": 5,
      "reason": "the caller owns these whole pages of its own mapping and keeps no live value in them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 331,
      "column": 5,
      "reason": "the root and span tables are process-lifetime mappings, zero where nothing is registered, whose aligned words are read atomically on x86"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 362,
      "column": 5,
      "reason": "the caller holds the page lock, which serializes every table write; a fresh table is zero-filled before the aligned store that publishes it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 387,
      "column": 5,
      "reason": "the root and span tables of the range are mapped for the process's lifetime, and each unit's aligned word is written only by the range's owner"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 470,
      "column": 5,
      "reason": "a fresh inaccessible NORESERVE mapping aliases nothing; its link and record units are remapped writable inside it and its span tables mapped before its base is published, and a caller that loses the publication unmaps only its own mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 515,
      "column": 9,
      "reason": "row is the running TCB's own run stack of this class, and the link word of a run on it is this thread's alone"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 553,
      "column": 5,
      "reason": "the fresh run is this caller's alone, inside the window and inaccessible; the fixed mapping replaces only its own units"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 590,
      "column": 5,
      "reason": "row is the running TCB's own run stack of this class, and the ended run's link word is no other thread's"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 671,
      "column": 5,
      "reason": "the region has ended, so nothing reads or writes its pages; MADV_DONTNEED drops only its own units inside the window, which read as zero again"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 765,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 768,
      "column": 5,
      "reason": "the fresh state page is private to initialization, which precedes every allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 784,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 813,
      "column": 5,
      "reason": "initialization wrote the arena table word once, before any allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 828,
      "column": 5,
      "reason": "reads the stack pointer register only; nothing is dereferenced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 842,
      "column": 9,
      "reason": "the live totals are process-lifetime arena words; the quiescent caller needs no lock to read them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 871,
      "column": 5,
      "reason": "root is an aligned lock word in the process-lifetime state mapping; CAS/xchg acquire all metadata it protects and the futex wait only reads that word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 890,
      "column": 5,
      "reason": "the calling thread holds this lock; xchg publishes its completed metadata updates and reports sleepers, and the futex wake only names the word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 931,
      "column": 5,
      "reason": "slab and its linked neighbors are live heap-owned headers protected by the held heap lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 949,
      "column": 5,
      "reason": "head and slab are live heap-owned metadata protected by the held heap lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 985,
      "column": 5,
      "reason": "the slab-region lock owns the cursor, end and free-slab stack words and every stacked slab's link word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1021,
      "column": 5,
      "reason": "the slab-region lock owns the free-slab stack; the slab is unlinked from every class list and holds no live or cached block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1037,
      "column": 5,
      "reason": "madvise only changes the page size of the fresh private region"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1049,
      "column": 5,
      "reason": "the slab is this arena's alone: fresh from its region or off the free-slab stack; the held arena lock excludes all list readers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1075,
      "column": 5,
      "reason": "the held arena lock protects the class list, the slab's free links, carve word and live count, the retained count and the arena live total; row is the caller's exclusive cache row; the blocks leave the slab before any caller sees them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1124,
      "column": 5,
      "reason": "the held arena lock protects the slab's free links, carve word, live count, class list, retained count and live total; the block is no longer any payload or cache entry"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1157,
      "column": 5,
      "reason": "the held arena lock makes the arena's scratch row exclusive; it is emptied again before the lock is released"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1166,
      "column": 5,
      "reason": "block is this caller's exclusive block; its two cached words are read before the payload header overwrites them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1181,
      "column": 5,
      "reason": "row is this thread's private cache row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1199,
      "column": 5,
      "reason": "row is this thread's private cache row; the flag is idempotent: every writer stores the same 1"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1214,
      "column": 5,
      "reason": "row is this thread's private cache row and block its cached head; both cached words are read before the payload header overwrites them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1229,
      "column": 5,
      "reason": "row is this thread's private cache row; each cached block's slab is live while the block is cached, so its arena word is readable, and the block returns under that arena's lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1260,
      "column": 9,
      "reason": "row is one of this thread's eight private cache rows"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1274,
      "column": 5,
      "reason": "tcb is a live TCB whose eight cache rows start at @320"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1283,
      "column": 9,
      "reason": "row is one of the TCB's eight cache rows, which only the thread running on it touches"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1308,
      "column": 13,
      "reason": "row is one of the running TCB's eight private cache rows"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1351,
      "column": 9,
      "reason": "row is one of the switched-out fiber's eight cache rows, which no thread runs on now"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1440,
      "column": 9,
      "reason": "row is one of the running TCB's kept-mapping rows, which only the thread running on it touches"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1462,
      "column": 9,
      "reason": "row is one of the running TCB's kept-mapping rows, which only the thread running on it touches"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1479,
      "column": 5,
      "reason": "the large-cache lock owns every class list; the mapping's first word is no live payload once it is freed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1540,
      "column": 5,
      "reason": "the large-cache lock owns every class list and its link words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1580,
      "column": 5,
      "reason": "pointer's two header words and the aligned payload are wholly within this page-rounded class mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1603,
      "column": 5,
      "reason": "pointer is a live direct allocation of exactly `size` bytes at align 1 whose two header words name its mapping and the mapping's bytes; the mode and tracked-count words are aligned words read atomically on x86, as alloc_reserve and alloc_untrack read them; a refused mremap leaves the mapping as it was"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1643,
      "column": 9,
      "reason": "row is this thread's private cache row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1662,
      "column": 5,
      "reason": "the caller holds the policy lock and cell is a counter in alloc_root"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1670,
      "column": 5,
      "reason": "the aligned mode word is read atomically on x86; system allocation needs no policy lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1676,
      "column": 5,
      "reason": "the policy lock exclusively owns the mode, cap and accounting words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1705,
      "column": 5,
      "reason": "the policy lock exclusively owns live bytes; restoration does not erase outstanding tracked blocks"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1719,
      "column": 5,
      "reason": "the held policy lock serializes all native allocator configuration and reservations"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1736,
      "column": 5,
      "reason": "the policy lock serializes restoration with native reserve and free; tracked blocks remain in the side table"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1750,
      "column": 5,
      "reason": "the policy lock protects this bounded fact word in alloc_root"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1765,
      "column": 5,
      "reason": "the private raw-heap node is fully initialized before linking under the policy/tracker lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1778,
      "column": 5,
      "reason": "the process-lifetime tracker count is an aligned x86 word; a zero count avoids any lookup or lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 1785,
      "column": 5,
      "reason": "the policy/tracker lock exclusively owns all side-table links and count"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2085,
      "column": 5,
      "reason": "reads the stack pointer register only; nothing is dereferenced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2338,
      "column": 13,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row (TCB @320 + 16 per class, alloc_cache_row) and block its cached head, whose two cached words are read before the payload header overwrites them; an align <= 8 payload at block + header is aligned because blocks start 32-aligned"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2385,
      "column": 13,
      "reason": "as in jet_rt_alloc: GS:0 is the attached thread's TCB or the readable zero word; the aligned mode word is read atomically on x86; row is this thread's private cache row and block its cached head, whose two cached words are read before the payload header overwrites them; an align <= 8 payload at block + header is aligned because blocks start 32-aligned"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2421,
      "column": 13,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count and the page-map root are aligned words read atomically, as alloc_untrack and alloc_page_region read them; row is this thread's private cache row; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2446,
      "column": 5,
      "reason": "pointer is an align-8 payload being freed; its block reserved the count word 24 bytes before it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2486,
      "column": 17,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row of the size's class and block its cached head, whose next word is read before the count overwrites it; the block's second word is its slab word, the payload header's; the payload at block + 24 is 8-aligned because blocks start 32-aligned"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2519,
      "column": 21,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count and the page-map root are aligned words read atomically, as alloc_untrack and alloc_page_region read them; row is this thread's private cache row of the size's class; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's next word overwrites it; the block's second word stays its slab word; a count word with the zero-at-free mark takes the general entry, which wipes the payload"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2555,
      "column": 13,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row (TCB @320 + 16 per class, alloc_cache_row) and block its cached head, whose two cached words are read before the payload header overwrites them; an align <= 8 payload at block + header is aligned because blocks start 32-aligned"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2600,
      "column": 17,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row of the size's class and block its cached head, whose next word is read before the count overwrites it; the block's second word is its slab word, the payload header's; the payload at block + 24 is 8-aligned because blocks start 32-aligned"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2643,
      "column": 13,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row of the size's class and block its cached head, whose two cached words are read before the payload header overwrites them; an align-1 payload at block + 16 needs no alignment"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2671,
      "column": 13,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count and the page-map root are aligned words read atomically, as alloc_untrack and alloc_page_region read them; row is this thread's private cache row of the size's class; the caller frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2709,
      "column": 9,
      "reason": "row is this thread's private cache row; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2725,
      "column": 5,
      "reason": "the lowering returns this still-live direct allocation with exactly its original size and alignment"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2737,
      "column": 5,
      "reason": "the caller's live payload header names its slab, whose arena word is fixed when the slab is created"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2742,
      "column": 5,
      "reason": "the lowering frees exactly once with the allocation's original layout; its slab remains live until this last payload is returned under its arena's lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2808,
      "column": 5,
      "reason": "the fresh state page is private to initialization, which precedes every allocation"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2811,
      "column": 5,
      "reason": "checked-heap start runs during runtime-init, before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2825,
      "column": 5,
      "reason": "the state page's stub word, written only here"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2832,
      "column": 5,
      "reason": "the fresh stub page is private until its address is stored; it becomes read+execute before any call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2851,
      "column": 5,
      "reason": "the frame stub (alloc_frames_start) only copies rbp to rax and returns; a missing stub leaves no chain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2859,
      "column": 13,
      "reason": "frame is an active caller's [saved rbp][return address] pair on this thread's stack: each followed link ascends by less than 16 MiB or crosses a stack segment's bridge frame"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2864,
      "column": 9,
      "reason": "out lends count writable words: a checked block's chain words or a trap's scratch page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2879,
      "column": 5,
      "reason": "the frame stub (alloc_frames_start) only copies rbp to rax and returns; a missing stub leaves no chain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2888,
      "column": 9,
      "reason": "frame is an active caller's [saved rbp][return address] pair on this thread's stack: each followed link ascends by less than 16 MiB or crosses a stack segment's bridge frame"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2895,
      "column": 13,
      "reason": "into lends count writable words: a host call row's site words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2914,
      "column": 5,
      "reason": "block is this caller's fresh heap block of prefix + size + 8 bytes; every word written lies in block .. pointer + size + 8"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2954,
      "column": 5,
      "reason": "a checked payload's canary word precedes it; the quarantine lock orders racing frees of one block and guards the live list"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2968,
      "column": 5,
      "reason": "the live canary proves pointer is a checked payload whose header and tail this free now owns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 2985,
      "column": 5,
      "reason": "the quarantine lock owns the ring and its counters; every parked block is freed and owned by the quarantine"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3020,
      "column": 5,
      "reason": "aligned whole-word stores; each checked take or give loads its observer word once"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3032,
      "column": 5,
      "reason": "the observation start runs before the entry; a take links a block only once the entry starts the list"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3047,
      "column": 5,
      "reason": "an aligned whole-word store; a take on the observation's writer thread that loads the switch before it links no block, which its free leaves alone"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3059,
      "column": 5,
      "reason": "an aligned whole-word store; a take that loads the word before it is a take of another thread's"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3074,
      "column": 5,
      "reason": "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); only an attached TCB's own-takes word is read"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3096,
      "column": 5,
      "reason": "GS:0 is the attached thread's TCB, whose own-takes word only this thread writes; an unattached thread's takes link no block, so it keeps no count"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3107,
      "column": 5,
      "reason": "pointer is this take's fresh checked payload, whose freeing-chain words are unused while it lives; the held quarantine lock guards the list and its blocks' link words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3120,
      "column": 5,
      "reason": "the live canary proves pointer a checked payload; the held quarantine lock guards the list and its blocks' link words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3138,
      "column": 5,
      "reason": "the held quarantine lock guards the count"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3154,
      "column": 1,
      "reason": "pointer is 0 or a tracked live payload of the held live list"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3161,
      "column": 1,
      "reason": "pointer is a tracked live payload of the held live list"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3172,
      "column": 1,
      "reason": "the caller holds the live list (alloc_check_track_hold), or is a snapshot's forked child, whose one thread is the holder, and puts the head back before it lets the list go"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3184,
      "column": 5,
      "reason": "the frame stub (alloc_frames_start) only copies rbp to rax and returns; a missing stub leaves no chain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3190,
      "column": 5,
      "reason": "frame is this call's live [saved rbp][return address] pair on this thread's stack"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3198,
      "column": 5,
      "reason": "the freeing caller owns size payload bytes at pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3213,
      "column": 5,
      "reason": "pointer is a parked checked payload owned by the quarantine; its header, payload and tail stay mapped until retired"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3240,
      "column": 5,
      "reason": "the freed canary proved the checked header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3249,
      "column": 5,
      "reason": "pointer is an inspected parked checked payload; its header still names the enclosing block and layout"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3280,
      "column": 5,
      "reason": "the quarantine lock owns the ring and its counters"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3296,
      "column": 5,
      "reason": "data lends length readable bytes; write retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3333,
      "column": 5,
      "reason": "path is a fresh private page; the image is mapped read-only and only read"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3357,
      "column": 5,
      "reason": "image is a read-only private mapping of this executable's ELF64 file; its program header table lies inside it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3386,
      "column": 9,
      "reason": "image is a read-only private mapping of this executable's ELF64 file; every offset read comes from its own section and symbol tables"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3429,
      "column": 9,
      "reason": "words lends count readable chain words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3475,
      "column": 9,
      "reason": "the live canary proved the checked header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3524,
      "column": 5,
      "reason": "remaining is in 1..=8, so write borrows only the initialized stack word; a short write advances within it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Alloc.jet",
      "line": 3583,
      "column": 5,
      "reason": "the stack-word syscalls unblock SIGABRT and, if necessary, restore its default disposition before raising it again; no heap pointer is needed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Authority.jet",
      "line": 399,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CABISession.jet",
      "line": 21,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CABISession.jet",
      "line": 27,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Channel.jet",
      "line": 198,
      "column": 5,
      "reason": "an idempotent flag: every writer stores the same 1"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerFS.jet",
      "line": 266,
      "column": 9,
      "reason": "the prefix remains terminated and scratch lends 128 writable portable stat-fact bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerFS.jet",
      "line": 288,
      "column": 5,
      "reason": "name is NUL-terminated and scratch lends 128 writable portable stat-fact bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerHost.jet",
      "line": 12,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerHost.jet",
      "line": 164,
      "column": 5,
      "reason": "page is a fresh private page: the path, then the [type, value] pairs read into it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 59,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 260,
      "column": 5,
      "reason": "the descriptor is this map's own; mmap creates exactly length read-only private bytes; the map owner alone unmaps them"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 273,
      "column": 5,
      "reason": "the map owner unmaps its own mapping once, after its pages left the sentry's live storage"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 282,
      "column": 5,
      "reason": "closes a temp-file descriptor this map alone owns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 290,
      "column": 5,
      "reason": "fcntl releases the lease on the map's own descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 301,
      "column": 5,
      "reason": "fstatfs writes one 120-byte statfs into the map's 160-byte scratch"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 348,
      "column": 5,
      "reason": "FICLONE shares the file's blocks with the empty unnamed spare this map owns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 366,
      "column": 9,
      "reason": "copy_file_range and sendfile move bytes between two descriptors this map owns; the offset word is the caller's"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 380,
      "column": 5,
      "reason": "ftruncate sizes the unnamed temp file this map owns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 417,
      "column": 5,
      "reason": "fcntl names the lease-break signal for, then takes a read lease on, the map's own read-only descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 442,
      "column": 9,
      "reason": "rt_sigaction borrows the four-word action on the stack; the handler is this runtime's three-word C-ABI export and the restorer its rt_sigreturn rail"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 446,
      "column": 9,
      "reason": "written under the map registry mutex, before any lease can name a slot"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 509,
      "column": 9,
      "reason": "memfd_create reads the slot's NUL-terminated name word; MFD_CLOEXEC"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CompilerMapped.jet",
      "line": 531,
      "column": 5,
      "reason": "MAP_FIXED replaces exactly the held map's own read-only private pages with the same length of its copy; borrowed views keep their addresses"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Comptime.jet",
      "line": 18,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ComptimeServe.jet",
      "line": 95,
      "column": 5,
      "reason": "the filter is installed once, on the image's only thread, before any call it traps"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ComptimeServe.jet",
      "line": 181,
      "column": 13,
      "reason": "the compiler admits between calls of the one thread it drives; the handler only reads the flag"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ComptimeServe.jet",
      "line": 196,
      "column": 5,
      "reason": "the serve entry runs once, on the image's only thread, before it serves"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Context.jet",
      "line": 363,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Coverage.jet",
      "line": 25,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CryptoAES.jet",
      "line": 26,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CryptoAES.jet",
      "line": 71,
      "column": 5,
      "reason": "the caller holds (rounds + 1) round keys at keys and four blocks at src, mask and out"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CryptoAES.jet",
      "line": 99,
      "column": 5,
      "reason": "the caller holds (rounds + 1) round keys at keys and one block at src, pre, post, out and chain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CryptoAES.jet",
      "line": 246,
      "column": 5,
      "reason": "y and h hold one block, the range check keeps every block inside the length borrowed bytes, and the powers table holds four"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CryptoEntropy.jet",
      "line": 23,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/CryptoEntropy.jet",
      "line": 69,
      "column": 5,
      "reason": "Linux getrandom flags0 writes only the exclusively borrowed writable span and retains no address; kernel negative errno is preserved"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/DynCodec.jet",
      "line": 137,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/DynCodec.jet",
      "line": 167,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/EnvOverlay.jet",
      "line": 40,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 63,
      "column": 5,
      "reason": "the image's entry calls this before it starts any other thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 78,
      "column": 9,
      "reason": "jet_rt_main calls this on the main thread before the entry runs"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 97,
      "column": 5,
      "reason": "rt_sigaction borrows the four-word action on the stack; handler is this runtime's C-ABI export with the (signal, info, context) signature"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 110,
      "column": 5,
      "reason": "stack is a fresh private mapping; sigaltstack copies its first three words (ss_sp, ss_flags, ss_size)"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 130,
      "column": 5,
      "reason": "stack is this thread's registered alternate stack, unused outside a handler; sigaltstack copies its first three words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 177,
      "column": 5,
      "reason": "rt_sigaction borrows the four-word SIG_DFL action on the stack; tgkill signals this thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 234,
      "column": 5,
      "reason": "stub is a fresh private page that holds `mov rax, rbp; ret` and becomes read+execute before the call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 256,
      "column": 9,
      "reason": "path is a fresh private page; the executable is mapped read-only and only read"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 383,
      "column": 5,
      "reason": "the trace made this mapping and holds no pointer into it past this call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 473,
      "column": 5,
      "reason": "image is a read-only mapping of this executable's ELF64 file; every offset read comes from its own header and section table"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 508,
      "column": 9,
      "reason": "frame is an active caller's [saved rbp][return address] pair on this thread's stack"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 515,
      "column": 9,
      "reason": "each followed link ascends by less than 16 MiB or crosses a stack segment's bridge frame, so frame stays on this thread's chain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 550,
      "column": 9,
      "reason": "frame is an active caller's [saved rbp][return address] pair on this thread's stack"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 563,
      "column": 9,
      "reason": "each followed link ascends by less than 16 MiB or crosses a stack segment's bridge frame, so frame stays on this thread's chain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 633,
      "column": 9,
      "reason": "block + 12 is inside this block, which lies in the read-only table"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fault.jet",
      "line": 726,
      "column": 5,
      "reason": "at lies in a mapping the trace reads: the executable's read-only ELF header, program or section table, notes or location table, or the page the host's frames were written to"
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
      "line": 34,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fiber.jet",
      "line": 133,
      "column": 5,
      "reason": "the carrier saved its stack at carrier+16 when it switched to this fiber and runs nothing else until this switch resumes it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fiber.jet",
      "line": 160,
      "column": 5,
      "reason": "sched_yield retains no memory and yields this OS thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fiber.jet",
      "line": 230,
      "column": 9,
      "reason": "base is a live self-addressed TCB this carrier runs on until its next switch, and the kernel reported FSGSBASE"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fiber.jet",
      "line": 266,
      "column": 9,
      "reason": "the fiber's saved stack is owned by no running thread: it is fresh, pooled or committed PARKED/RUNNABLE, and this carrier alone took it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fiber.jet",
      "line": 349,
      "column": 9,
      "reason": "an idempotent install: every writer stores 1, and a carrier that read the old 0 left its slice unstamped, which the read below stamps"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fiber.jet",
      "line": 476,
      "column": 13,
      "reason": "written once, under the scheduler root lock, before any fiber can wait; a reader sees 0 (no fiber waiter exists) or the lot"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fiber.jet",
      "line": 527,
      "column": 9,
      "reason": "a fresh private anonymous stack mapping, unreserved until touched"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fiber.jet",
      "line": 539,
      "column": 5,
      "reason": "the first page of the fresh, untouched mapping becomes the stack's guard: a guard marker, else PROT_NONE; madvise only changes the mapping's page size"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Fiber.jet",
      "line": 694,
      "column": 9,
      "reason": "sched_yield retains no memory; the firing carrier clears the word right after its wake"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/FilesPathOps.jet",
      "line": 38,
      "column": 5,
      "reason": "the terminated raw path and the 128-byte stat fact row are live for the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/FilesStreams.jet",
      "line": 34,
      "column": 9,
      "reason": "the freshly opened owned descriptor and the 128-byte stat fact row are live for fstat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/FilesWalk.jet",
      "line": 46,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 28,
      "column": 5,
      "reason": "TCGETS writes its termios into the fresh mapping's scratch at 32 and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 32,
      "column": 5,
      "reason": "runtime startup publishes the root before any program thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 41,
      "column": 5,
      "reason": "rt_sigaction synchronously borrows the four-word SIG_IGN action; no signal handler or restorer runs for ignored SIGPIPE"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 58,
      "column": 5,
      "reason": "the caller lends length readable bytes for the complete synchronous write"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 75,
      "column": 5,
      "reason": "the caller lends length readable bytes; writev borrows its own LF until return"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 104,
      "column": 5,
      "reason": "cpuid only reports processor identification"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 124,
      "column": 5,
      "reason": "the output lock exclusively owns the initialized buffer and length; write borrows its pending bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 244,
      "column": 5,
      "reason": "the held host lock lends enough scratch for Linux TCGETS"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Host.jet",
      "line": 314,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostDescriptors.jet",
      "line": 40,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostDescriptors.jet",
      "line": 180,
      "column": 9,
      "reason": "bytes owns cap writable bytes; pread retains no pointer and the caller's Fd keeps the descriptor open"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostDescriptors.jet",
      "line": 373,
      "column": 9,
      "reason": "scratch lends the pollfd and timespec and ppoll retains neither; the caller's Fd keeps the descriptor open"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostDescriptors.jet",
      "line": 400,
      "column": 9,
      "reason": "bytes owns cap writable bytes; read retains no pointer and the caller's Fd keeps the descriptor open"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostDescriptors.jet",
      "line": 436,
      "column": 9,
      "reason": "packed lends count bytes; write retains no pointer and the caller's Fd keeps the descriptor open"
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
      "line": 47,
      "column": 9,
      "reason": "an owned fresh directory descriptor and writable getdents64 buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostDirectoryList.jet",
      "line": 82,
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
      "line": 287,
      "column": 9,
      "reason": "the caller lends 128 writable bytes for portable stat facts"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 383,
      "column": 5,
      "reason": "the terminated borrowed path and 128-byte stat fact output are live for the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 451,
      "column": 9,
      "reason": "the held directory and terminated source-approved single name stay live through openat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 474,
      "column": 9,
      "reason": "the held directory and terminated final name live through O_PATH|O_NOFOLLOW|O_CLOEXEC openat; no read access or link traversal"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 482,
      "column": 9,
      "reason": "openat returned an exclusively owned descriptor and the caller lends StatFacts128; fstat copies initialized scalar facts"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 512,
      "column": 9,
      "reason": "the descriptor is held live, output begins with the borrowed empty pathname, statx writes only its private stack frame"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 523,
      "column": 5,
      "reason": "the terminated source-approved path and three writable birth-time output words stay live through statx"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 581,
      "column": 5,
      "reason": "both terminated borrowed names and the caller's optional compact byte output remain live only through lgetxattr"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 613,
      "column": 9,
      "reason": "the held directory, terminated plain name and writable byte range are borrowed only through readlinkat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 641,
      "column": 5,
      "reason": "held directory duplicates and terminated names prevent any descriptor-close/reuse race during renameat2"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostFiles.jet",
      "line": 665,
      "column": 9,
      "reason": "the held fd and stack-local source-validated winsize word live only through ioctl"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 18,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 215,
      "column": 13,
      "reason": "the owned walk descriptor and terminated part live through openat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 280,
      "column": 13,
      "reason": "the walk directory and terminated final part live through openat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 446,
      "column": 5,
      "reason": "the terminated borrowed path and the 256-byte statx buffer are live for the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 550,
      "column": 13,
      "reason": "the walk directory and terminated final part live through O_PATH|O_NOFOLLOW|O_CLOEXEC openat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 556,
      "column": 17,
      "reason": "the owned O_PATH descriptor and the caller's 128-byte stat facts live through fstat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 563,
      "column": 21,
      "reason": "the owned O_PATH descriptor, birth's zeroed empty pathname and three writable words live through statx"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 642,
      "column": 5,
      "reason": "both walk directories and terminated final parts live through renameat/renameat2"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostHeldDirectory.jet",
      "line": 656,
      "column": 9,
      "reason": "both walk directories and terminated final parts live through linkat"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetLookup.jet",
      "line": 271,
      "column": 5,
      "reason": "socket(AF_UNIX, SOCK_DGRAM|SOCK_CLOEXEC, 0) takes no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetLookup.jet",
      "line": 286,
      "column": 5,
      "reason": "ioctl(SIOCGIFINDEX) reads and writes exactly the owned 40-byte ifreq; close releases the descriptor opened here"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetLookup.jet",
      "line": 348,
      "column": 5,
      "reason": "open(2) reads the owned NUL-terminated path; O_RDONLY|O_CLOEXEC retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetLookup.jet",
      "line": 364,
      "column": 5,
      "reason": "close(2) releases the descriptor lk_open opened"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetLookup.jet",
      "line": 380,
      "column": 9,
      "reason": "read(2) fills at most LK_READ bytes of the reader's owned buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetPoller.jet",
      "line": 45,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetPoller.jet",
      "line": 104,
      "column": 5,
      "reason": "the shard lock owns a packed 12B epoll_event; state retains the native fd through this synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostNetPoller.jet",
      "line": 140,
      "column": 9,
      "reason": "events owns 64 packed epoll_event entries; epoll_wait retains no buffer pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostParallelism.jet",
      "line": 486,
      "column": 5,
      "reason": "sched_getaffinity writes at most the owned 128-byte mask; pid0 names this thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostParallelism.jet",
      "line": 563,
      "column": 9,
      "reason": "the probe answer is fixed for the process; a racing probe stores the same word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostParallelism.jet",
      "line": 578,
      "column": 9,
      "reason": "xgetbv only reports the OS-enabled state, which CPUID.1:ECX has just shown the OS reports (OSXSAVE)"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 8,
      "column": 5,
      "reason": "the audited caller supplies the Linux syscall's valid word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 36,
      "column": 1,
      "reason": "a raw Linux x86-64 syscall; the caller supplies the syscall's valid machine-word arguments"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 213,
      "column": 5,
      "reason": "rt_sigaction borrows the four-word SIG_DFL action on the stack and writes no old action"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 255,
      "column": 5,
      "reason": "buffer owns the sixteen-byte struct rlimit64 the kernel writes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 283,
      "column": 5,
      "reason": "buffer owns the sixteen-byte struct rlimit64 the kernel reads and does not retain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostProcess.jet",
      "line": 329,
      "column": 5,
      "reason": "status lends an eight-byte out slot; wait4 writes its low four bytes and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 14,
      "column": 9,
      "reason": "the FD72 lock prevents native descriptor close/reuse throughout this nonblocking syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 62,
      "column": 13,
      "reason": "accept4 retains no address pointers and atomically sets CLOEXEC/NONBLOCK on the accepted socket"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 69,
      "column": 13,
      "reason": "private scratch lends sockaddr bytes and its exact 4B socklen; accept4 retains neither pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 108,
      "column": 9,
      "reason": "private scratch lends one native int and its 4B socklen; getsockopt retains neither pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 124,
      "column": 9,
      "reason": "private scratch lends exactly one native int; setsockopt retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 166,
      "column": 9,
      "reason": "the held FD72 lock lends its live nonblocking socket; packed lends exactly length bytes; no pointer survives recvfrom/sendto"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 208,
      "column": 9,
      "reason": "the FD72 lock retains the socket; private scratch lends sockaddr bytes; packed lends length bytes; sendto retains no pointers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSockets.jet",
      "line": 228,
      "column": 9,
      "reason": "private scratch lends sockaddr/socklen and packed lends length mutable bytes; MSG_TRUNC returns the complete datagram length while writing at most length bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 333,
      "column": 13,
      "reason": "rt_sigaction borrows four initialized stack words; no allocator, mutex or TLS is touched after fork"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 440,
      "column": 13,
      "reason": "rt_sigaction writes the four-word old action into this spawn's block; no allocator, mutex or TLS is touched in the child"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 450,
      "column": 5,
      "reason": "rt_sigprocmask reads the parent's saved mask word in this spawn's block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 505,
      "column": 5,
      "reason": "the masks are words of this spawn's block; the child runs only jet_rt_process_spawn_child on the mapped stack and leaves through execve or exit_group before the parent resumes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 904,
      "column": 9,
      "reason": "ioctl borrows one stack word of source-validated winsize dimensions; no pointer escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 987,
      "column": 21,
      "reason": "readlink borrows the NUL-terminated output path and writes at most p (or the one dummy) bytes of the owned stack"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 1028,
      "column": 5,
      "reason": "getcwd writes at most 4097 bytes of the owned stack"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostSpawn.jet",
      "line": 1087,
      "column": 5,
      "reason": "source lends a NUL-terminated raw path including non-Unicode bytes and a writable 128-byte stat fact row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 38,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 101,
      "column": 5,
      "reason": "host argument registration is single-threaded and precedes user code"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 112,
      "column": 5,
      "reason": "host argument registration is single-threaded and precedes user code"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 144,
      "column": 5,
      "reason": "registration is single-threaded and precedes user code"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartup.jet",
      "line": 191,
      "column": 5,
      "reason": "the index is published once: by single-threaded registration, or inside host_environment_require's once before it completes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostStartupOutputFacts.jet",
      "line": 113,
      "column": 5,
      "reason": "Host startup already initialized its process-lifetime mapping; the held lock owns ioctl scratch32..47 and FIRST raw lookup only borrows the immutable captured index"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostTime.jet",
      "line": 19,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/HostTiming.jet",
      "line": 28,
      "column": 9,
      "reason": "the probe answer is fixed for the process; a racing probe stores the same word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Int.jet",
      "line": 459,
      "column": 13,
      "reason": "the final owner has reached zero; the remaining shared hazard domain decides immediate free or retirement"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/IntHazard.jet",
      "line": 21,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/JourneyTLS.jet",
      "line": 26,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/LogicalEnvironment.jet",
      "line": 123,
      "column": 5,
      "reason": "host argument registration is single-threaded and precedes user code"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 27,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 55,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 71,
      "column": 5,
      "reason": "an idempotent flag: every writer stores the same 1"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 86,
      "column": 5,
      "reason": "a whole-word setting: each setter stores one aligned word and the last setter wins, which is what setting it means"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 129,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 135,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 144,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 220,
      "column": 5,
      "reason": "only the main thread running jet_rt_main writes its root control"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 312,
      "column": 5,
      "reason": "only the main thread running jet_rt_main writes its root control"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 316,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 328,
      "column": 5,
      "reason": "only the main thread running jet_rt_main writes its root control"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Main.jet",
      "line": 355,
      "column": 5,
      "reason": "the entry has returned and drained its workers; only the main thread runs here"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/MemRegistry.jet",
      "line": 22,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NativeLayoutABI.jet",
      "line": 98,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NativeLayoutText.jet",
      "line": 65,
      "column": 5,
      "reason": "runtime-init (jet_rt_layout_init) runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NetUnix.jet",
      "line": 431,
      "column": 5,
      "reason": "private scratch lends one native int and its 4B socklen; getsockopt retains neither pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/NetUnix.jet",
      "line": 551,
      "column": 13,
      "reason": "the FD72 lock keeps the stream's descriptor live and unreused for the whole ppoll; the caller's Fd keeps the wake descriptor open; scratch lends the pollfds and timespec"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Observe.jet",
      "line": 29,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Observe.jet",
      "line": 35,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Observe.jet",
      "line": 235,
      "column": 5,
      "reason": "an idempotent flag: every writer stores the same 1"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Observe.jet",
      "line": 485,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ObserveLive.jet",
      "line": 83,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ObserveLive.jet",
      "line": 184,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ObserveLive.jet",
      "line": 234,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ObserveLive.jet",
      "line": 403,
      "column": 5,
      "reason": "the vetted Linux calls of Observe and the profiler take words alone or borrow native buffers only for their duration"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ObserveLive.jet",
      "line": 708,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 711,
      "column": 5,
      "reason": "the observation start runs before the timer whose handler reads these is armed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 729,
      "column": 9,
      "reason": "no timer is armed yet, so no handler reads these before they are set"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 743,
      "column": 9,
      "reason": "no timer is armed, so no handler reads the descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 757,
      "column": 5,
      "reason": "only the exit reads it, and no exit runs before the start returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 780,
      "column": 5,
      "reason": "rt_sigaction borrows the four-word action on the stack; the handler is this runtime's three-word C-ABI export and the restorer its rt_sigreturn rail"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 820,
      "column": 5,
      "reason": "clock_gettime writes the sampler's two words of the thread-owned TCB; no pointer escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 878,
      "column": 5,
      "reason": "timer_create reads the sigevent and writes the timer id in the scratch words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 896,
      "column": 13,
      "reason": "timer_settime reads the itimerspec in the scratch words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 984,
      "column": 5,
      "reason": "only the observation writer changes it, under the flight recorder's lock; a starting thread reads it again after it arms"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 1001,
      "column": 17,
      "reason": "timer_settime reads the itimerspec this call owns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 1031,
      "column": 5,
      "reason": "set once, before the table is published; a host call or spawn that reads perf_system 0 keeps no row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 1312,
      "column": 9,
      "reason": "the table and the depth are set before the first sampler is armed, so no handler reads them before"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 1322,
      "column": 17,
      "reason": "no sampler is armed yet, so no thread reads the source or its period"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 1329,
      "column": 21,
      "reason": "no sampler is armed yet, so no thread reads the source"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 1338,
      "column": 13,
      "reason": "no sampler is armed yet, so no thread reads the source"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 1345,
      "column": 13,
      "reason": "no sampler is armed, so no handler reads the table"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 1356,
      "column": 9,
      "reason": "eight writes of a stack-local word, which the kernel copies before each call returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 1414,
      "column": 5,
      "reason": "perf_event_open reads the attribute words of the scratch"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 1496,
      "column": 5,
      "reason": "perf_event_open reads the attribute words of the scratch"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 2389,
      "column": 9,
      "reason": "one write of a stack-local word, which the kernel copies before the call returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 2589,
      "column": 5,
      "reason": "the exit and a window's close, under the flight recorder's lock, alone clear it; a handler reads it only while it holds its buffer, which the claims below order"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 2624,
      "column": 5,
      "reason": "clock_gettime writes the two timespec words of the scratch"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 2832,
      "column": 9,
      "reason": "registrations never overlap, since constructors run one at a time, and the fence below orders this store before a turn reads the tables"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 2876,
      "column": 9,
      "reason": "registrations never overlap, and a turn reads the page only once it is stored"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 2891,
      "column": 5,
      "reason": "the observation start, before the entry, and then its writer store the same roots"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 2907,
      "column": 9,
      "reason": "turns run one at a time, the observation start's before the entry and then its writer's, and a registration reads the code only after the zones word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 2915,
      "column": 9,
      "reason": "turns run one at a time, the observation start's before the entry and then its writer's, and a zone route reads the rings' head and a registration the lock only after the zones word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 2920,
      "column": 5,
      "reason": "turns run one at a time; the lock orders their patches with a registration's"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3026,
      "column": 5,
      "reason": "pwrite64 copies one byte from the thread-owned TCB scratch and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3072,
      "column": 5,
      "reason": "rdtsc only reads the time-stamp counter, which the zones' calibration found invariant and enabled for this process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3089,
      "column": 5,
      "reason": "the first turn of zones runs alone, before it stores the zones word that every zone route reads first"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3123,
      "column": 9,
      "reason": "x86 pause only hints that this thread is waiting in a spin loop"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3631,
      "column": 5,
      "reason": "fork copies the held live list into a child whose only thread writes the snapshot and exits, never returning from here"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3679,
      "column": 5,
      "reason": "this thread holds the live list, or is the fork's child, whose copy of it its one thread held"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3690,
      "column": 9,
      "reason": "block is a tracked live payload of the held live list"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3703,
      "column": 9,
      "reason": "block is a tracked live payload of the held live list"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3761,
      "column": 5,
      "reason": "this thread holds the live list, or is the fork's child, whose copy of it its one thread held"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3774,
      "column": 5,
      "reason": "this thread holds the live list, or is the fork's child, whose copy of it its one thread held"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3785,
      "column": 5,
      "reason": "this thread holds the live list, or is the fork's child, whose copy of it its one thread held"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3790,
      "column": 9,
      "reason": "block is a tracked live payload of the held live list"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3816,
      "column": 9,
      "reason": "block is a tracked live payload of the held live list"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3847,
      "column": 9,
      "reason": "link is a live [saved rbp][return address] pair of this thread's frame pointer chain, above the snapshot's own frames"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3874,
      "column": 9,
      "reason": "link is a live [saved rbp][return address] pair of this thread's frame pointer chain, above the snapshot's own frames"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3906,
      "column": 5,
      "reason": "this thread holds the live list, or is the fork's child, whose copy of it its one thread held; the head goes back below"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 3946,
      "column": 5,
      "reason": "the head this scan took from the live list goes back before the list is let go"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 4126,
      "column": 5,
      "reason": "the observation start runs before the program's entry, whose takes read it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 4222,
      "column": 5,
      "reason": "the held quarantine lock guards the count; every take reads it under the lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 4290,
      "column": 5,
      "reason": "the observation start runs before the entry, whose span closes read it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 4689,
      "column": 9,
      "reason": "two writes of a stack-local word, which the kernel copies before each call returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 4743,
      "column": 5,
      "reason": "fcntl reads the two-word f_owner_ex built at `owner`"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 4771,
      "column": 5,
      "reason": "perf_event_open reads the attribute words the held buffer keeps"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 5028,
      "column": 5,
      "reason": "five writes of a stack-local word, which the kernel copies before each call returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 5068,
      "column": 9,
      "reason": "four writes of a stack-local word, which the kernel copies before each call returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 5077,
      "column": 5,
      "reason": "the observation start runs before the exit that reads these"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 5092,
      "column": 5,
      "reason": "the exit runs on one thread, after the start set these"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 5677,
      "column": 5,
      "reason": "the observation start runs before the timer whose handler reads these is armed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 5769,
      "column": 9,
      "reason": "one write of a stack-local word, which the kernel copies before the call returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 5862,
      "column": 9,
      "reason": "each followed link ascends by less than 16 MiB or crosses a stack segment's bridge frame, so frame stays on the thread's chain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 5976,
      "column": 9,
      "reason": "only the observation writer opens a window, under the flight recorder's lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 5987,
      "column": 5,
      "reason": "only the observation writer opens a window, under the flight recorder's lock; a handler reads the descriptor once, and the buffering only while it holds its buffer (perf_samples_flush)"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 5998,
      "column": 5,
      "reason": "only the observation writer opens a window, under the flight recorder's lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 6030,
      "column": 5,
      "reason": "only the observation writer and the exit close a window, under the flight recorder's lock; a handler reads the descriptor once"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 6310,
      "column": 5,
      "reason": "socketpair writes its two descriptors into the fresh state's receive buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 6323,
      "column": 5,
      "reason": "the observation start runs before the writer and the stop that read it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 6345,
      "column": 9,
      "reason": "read fills the state's receive buffer with one datagram"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 7048,
      "column": 9,
      "reason": "only the observation writer opens a window, under the flight recorder's lock, and a record reads the state only once it is stored"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 7392,
      "column": 5,
      "reason": "openat reads the image copy's NUL-terminated path in the state's text page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 7740,
      "column": 5,
      "reason": "the dump's calls take words alone or borrow the state's scratch or the process's own mappings for their duration"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 7844,
      "column": 5,
      "reason": "the vectors name the state's copy and a range of this process's own mappings; the kernel copies between them, faulting on neither"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 8344,
      "column": 9,
      "reason": "openat reads the dump's NUL-terminated path in the state's text page"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 8885,
      "column": 5,
      "reason": "getrusage writes the struct rusage in this thread's own entry"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/PerfSamples.jet",
      "line": 9083,
      "column": 5,
      "reason": "getcpu writes the CPU's 32-bit number into this thread's own entry"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 28,
      "column": 5,
      "reason": "the kernel enters this restorer only from a returning handler, on its signal frame"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 40,
      "column": 5,
      "reason": "rt_sigaction borrows the four-word action on the stack; handler is this runtime's one-word C-ABI export and the restorer its rt_sigreturn rail"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 63,
      "column": 9,
      "reason": "written under the scheduler root mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 224,
      "column": 5,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 284,
      "column": 9,
      "reason": "written under the scheduler root mutex, after every word the handler and dispatcher read"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 334,
      "column": 5,
      "reason": "rt_sigaction writes the signal's four-word action into the hub's preallocated table and sets none"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 338,
      "column": 5,
      "reason": "rt_sigaction borrows the four-word action on the stack; the handler is this runtime's one-word C-ABI export and the restorer its rt_sigreturn rail"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 347,
      "column": 5,
      "reason": "rt_sigaction reads the saved four-word action from the hub's table and writes no old action"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProcessSignal.jet",
      "line": 594,
      "column": 9,
      "reason": "rt_sigaction borrows the four-word SIG_IGN or SIG_DFL action on the stack and writes no old action"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProveJournal.jet",
      "line": 104,
      "column": 5,
      "reason": "the harness main arms the journal before any test, so before any other thread reads it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProveJournal.jet",
      "line": 171,
      "column": 5,
      "reason": "into lends `capacity` writable bytes for a raw Linux read of stdin"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProveJournal.jet",
      "line": 187,
      "column": 5,
      "reason": "cell lends 24 writable bytes to TIOCGPGRP and getpgrp takes none"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProveJournal.jet",
      "line": 197,
      "column": 9,
      "reason": "cell lends the new and old signal masks and the group id to rt_sigprocmask and TIOCSPGRP"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ProveJournal.jet",
      "line": 205,
      "column": 9,
      "reason": "cell lends the group id to TIOCSPGRP and the old mask to rt_sigprocmask"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 11,
      "column": 5,
      "reason": "address borrows an initialized aligned word in a live native layout the caller holds"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 17,
      "column": 5,
      "reason": "address is an aligned word of native storage the caller owns or holds the lock for"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 23,
      "column": 5,
      "reason": "address borrows an initialized byte inside the caller's live buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 29,
      "column": 5,
      "reason": "address is one exclusively owned writable byte and value is a byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 47,
      "column": 5,
      "reason": "native lowering owns both live ranges of count bytes; each leaf loads all its bytes before it stores any, and the steps run in the order the overlap requires"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 94,
      "column": 5,
      "reason": "native lowering owns total writable bytes at target and period readable bytes at source, outside the target bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 144,
      "column": 5,
      "reason": "native lowering owns count writable bytes at target; each leaf stores only inside its class's count"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 202,
      "column": 5,
      "reason": "the caller (C, or the List equality's lowering) holds count readable bytes at left and at right"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 226,
      "column": 5,
      "reason": "checked native value-returning callback; its raw argument stays valid for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 232,
      "column": 5,
      "reason": "checked native value-returning callback; both raw arguments stay valid for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 238,
      "column": 5,
      "reason": "checked unary closure adapter with borrowed environment, input and output slots"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 244,
      "column": 5,
      "reason": "checked binary closure adapter with live environment, inputs and output slots"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 250,
      "column": 5,
      "reason": "checked native Unit callback; its raw argument stays valid for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 256,
      "column": 5,
      "reason": "checked Unit clone callback; source remains live and target lends writable output storage"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 262,
      "column": 5,
      "reason": "checked native entry or runtime export whose System V signature takes five words, all live for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 268,
      "column": 5,
      "reason": "checked executable System V void(void) address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 272,
      "column": 5,
      "reason": "checked native Unit callback; its three raw arguments stay valid for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 280,
      "column": 5,
      "reason": "checked code(box) address; the box stays live for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RawWords.jet",
      "line": 286,
      "column": 5,
      "reason": "checked code(box, argument) address; the box and argument stay live for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Reactive.jet",
      "line": 41,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Reactive.jet",
      "line": 81,
      "column": 5,
      "reason": "the observer id is a label nothing reads back; a racing creator can only repeat a label"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/ReceiptSections.jet",
      "line": 245,
      "column": 5,
      "reason": "the terminated path and the 128-byte stat fact row are live for the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RecordReplay.jet",
      "line": 176,
      "column": 5,
      "reason": "the observation start runs on the main thread before any task"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RecordReplay.jet",
      "line": 211,
      "column": 5,
      "reason": "the observation start runs on the main thread before any task"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RecordReplay.jet",
      "line": 255,
      "column": 5,
      "reason": "the observation start runs on the main thread before any task"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAEncoding.jet",
      "line": 99,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAEncoding.jet",
      "line": 369,
      "column": 5,
      "reason": "the terminated raw path and the 128-byte stat fact row are live for the syscall"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAFiles.jet",
      "line": 36,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAFiles.jet",
      "line": 523,
      "column": 5,
      "reason": "the audited caller supplies the Linux syscall's valid word arguments and live buffers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAFiles.jet",
      "line": 529,
      "column": 5,
      "reason": "the audited caller supplies the Linux syscall's valid word arguments and live buffers"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteALog.jet",
      "line": 90,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteALog.jet",
      "line": 124,
      "column": 5,
      "reason": "the log start runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteALog.jet",
      "line": 131,
      "column": 5,
      "reason": "the entry function configures before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteAMath.jet",
      "line": 14,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteSys.jet",
      "line": 114,
      "column": 5,
      "reason": "close(2) releases a descriptor this route opened"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteSys.jet",
      "line": 124,
      "column": 9,
      "reason": "read(2) fills at most the caller's remaining owned bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteSys.jet",
      "line": 146,
      "column": 9,
      "reason": "socket(AF_UNIX, SOCK_STREAM|SOCK_CLOEXEC) retains no memory"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteSys.jet",
      "line": 162,
      "column": 9,
      "reason": "connect(2) reads the owned 23-byte address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteSys.jet",
      "line": 178,
      "column": 9,
      "reason": "sendto(2) with MSG_NOSIGNAL reads the owned request"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteSys.jet",
      "line": 620,
      "column": 5,
      "reason": "utimensat borrows the NUL-terminated path and the two owned timespecs for the synchronous call"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteTerm.jet",
      "line": 333,
      "column": 13,
      "reason": "written under the host lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteTerm.jet",
      "line": 342,
      "column": 5,
      "reason": "TCGETS writes the kernel termios into the caller's writable 64-byte struct"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteTerm.jet",
      "line": 352,
      "column": 5,
      "reason": "TCSETS reads the kernel termios from the caller's initialized struct"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteTesting.jet",
      "line": 131,
      "column": 13,
      "reason": "written under the host lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/RouteTesting.jet",
      "line": 140,
      "column": 9,
      "reason": "written under the host lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/SHA256.jet",
      "line": 255,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/SHA256.jet",
      "line": 283,
      "column": 9,
      "reason": "the state list holds eight U32 words, and the range check keeps each block inside the length borrowed bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 54,
      "column": 5,
      "reason": "idempotent flags: every writer stores the same 1, the held one first"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 63,
      "column": 5,
      "reason": "an idempotent flag: every writer stores the same 1"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 77,
      "column": 5,
      "reason": "the monitor is the only thread that clears the flag; an arm it races restores it"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 253,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 378,
      "column": 5,
      "reason": "the cold OS leaf borrows its initialized stack word; a successful nonzero base is this attached thread's live Jet TCB or the detached zero word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 454,
      "column": 5,
      "reason": "written under the scheduler root mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 463,
      "column": 9,
      "reason": "written under the scheduler root mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 477,
      "column": 9,
      "reason": "written under the scheduler root mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 891,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 1048,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same code address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 1365,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 1705,
      "column": 5,
      "reason": "clock_gettime writes two words of thread-owned TCB scratch; no pointer escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 1757,
      "column": 5,
      "reason": "openat, read and close the path and fill only the caller's own scratch; the descriptor is closed before return"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 2058,
      "column": 9,
      "reason": "x86 pause only hints that this thread is waiting in a spin loop"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 2417,
      "column": 9,
      "reason": "sched_yield retains no memory and yields this OS thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Scheduler.jet",
      "line": 2480,
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
      "line": 41,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 164,
      "column": 17,
      "reason": "secret is a registered live Secret header (Close unregisters before it clears); its buffer holds capacity bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 205,
      "column": 17,
      "reason": "secret is a registered live Secret header; its length and data words are read"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 274,
      "column": 17,
      "reason": "secret is a registered live Secret header; its capacity and data words are read"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 300,
      "column": 5,
      "reason": "start..start+length covers the Secret buffer's mapped pages; madvise and mlock change only their dump and residency attributes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 318,
      "column": 5,
      "reason": "bytes is the moved native [U8] header; secret is fresh aligned24B storage and takes over its buffer reference without copying secret material"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 342,
      "column": 5,
      "reason": "secret is fresh aligned24B storage this call owns exclusively"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Secret.jet",
      "line": 359,
      "column": 5,
      "reason": "secret is a live exclusively borrowed native24B Secret header; len/cap/data follow the native List layout and all byte owners remain inside its buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sentry.jet",
      "line": 18,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Shared.jet",
      "line": 69,
      "column": 5,
      "reason": "x86 pause only hints that this thread is waiting in a spin loop; no memory is touched"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Shared.jet",
      "line": 571,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/SharedTransactions.jet",
      "line": 20,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/SharedTransactions.jet",
      "line": 24,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/StackSegment.jet",
      "line": 115,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/StackSegment.jet",
      "line": 210,
      "column": 9,
      "reason": "target..frontier lies inside the segment, this thread's own mapping above its guard page; populating only backs its pages"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/StackSegment.jet",
      "line": 283,
      "column": 5,
      "reason": "a fresh private anonymous mapping, owned by this thread's segment chain; its first page becomes the guard"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/StackSegment.jet",
      "line": 301,
      "column": 5,
      "reason": "no frame runs on a returned segment and nothing else holds its address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/StackSegment.jet",
      "line": 322,
      "column": 1,
      "reason": "frame is a live [saved rbp][return address] pair of the calling thread's frame-pointer chain"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 12,
      "column": 5,
      "reason": "borrows an aligned live synchronization word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 16,
      "column": 5,
      "reason": "publishes an aligned live synchronization word with x86 release ordering"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 22,
      "column": 5,
      "reason": "atomic exchange on an aligned live synchronization word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 28,
      "column": 5,
      "reason": "atomic fetch-add on an aligned live synchronization word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 34,
      "column": 5,
      "reason": "atomic compare-exchange on an aligned live synchronization word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 65,
      "column": 5,
      "reason": "futex wake borrows a live aligned word and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 88,
      "column": 5,
      "reason": "futex borrows live state; timed wait uses this attached thread's reserved TCB scratch until the syscall returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 104,
      "column": 5,
      "reason": "futex borrows a live aligned word and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 207,
      "column": 5,
      "reason": "base points into the kernel's vDSO image, mapped readable for the process lifetime"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 231,
      "column": 9,
      "reason": "an idempotent install: every writer stores the same resolved address"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 237,
      "column": 9,
      "reason": "the vDSO's clock_gettime(clockid_t, struct timespec *) writes the caller's two timespec words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 245,
      "column": 9,
      "reason": "clock_gettime writes the caller's two timespec words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 253,
      "column": 5,
      "reason": "clock_gettime writes two words of thread-owned TCB scratch; no pointer escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 262,
      "column": 5,
      "reason": "clock_gettime writes two words of thread-owned TCB scratch; no pointer escapes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 306,
      "column": 9,
      "reason": "x86 pause only hints that this thread is waiting in a spin loop"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 350,
      "column": 5,
      "reason": "sync_sole_inflight is this sole thread's own count until sync_sole is cleared; sync_sole_end reads it only after a membarrier"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 367,
      "column": 5,
      "reason": "an elided section: only this thread has written the lock word and the count since sync_sole_enter"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Sync.jet",
      "line": 381,
      "column": 5,
      "reason": "membarrier and sched_yield retain no memory; sync_sole only ever goes from 1 to 0"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Task.jet",
      "line": 774,
      "column": 9,
      "reason": "written once, zeroed first, under the TLS registry mutex"
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
      "line": 26,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TerminalProcessRoutes.jet",
      "line": 58,
      "column": 9,
      "reason": "TIOCSWINSZ borrows one initialized stack winsize word; the Child104 lock prevents close/recycling until ioctl returns"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Test.jet",
      "line": 134,
      "column": 9,
      "reason": "the OS leaf borrows at most eight initialized bytes from its own stack word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestCapture.jet",
      "line": 34,
      "column": 9,
      "reason": "written under the TLS registry mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestContracts.jet",
      "line": 30,
      "column": 5,
      "reason": "the harness main opens contracts before any test, so before any other thread observes one"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 37,
      "column": 5,
      "reason": "startup is single-threaded and precedes user code"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 53,
      "column": 5,
      "reason": "the startup entry is a live NUL-terminated path for open"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 66,
      "column": 5,
      "reason": "an idempotent install: every racing first use opens the same log"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 111,
      "column": 13,
      "reason": "buffer lends 4096 writable bytes after the kind byte to getcwd"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 118,
      "column": 13,
      "reason": "link is a live NUL-terminated /proc path and buffer lends 4096 writable bytes after the kind byte"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestReads.jet",
      "line": 178,
      "column": 5,
      "reason": "data lends `length` initialized bytes to one write"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestSuite.jet",
      "line": 27,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/TestSuite.jet",
      "line": 84,
      "column": 5,
      "reason": "`test: fn` calls `t.run()` on the harness's main thread, before any test task exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 119,
      "column": 5,
      "reason": "every caller has attached before running Jet TLS code; an unattached thread's GS names the readable zero word, which the allocator alone treats as no TCB"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 131,
      "column": 5,
      "reason": "GS is reserved for Jet and base is a live self-addressed TCB or the detached zero word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 138,
      "column": 5,
      "reason": "gettid retains no memory; every caller runs after thread_set_base, so a counted call reads a live GS"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 192,
      "column": 5,
      "reason": "getpid retains no memory"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 260,
      "column": 9,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 356,
      "column": 9,
      "reason": "the registry owns this live closure header whose code accepts header and one I64 value"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 462,
      "column": 5,
      "reason": "rt_sigprocmask reads the spawner's mask from the live start record and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 484,
      "column": 5,
      "reason": "the caller transferred a System V I64->I64 code address and one argument to this thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 501,
      "column": 5,
      "reason": "getrlimit writes its two-word record into the owned 16-byte scratch and retains no pointer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 595,
      "column": 5,
      "reason": "a fresh private anonymous mapping, owned by the new thread's handle; its first page becomes the guard and madvise only changes its page size"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 650,
      "column": 5,
      "reason": "the child inherits a fully blocked mask and starts on its own guarded stack seeded with jet_rt_thread_entry and the start record; the handle's tid word lives until join or reap"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 690,
      "column": 5,
      "reason": "the startup hands these over once, before the init arrays and any other thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 700,
      "column": 5,
      "reason": "the link's thread functions take words: the handle's live thread id word, jet_rt_thread_entry, its start record and a stack size, or a thread id"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 733,
      "column": 5,
      "reason": "rt_sigprocmask reads and writes the owned masks record; create writes the thread id into the live handle and hands jet_rt_thread_entry the start record, which the thread frees"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 968,
      "column": 5,
      "reason": "a credential system call takes word arguments; setgroups' list is the synccall caller's, live until every thread has run"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 1005,
      "column": 9,
      "reason": "kill(getpid(), SIGKILL) retains no memory"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 1023,
      "column": 5,
      "reason": "getpid retains no memory"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 1048,
      "column": 13,
      "reason": "tgkill names a live registered thread of this process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 1086,
      "column": 9,
      "reason": "a credential system call takes word arguments and retains no memory"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Thread.jet",
      "line": 1098,
      "column": 13,
      "reason": "written under the registry mutex, before any handler can run"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 21,
      "column": 5,
      "reason": "audited runtime syscall: write borrows live text, exit_group ends the process; getcwd, mkdir, open and close name caller-owned scratch or a descriptor"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/Trap.jet",
      "line": 118,
      "column": 9,
      "reason": "the OS leaf borrows a stack word; generated calls have 1..8 valid bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UIBackends.jet",
      "line": 76,
      "column": 5,
      "reason": "a web page's module runs on one thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UIBackends.jet",
      "line": 86,
      "column": 5,
      "reason": "only an attached web build records, on its one thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UIBackends.jet",
      "line": 96,
      "column": 5,
      "reason": "a web page's module runs on one thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UIBackends.jet",
      "line": 145,
      "column": 9,
      "reason": "written under the host lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UIBackends.jet",
      "line": 158,
      "column": 9,
      "reason": "written under the host lock"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UIBackends.jet",
      "line": 194,
      "column": 5,
      "reason": "a web page's module runs on one thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/UIBackends.jet",
      "line": 207,
      "column": 5,
      "reason": "a web page's module runs on one thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/VaultTestHooks.jet",
      "line": 29,
      "column": 5,
      "reason": "written under the TLS registry mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/VaultTestHooks.jet",
      "line": 34,
      "column": 5,
      "reason": "written under the TLS registry mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/VaultTestHooks.jet",
      "line": 219,
      "column": 5,
      "reason": "written under the vault test backup mutex"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/World.jet",
      "line": 647,
      "column": 5,
      "reason": "the entry function sets the device store before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/World.jet",
      "line": 719,
      "column": 5,
      "reason": "device_mutex is held"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Source/WsRoutes.jet",
      "line": 613,
      "column": 5,
      "reason": "runtime-init runs before any other thread exists"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 17,
      "column": 5,
      "reason": "the witnesses run one at a time on the main thread"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 24,
      "column": 5,
      "reason": "a failed allocator witness writes three process-static texts to stderr with raw writes, which take no heap, and terminates this isolated test process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 45,
      "column": 5,
      "reason": "both allocations own count bytes; touching their endpoints must not alias each other or their metadata"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 105,
      "column": 5,
      "reason": "first's private header names its live slab, whose arena word is fixed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 123,
      "column": 5,
      "reason": "the flushed cache left the slab linked at its class head; its live count and the class cell are stable while this thread is the heap's only user"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 130,
      "column": 9,
      "reason": "pointer's private header identifies its still-live slab, which must be the same mapping"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 135,
      "column": 9,
      "reason": "no other thread touches the slab or its class list"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 148,
      "column": 5,
      "reason": "first's private header holds its mapping base and length"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 230,
      "column": 9,
      "reason": "the test owns the full payload and can read its allocator header; the slab's live count and this thread's cache row hold still while it is the heap's only user"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 251,
      "column": 9,
      "reason": "the chain word remains readable until its payload is freed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 258,
      "column": 5,
      "reason": "after all payloads are freed, the class keeps its emptied slabs linked up to ALLOC_RETAINED_EMPTY_SLABS and counts each; any further empty slab went onto the free-slab stack; a first slab the host's blocks share never empties, so it stays linked"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 289,
      "column": 9,
      "reason": "the test owns the full payload and keeps its chain word there"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 297,
      "column": 9,
      "reason": "the chain word remains readable until its payload is freed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 304,
      "column": 5,
      "reason": "this thread is the heap's only user; the stack word is read once and the stacked slab's link word stays mapped"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 312,
      "column": 5,
      "reason": "the retired slab is the stack's top again; its pages were never unmapped"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 339,
      "column": 5,
      "reason": "the test owns this fresh align-8 block and its count word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 368,
      "column": 5,
      "reason": "the test owns this fresh align-8 block of size bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 377,
      "column": 5,
      "reason": "the test holds both owners of this block and its count word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 383,
      "column": 5,
      "reason": "the test still owns this block"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 389,
      "column": 5,
      "reason": "a cached small block stays mapped; its payload bytes are not cache links"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 404,
      "column": 5,
      "reason": "the test owns these fresh align-8 blocks and their count words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 408,
      "column": 5,
      "reason": "the test owns these fresh align-8 blocks and their count words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 437,
      "column": 9,
      "reason": "the test owns the payload and keeps its chain word there"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 446,
      "column": 5,
      "reason": "row is this thread's own cache row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 450,
      "column": 9,
      "reason": "the chain word remains readable until its payload is freed"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 456,
      "column": 9,
      "reason": "row is this thread's own cache row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 463,
      "column": 5,
      "reason": "after the flush the row is empty"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 473,
      "column": 5,
      "reason": "row is this thread's own cache row"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 482,
      "column": 5,
      "reason": "the test owns this fresh align-8 block and its header words"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 545,
      "column": 9,
      "reason": "the region owns its 64 KiB until its release below"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 559,
      "column": 5,
      "reason": "the region owns its 64 KiB until its release below"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 566,
      "column": 5,
      "reason": "the region owns its two units until its release below"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 573,
      "column": 5,
      "reason": "the run's pages were dropped at its release, and it stays writable for this region"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 587,
      "column": 5,
      "reason": "the region owns its two units until its release below"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 622,
      "column": 5,
      "reason": "runs holds count words this test owns, and the window's link words stay mapped"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet",
      "line": 722,
      "column": 5,
      "reason": "the exact success line is two bounded stack words written to stdout"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/CollectionsStopTests.jet",
      "line": 22,
      "column": 5,
      "reason": "the single-threaded fixture host owns the snapshot from a case to its check"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/CollectionsStopTests.jet",
      "line": 37,
      "column": 5,
      "reason": "the single-threaded fixture host owns the snapshot from a case to its check"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/EnvOverlayKernel.jet",
      "line": 8,
      "column": 5,
      "reason": "failed witness terminates its isolated real-runtime process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/PerfRingTests.jet",
      "line": 108,
      "column": 5,
      "reason": "the test runs on one thread and arms no sampler, so no handler reads these"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/PerfRingTests.jet",
      "line": 197,
      "column": 5,
      "reason": "the test runs on one thread and arms no sampler, so no handler reads these"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet",
      "line": 7,
      "column": 5,
      "reason": "an isolated failed scheduler witness exits with no further runtime cleanup"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 11,
      "column": 5,
      "reason": "a failed isolated Secret witness terminates this test process"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 21,
      "column": 5,
      "reason": "header is fresh aligned24B storage owned by the test; data owns cap times the host U8 stride bytes or is zero for an empty List"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 33,
      "column": 5,
      "reason": "the pointer is a live small allocation; this isolated test is the sole allocator user and can inspect its slab's class metadata"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 49,
      "column": 5,
      "reason": "input and secret are distinct live headers; adoption leaves one live buffer owner and borrowed bytes alias only that parent's header"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 70,
      "column": 5,
      "reason": "the test owns data and checks the initialized U8 slots, including epoch padding, before any release"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 81,
      "column": 5,
      "reason": "secret still owns its live24B header after Close; the buffer was released, and no pointer into it is used"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 93,
      "column": 5,
      "reason": "each block is a fresh live allocation this witness owns exclusively and chains through its first word; reused is the newly allocated buffer, not a read of freed storage"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 142,
      "column": 5,
      "reason": "data is this test's live List allocation; every capacity byte is dirtied before the wipe"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 150,
      "column": 5,
      "reason": "the same live allocation remains exclusively owned while this test checks all wiped bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 170,
      "column": 5,
      "reason": "data is this test's fresh live allocation of bytes bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 182,
      "column": 5,
      "reason": "secret is a live Secret header over its own buffer; data stays live through the retained reference"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 196,
      "column": 5,
      "reason": "data is still live: the other holder's reference was never released"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 207,
      "column": 5,
      "reason": "frozen is a live Secret header over its own buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 256,
      "column": 5,
      "reason": "both buffers are this test's fresh live allocations of bytes bytes"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 273,
      "column": 5,
      "reason": "the single-threaded fixture host owns the kept Secret from the entry to its ended check"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 296,
      "column": 5,
      "reason": "kept is the live Secret header the entry registered and kept_data its live buffer; the process end only zeroed the buffer"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet",
      "line": 309,
      "column": 5,
      "reason": "this isolated native witness writes its17B success literal without introducing any text or collection runtime dependency"
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
      "line": 37,
      "column": 5,
      "reason": "gettid verifies identity without trusting the TCB under test"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
      "line": 47,
      "column": 5,
      "reason": "ARCH_GET_GS writes a caller-owned word; the inherited pointer is compared, never dereferenced"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
      "line": 120,
      "column": 5,
      "reason": "rt_sigprocmask writes this thread's mask to a caller-owned word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
      "line": 207,
      "column": 13,
      "reason": "sched_yield widens both sides of the check-to-wait race without timing assumptions"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
      "line": 245,
      "column": 5,
      "reason": "rt_sigprocmask writes this thread's mask to a caller-owned word"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
      "line": 379,
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
      "line": 33,
      "column": 5,
      "reason": "single-threaded test: plain module writes of each scalar type"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetSema/Tests/XMLCheckerRootsFixtures.jet",
      "line": 13,
      "column": 5,
      "reason": "no raw operations in this regression"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetSema/Tests/XMLCheckerRootsFixtures.jet",
      "line": 14,
      "column": 9,
      "reason": "nested tail retains its value grammar"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Assets/no_nix.jet",
      "line": 57,
      "column": 9,
      "reason": "the no-nix launcher passes its subject's status 101 through unchanged; process.exit would report it as 1 (S36)"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Source/Exec.jet",
      "line": 621,
      "column": 13,
      "reason": "the testkit signals the child of its own step child, as the row's `@signal_child` asks"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Source/Exec.jet",
      "line": 715,
      "column": 5,
      "reason": "the testkit stops its own step child at a resource guard"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Source/Exec.jet",
      "line": 975,
      "column": 9,
      "reason": "the testkit stops a process its own case left running in the case scratch"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Source/StepsControl.jet",
      "line": 193,
      "column": 5,
      "reason": "kill(pid, 0) observes the case-owned child without sending a signal"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Source/StepsSessions.jet",
      "line": 370,
      "column": 5,
      "reason": "the case explicitly authorizes signaling its own spawned child"
    },
    {
      "package": "Compiler",
      "file": "Compiler/JetTestKit/Tests/Fixtures/Golden/compiler.jet",
      "line": 32,
      "column": 9,
      "reason": "the CLI double stands in for jet, whose internal compiler error alone ends with status 101 (I2); process.exit would report it as 1 (S36)"
    },
    {
      "package": "Core",
      "file": "Core/Auth/Auth.jet",
      "line": 228,
      "column": 5,
      "reason": "auth PASETO v4.public verification uses the fixed Ed25519 primitive"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 273,
      "column": 5,
      "reason": "vault key import: validate a serialized wrapped carrier at the typed crypto boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 280,
      "column": 5,
      "reason": "vault key import: expose the authenticated wrapped carrier, not plaintext key material"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 286,
      "column": 5,
      "reason": "vault key import: construct the typed recipient unlock request"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 292,
      "column": 5,
      "reason": "vault key import: construct the typed passphrase unlock request"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 818,
      "column": 5,
      "reason": "crypto expert: Argon2id password hashing"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 845,
      "column": 5,
      "reason": "crypto expert: Argon2id password verification"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 1146,
      "column": 5,
      "reason": "crypto expert: the authenticated envelope owns the one-time Poly1305 key and MAC framing"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 1244,
      "column": 5,
      "reason": "crypto expert: typed key agreement validates key widths and rejects all-zero shared secrets"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 1253,
      "column": 5,
      "reason": "crypto expert: typed signing keys validate the Ed25519 seed width"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Crypto.jet",
      "line": 1259,
      "column": 5,
      "reason": "crypto expert: typed verification validates public key and signature widths before strict verification"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Expert.jet",
      "line": 289,
      "column": 1,
      "reason": "crypto expert: the caller vouches for the password it hands the hash; its bytes stay in this call"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Expert.jet",
      "line": 331,
      "column": 1,
      "reason": "crypto expert: the caller takes custody of the secret's raw bytes and wipes them"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Expert.jet",
      "line": 334,
      "column": 1,
      "reason": "crypto expert: the caller takes custody of the shared secret's raw bytes and wipes them"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Expert.jet",
      "line": 337,
      "column": 1,
      "reason": "crypto expert: the caller takes custody of the signing seed's raw bytes and wipes them"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Expert.jet",
      "line": 340,
      "column": 1,
      "reason": "crypto expert: the caller takes custody of the X25519 private key's raw bytes and wipes them"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 120,
      "column": 5,
      "reason": "Core JETC2 applies the fixed XChaCha20-Poly1305 suite and protocol-owned nonce/AAD domains"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 129,
      "column": 5,
      "reason": "Core JETC2 authenticates a bounded record before releasing its plaintext"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 137,
      "column": 5,
      "reason": "Core JETC2 borrows shared material through expert, wiping its copy before propagating HKDF success or failure"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 153,
      "column": 5,
      "reason": "Core JETC2 exposes only an operation-scoped key copy and wipes it before either seal outcome escapes"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 169,
      "column": 5,
      "reason": "Core JETC2 exposes only an operation-scoped key copy and wipes it before either authentication outcome escapes"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 185,
      "column": 5,
      "reason": "Core JETC2 copies its derived wrap nonce and file key only for sealing, wiping both copies on either outcome"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 204,
      "column": 5,
      "reason": "Core JETC2 copies its derived wrap nonce only for authentication, wiping the copy on either outcome"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 267,
      "column": 5,
      "reason": "Core JETC2 checks secret draw widths through the audited exposure and wipes the temporary copy"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Jetc.jet",
      "line": 312,
      "column": 5,
      "reason": "Core JETC2 copies a validated ephemeral Secret into a zeroizing X25519SecretKey; both owners retain their Close duty"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 233,
      "column": 1,
      "reason": "vault key import: the caller's unlock request borrows the private key's bytes for the unlock only"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 238,
      "column": 1,
      "reason": "vault key import: the caller's unlock request borrows the passphrase's bytes for the unlock only"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 253,
      "column": 5,
      "reason": "vault typed load provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 258,
      "column": 5,
      "reason": "vault typed current provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 262,
      "column": 5,
      "reason": "vault typed status provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 267,
      "column": 5,
      "reason": "vault typed versions provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 272,
      "column": 5,
      "reason": "vault typed generation provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 277,
      "column": 5,
      "reason": "vault typed store provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 283,
      "column": 5,
      "reason": "vault key import: signing key material"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 291,
      "column": 5,
      "reason": "vault key import: X25519 secret material"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 301,
      "column": 5,
      "reason": "vault wrapped import provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 308,
      "column": 5,
      "reason": "vault typed rotation provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 313,
      "column": 5,
      "reason": "vault typed retirement provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 320,
      "column": 5,
      "reason": "vault typed revocation provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 327,
      "column": 5,
      "reason": "vault write authorization provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 334,
      "column": 5,
      "reason": "vault wrapped authorization provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 340,
      "column": 5,
      "reason": "vault typed generation commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 346,
      "column": 5,
      "reason": "vault typed store commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 352,
      "column": 5,
      "reason": "vault key import commit: signing key material"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 358,
      "column": 5,
      "reason": "vault key import commit: X25519 secret material"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 364,
      "column": 5,
      "reason": "vault wrapped import commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 370,
      "column": 5,
      "reason": "vault typed rotation commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 376,
      "column": 5,
      "reason": "vault typed retirement commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 382,
      "column": 5,
      "reason": "vault typed revocation commit provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 388,
      "column": 5,
      "reason": "vault passphrase export provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/Crypto/Vault.jet",
      "line": 397,
      "column": 5,
      "reason": "vault recipient export provider boundary"
    },
    {
      "package": "Core",
      "file": "Core/DB/Query.jet",
      "line": 172,
      "column": 5,
      "reason": "the exact owned query bytes passed the shared strict UTF-8 validator"
    },
    {
      "package": "Core",
      "file": "Core/Data/Data.jet",
      "line": 3211,
      "column": 5,
      "reason": "inspect existing private Decode carriers only inside checked Core data source"
    },
    {
      "package": "Core",
      "file": "Core/Data/Data.jet",
      "line": 3595,
      "column": 5,
      "reason": "inspect existing private Decode carriers only inside checked Core data source"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/CBOR.jet",
      "line": 110,
      "column": 5,
      "reason": "inspect private typed carriers without changing public Dyn variants"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/CBOR.jet",
      "line": 178,
      "column": 5,
      "reason": "validated private carriers retain byte strings for typed Encode"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/CBORStream.jet",
      "line": 248,
      "column": 9,
      "reason": "the retained key copies exactly the already validated text bytes"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/CBORStream.jet",
      "line": 357,
      "column": 17,
      "reason": "the exact CBOR text allocation passed strict UTF-8 validation"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/CSV.jet",
      "line": 1099,
      "column": 9,
      "reason": "each completed UTF-8 scalar was validated before its original bytes entered this field"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/CSV.jet",
      "line": 1263,
      "column": 25,
      "reason": "every byte of the field is below 128, so it is whole UTF-8"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Eetf.jet",
      "line": 617,
      "column": 9,
      "reason": "inspect private typed carriers without changing public Dyn variants"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/JSON.jet",
      "line": 1323,
      "column": 13,
      "reason": "preserve the native compiler-private Dyn carriers in Core JSON event projection"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/JSON.jet",
      "line": 1328,
      "column": 13,
      "reason": "preserve the native compiler-private Dyn carriers in Core JSON event projection"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 24,
      "column": 5,
      "reason": "the shared parser validated this exact numeric lexeme"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 462,
      "column": 9,
      "reason": "the shared UTF-8 validator just accepted this owned byte allocation"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 617,
      "column": 9,
      "reason": "the shared UTF-8 validator just accepted this owned ASCII token"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 622,
      "column": 13,
      "reason": "retain a validated number lexeme in the private event carrier until typed decoding"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 854,
      "column": 9,
      "reason": "transfer already validated UTF-8 storage to inspect its actual owned capacity"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 864,
      "column": 9,
      "reason": "the private numeric event is projected only into the matching typed tree carrier"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 1899,
      "column": 5,
      "reason": "the shared UTF-8 validator accepted this exact owned formatter buffer"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 1914,
      "column": 13,
      "reason": "every byte before the closing quote is printable ASCII, so it is whole UTF-8"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 2078,
      "column": 13,
      "reason": "retain a validated number lexeme in the private event carrier"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 2083,
      "column": 13,
      "reason": "project the native compiler-private Bytes carrier into its event"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 2149,
      "column": 13,
      "reason": "classify the private Bytes carrier without borrowing or exposing its payload"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 2303,
      "column": 5,
      "reason": "owned validated UTF-8 allocation transfers in the encoding source seam"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/Stream.jet",
      "line": 2339,
      "column": 5,
      "reason": "exercise the typed Encode byte carrier at the JSON preflight boundary"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/XMLEngine.jet",
      "line": 279,
      "column": 5,
      "reason": "the caller selected a scalar-aligned window of validated UTF-8 bytes"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/XMLEngine.jet",
      "line": 3432,
      "column": 5,
      "reason": "attribute whitespace compaction preserves the already validated UTF-8 byte sequence"
    },
    {
      "package": "Core",
      "file": "Core/Encoding/YAML.jet",
      "line": 556,
      "column": 5,
      "reason": "inspect compiler-private Bytes and typed carriers; public Dyn variants are unchanged"
    },
    {
      "package": "Core",
      "file": "Core/Files/Data.jet",
      "line": 275,
      "column": 9,
      "reason": "native path bytes originate from the host environment and are split at ASCII boundaries"
    },
    {
      "package": "Core",
      "file": "Core/Files/Files.jet",
      "line": 647,
      "column": 1,
      "reason": "compiler store owns the explicit child descriptor slot and retains the held Directory until the process leaf duplicates its descriptor"
    },
    {
      "package": "Core",
      "file": "Core/Files/Files.jet",
      "line": 740,
      "column": 9,
      "reason": "this test owns both held directories and child descriptor slots 198 and 199 until the child runs"
    },
    {
      "package": "Core",
      "file": "Core/Files/Sealed.jet",
      "line": 161,
      "column": 5,
      "reason": "the expert API explicitly authorizes authenticated legacy-v1 migration"
    },
    {
      "package": "Core",
      "file": "Core/HTTP/Client.jet",
      "line": 704,
      "column": 5,
      "reason": "checked resolver octets and port transferred into the socket carrier"
    },
    {
      "package": "Core",
      "file": "Core/Jobs/Jobs.jet",
      "line": 136,
      "column": 5,
      "reason": "queue preflight reads final-component metadata without following a symlink; metadata failures are ignored exactly as the SQLite provider does"
    },
    {
      "package": "Core",
      "file": "Core/Jobs/Jobs.jet",
      "line": 143,
      "column": 5,
      "reason": "create the lexical parent with the existing raw recursive mkdir kernel before SQLite open"
    },
    {
      "package": "Core",
      "file": "Core/Jobs/Jobs.jet",
      "line": 283,
      "column": 13,
      "reason": "match the queue's existing raw wall-clock wait without advancing controlled scheduler time"
    },
    {
      "package": "Core",
      "file": "Core/Jobs/Jobs.jet",
      "line": 604,
      "column": 5,
      "reason": "publish already committed queue observations through the existing validated Devtools event/sink boundary"
    },
    {
      "package": "Core",
      "file": "Core/Jobs/Jobs.jet",
      "line": 622,
      "column": 5,
      "reason": "publish source-projected queue status through the existing validated Devtools event/sink boundary"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 41,
      "column": 1,
      "reason": "`addr` is the address of a live, aligned `T` that stays valid for every use of the returned pointer"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 44,
      "column": 1,
      "reason": "`pointer` names a live, aligned, readable `T` for the whole access"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 47,
      "column": 1,
      "reason": "`pointer` names a live, aligned, writable `T` for the whole access"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 53,
      "column": 1,
      "reason": "`to` and `from` each name `count` live, aligned `T`s, and a T that owns a resource has one owner after the copy"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 58,
      "column": 1,
      "reason": "`to` names `count` live, aligned, writable `T`s that hold no value to drop"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 67,
      "column": 1,
      "reason": "`from` names `count` live, aligned `T`s, and a T that owns a resource has one owner after the append"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 76,
      "column": 1,
      "reason": "`to` names `count` live, aligned, writable `T`s that hold no value to drop, and a T that owns a resource has one owner after the copy"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 85,
      "column": 1,
      "reason": "the storage is used only through pointers derived from the result, and `free_raw` releases it exactly once"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 90,
      "column": 1,
      "reason": "`pointer` came unchanged from `alloc_raw`, is not released yet, and no copy of it is used after"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 101,
      "column": 1,
      "reason": "`address` starts a mapping of `bytes` that the caller just made and alone owns"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 104,
      "column": 1,
      "reason": "`address` and `bytes` name pages of a mapping the caller owns, which it unmaps next, so nothing reads or writes them after"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 114,
      "column": 1,
      "reason": "`pointer` names `count` live, aligned Ts that `owner` keeps alive and unchanged by anything else while the View lives"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 122,
      "column": 1,
      "reason": "`pointer` names `count` live, aligned Ts that `owner` keeps alive, and `count` is not negative"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 157,
      "column": 1,
      "reason": "`pointer` and the T `count` elements past it lie in one live allocation"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 160,
      "column": 1,
      "reason": "`pointer` and the address `bytes` past it lie in one live allocation"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 170,
      "column": 1,
      "reason": "the pages are fresh and private; the caller unmaps them once, with `__region_release` at the length it reserved"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 173,
      "column": 1,
      "reason": "`address` and `bytes` are a mapping `__region_reserve` answered, and nothing reads or writes its pages again"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 192,
      "column": 9,
      "reason": "the runtime maps fresh private pages that no other owner can see; the region unmaps them once, at close"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 201,
      "column": 5,
      "reason": "`address` names `bytes` of live, writable memory that nothing else reads or writes while the region and its blocks live"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 235,
      "column": 13,
      "reason": "the region mapped these pages at reserve, and no block of it is used after close"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 273,
      "column": 9,
      "reason": "`next`'s bytes now belong to this block, so its release duty is this block's"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 302,
      "column": 5,
      "reason": "the allocator is a place the `#Context(allocator:)` block keeps in place until it ends"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 307,
      "column": 9,
      "reason": "the runtime calls this only inside the block, while the allocator at `address` is live, in place and borrowed by nothing else"
    },
    {
      "package": "Core",
      "file": "Core/Mem/Mem.jet",
      "line": 349,
      "column": 13,
      "reason": "the block's bytes went back to the allocator that served them"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 183,
      "column": 5,
      "reason": "checked IPv4/IPv6 octets transferred into the native address carrier"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 189,
      "column": 5,
      "reason": "read native address octets; no parsing or formatting policy in the carrier leaf"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 197,
      "column": 5,
      "reason": "read the native IP address discriminant"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 260,
      "column": 5,
      "reason": "validated IP octets, port and IPv6 numeric scope transferred into the socket carrier"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 267,
      "column": 5,
      "reason": "read socket IP representation; formatting stays in Jet"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 273,
      "column": 5,
      "reason": "read native socket port"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 287,
      "column": 5,
      "reason": "read native IPv6 socket scope identifier"
    },
    {
      "package": "Core",
      "file": "Core/Net/Net.jet",
      "line": 480,
      "column": 5,
      "reason": "read the native UDP packet's socket-address carrier"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3653,
      "column": 1,
      "reason": "`key` and `iv` are a vector record's, used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3656,
      "column": 5,
      "reason": "a vector record's key and nonce, used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3662,
      "column": 1,
      "reason": "`key` and `iv` are a vector record's, used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3665,
      "column": 5,
      "reason": "a vector record's key and nonce, used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3671,
      "column": 1,
      "reason": "`key` and `iv` are a vector record's, used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3674,
      "column": 5,
      "reason": "a vector record's key and nonce, used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3680,
      "column": 1,
      "reason": "`key` and `iv` are a vector record's, used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3683,
      "column": 5,
      "reason": "a vector record's key and nonce, used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3690,
      "column": 5,
      "reason": "each record's key and IV are used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3731,
      "column": 13,
      "reason": "the record's key and IV are used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3755,
      "column": 13,
      "reason": "the record's key and nonce are used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3767,
      "column": 1,
      "reason": "`private` is a vector record's key, used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3770,
      "column": 5,
      "reason": "a vector record's private key, used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3777,
      "column": 1,
      "reason": "the arguments are a vector record's public key, message and signature"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3780,
      "column": 5,
      "reason": "verification of a vector record's public signature"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3793,
      "column": 13,
      "reason": "the record's private key is used for that record only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 3809,
      "column": 13,
      "reason": "the record's public key, message and signature"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 4124,
      "column": 1,
      "reason": "`seed` is a timing run's test key, used for that run only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 4127,
      "column": 5,
      "reason": "a timing run's test key, used for that run only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 4143,
      "column": 5,
      "reason": "timing runs on test keys and nonces, each used for its run only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 4166,
      "column": 1,
      "reason": "a timing run's test key, used for that run only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 4174,
      "column": 1,
      "reason": "a timing run's test key, used for that run only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 4214,
      "column": 5,
      "reason": "timing runs on test keys, each used for its run only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 4625,
      "column": 5,
      "reason": "test keys and nonces, each used for this run only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 4628,
      "column": 1,
      "reason": "`r` draws test keys and nonces, each used for this run only"
    },
    {
      "package": "Core",
      "file": "Core/Net/SSL/TLS.jet",
      "line": 4658,
      "column": 5,
      "reason": "test keys and nonces, each used for this run only"
    },
    {
      "package": "Core",
      "file": "Core/Process/Process.jet",
      "line": 245,
      "column": 5,
      "reason": "transfer a fully initialized Jet process plan into the temporary native ProcessSpec carrier; paired environment columns have equal length"
    },
    {
      "package": "Core",
      "file": "Core/Process/Process.jet",
      "line": 351,
      "column": 1,
      "reason": "process exit_unmapped: the caller reports a Jet defect and owns the 101 defect status"
    },
    {
      "package": "Core",
      "file": "Core/Service/Service.jet",
      "line": 1684,
      "column": 5,
      "reason": "read the final path component's OS metadata kind without opening an identity handle"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1020,
      "column": 1,
      "reason": "posix umask: the caller owns the process file-creation mask and restores it when other code relies on it"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1223,
      "column": 1,
      "reason": "posix set_scheduling: the caller owns the CPU share of the process it changes"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1354,
      "column": 1,
      "reason": "posix set_wall_clock: the caller owns the machine's wall clock, which every process reads"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1455,
      "column": 5,
      "reason": "posix getpgid: inspect a process group"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1461,
      "column": 5,
      "reason": "posix getsid: inspect a process session"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1466,
      "column": 1,
      "reason": "posix getpriority: the caller vouches that `which` names this process or one it controls"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1474,
      "column": 1,
      "reason": "posix setpriority: the caller owns the scheduling priority of the process it changes"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1479,
      "column": 1,
      "reason": "posix utime: the caller owns the file whose timestamps it rewrites"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1489,
      "column": 1,
      "reason": "posix stop: the caller accepts that the process ends here after the shared cleanup boundary"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1499,
      "column": 1,
      "reason": "posix pipe: the caller owns both returned descriptors and closes each exactly once"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1506,
      "column": 1,
      "reason": "posix close_fd: the caller owns `fd` and nothing uses it after this call"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1777,
      "column": 1,
      "reason": "posix mkfifo: the caller owns `path` and removes the named pipe it creates"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1797,
      "column": 1,
      "reason": "posix mknod: the caller owns `path` and grants every process that may open it access to the device it names"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1818,
      "column": 1,
      "reason": "posix fork: the caller handles both the parent and the child return and reaps the child"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1821,
      "column": 1,
      "reason": "posix setsid: the caller owns the process session topology it changes"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1824,
      "column": 1,
      "reason": "posix wait: the caller owns the child processes it reaps"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1831,
      "column": 1,
      "reason": "posix waitpid: the caller owns the child process it reaps"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1838,
      "column": 1,
      "reason": "posix setuid: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1841,
      "column": 1,
      "reason": "posix setgid: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1865,
      "column": 1,
      "reason": "posix set_user_ids: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 1873,
      "column": 1,
      "reason": "posix set_group_ids: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 2068,
      "column": 1,
      "reason": "posix setpgrp: the caller owns the process group topology it changes"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 2071,
      "column": 1,
      "reason": "posix setpgid: the caller owns the process group topology it changes"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 2076,
      "column": 1,
      "reason": "posix kill: the caller owns or is entitled to signal the target process"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 2084,
      "column": 1,
      "reason": "posix signal reset: the caller owns the process signal disposition it resets"
    },
    {
      "package": "Core",
      "file": "Core/Sys/Sys.jet",
      "line": 2089,
      "column": 1,
      "reason": "posix initgroups: the caller changes supplementary groups deliberately"
    },
    {
      "package": "Core",
      "file": "Core/TUI/TUI.jet",
      "line": 108,
      "column": 5,
      "reason": "Core TUI reads owned encoded OS environment bytes from the current host; no UTF-8 assumption"
    },
    {
      "package": "Core",
      "file": "Core/Tasks/Tasks.jet",
      "line": 115,
      "column": 1,
      "reason": "`handle` is a task the runtime submitted that no carrier has taken, and it goes back to the runtime once"
    },
    {
      "package": "Core",
      "file": "Core/Testing/Testing.jet",
      "line": 1513,
      "column": 5,
      "reason": "all appended bytes came from complete UTF-8 strings or ASCII"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/G15202611/Probes/c4599_28_comptime_asm.jet",
      "line": 10,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/G15202611/Probes/c4599_28_comptime_asm.jet",
      "line": 25,
      "column": 5,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/G15202611/Probes/c4599_28_comptime_asm_reach.jet",
      "line": 10,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/G15202611/Probes/c4599_28_comptime_asm_reach.jet",
      "line": 22,
      "column": 9,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/G15202611/Probes/c4599_28_runtime_asm_reach.jet",
      "line": 10,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/G15202611/Probes/c4599_28_runtime_asm_reach.jet",
      "line": 22,
      "column": 9,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/PlaceDefault20261121/Probes/returns.jet",
      "line": 107,
      "column": 5,
      "reason": "a raw word stands in for the destination word of the task's context"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/PlaceDefault20261121/Probes/returns.jet",
      "line": 116,
      "column": 5,
      "reason": "a raw word stands in for the destination word of the task's context"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/PlaceDefault20261121/Probes/returns.jet",
      "line": 206,
      "column": 17,
      "reason": "a raw word stands in for the destination word of the task's context"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/PlaceDefault20261121/Probes/returns.jet",
      "line": 245,
      "column": 17,
      "reason": "a raw word stands in for the destination word of the task's context"
    },
    {
      "package": "Docs",
      "file": "Docs/Audits/PlaceDefault20261121/Probes/returns.jet",
      "line": 284,
      "column": 17,
      "reason": "a raw word stands in for the destination word of the task's context"
    },
    {
      "package": "Docs",
      "file": "Docs/Proposals/DebugEvalNative20261118/Probes/fx_line.jet",
      "line": 13,
      "column": 5,
      "reason": "probe: a raw write under the empty row"
    },
    {
      "package": "Docs",
      "file": "Docs/Proposals/DebugEvalNative20261118/Probes/row_compose.jet",
      "line": 21,
      "column": 9,
      "reason": "probe: a raw write under the empty row"
    },
    {
      "package": "Docs",
      "file": "Docs/Proposals/DebugEvalNative20261118/Probes/unsafe_callee.jet",
      "line": 7,
      "column": 5,
      "reason": "probe: a raw write in a callee"
    },
    {
      "package": "Docs",
      "file": "Docs/Proposals/JaiStyleSample/Data.jet",
      "line": 1274,
      "column": 9,
      "reason": "native path bytes originate from the host environment and are split at ASCII boundaries"
    },
    {
      "package": "Docs",
      "file": "Docs/Proposals/JaiStyleSample/Data.jet",
      "line": 1886,
      "column": 5,
      "reason": "inspect existing private Decode carriers only inside checked Core data source"
    },
    {
      "package": "Docs",
      "file": "Docs/Spec/Reference/syntax-surface.jet",
      "line": 567,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be live and valid"
    },
    {
      "package": "Docs",
      "file": "Docs/Spec/Reference/syntax-surface.jet",
      "line": 570,
      "column": 5,
      "reason": "addr is the address of a live Int on this frame"
    },
    {
      "package": "Examples",
      "file": "Examples/Crypto/CryptoMigration.jet",
      "line": 35,
      "column": 5,
      "reason": "AES-256-GCM protocol interoperability"
    },
    {
      "package": "Examples",
      "file": "Examples/Crypto/CryptoSuite.jet",
      "line": 40,
      "column": 5,
      "reason": "known-answer check reads the RFC 7748 shared secret"
    },
    {
      "package": "Examples",
      "file": "Examples/Crypto/ExpertProjection.jet",
      "line": 36,
      "column": 5,
      "reason": "expert cryptography is an explicit opt-in"
    },
    {
      "package": "Examples",
      "file": "Examples/Crypto/RandomAPISplit.jet",
      "line": 36,
      "column": 5,
      "reason": "compare the typed and raw HKDF rungs"
    },
    {
      "package": "Examples",
      "file": "Examples/Crypto/VaultKeys.jet",
      "line": 44,
      "column": 5,
      "reason": "restore audited raw signing-key material"
    },
    {
      "package": "Examples",
      "file": "Examples/ErrorsAndEffects/Effects/AuditedGateLadder.jet",
      "line": 6,
      "column": 5,
      "reason": "the audited block has no low-level operation"
    },
    {
      "package": "Examples",
      "file": "Examples/ErrorsAndEffects/Effects/SingleUse.jet",
      "line": 85,
      "column": 5,
      "reason": "event canceled; the ticket admits to nothing, so voiding it is correct"
    },
    {
      "package": "Examples",
      "file": "Examples/Interop/FFI/InlineC.jet",
      "line": 7,
      "column": 1,
      "reason": "the scalar ABI contract matches the C definition"
    },
    {
      "package": "Examples",
      "file": "Examples/Interop/FFI/InlineC.jet",
      "line": 14,
      "column": 1,
      "reason": "the scalar ABI contract deliberately returns a mismatched result"
    },
    {
      "package": "Examples",
      "file": "Examples/Interop/FFI/InlineC.jet",
      "line": 20,
      "column": 5,
      "reason": "call the audited inline C contract"
    },
    {
      "package": "Examples",
      "file": "Examples/Interop/FFI/InlineC.jet",
      "line": 25,
      "column": 9,
      "reason": "call the audited inline C contract with bounded Int values"
    },
    {
      "package": "Examples",
      "file": "Examples/Interop/FFI/InlineC.jet",
      "line": 31,
      "column": 9,
      "reason": "call the audited inline C contract with a deliberately wrong C result"
    },
    {
      "package": "Examples",
      "file": "Examples/Interop/FFI/OpaqueHandles/run.jet",
      "line": 36,
      "column": 5,
      "reason": "fixed headers define handle close ownership, exclusive buffers, and bounded output lengths"
    },
    {
      "package": "Examples",
      "file": "Examples/LowLevel/InlineAsm.jet",
      "line": 9,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Examples",
      "file": "Examples/LowLevel/InlineAsm.jet",
      "line": 20,
      "column": 5,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Examples",
      "file": "Examples/LowLevel/LowLevel.jet",
      "line": 8,
      "column": 5,
      "reason": "`cell` is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Examples",
      "file": "Examples/LowLevel/LowLevel.jet",
      "line": 22,
      "column": 5,
      "reason": "`cell` is a live Int on this stack frame; the pointer never escapes"
    },
    {
      "package": "Examples",
      "file": "Examples/LowLevel/MMIOBoardWrite.jet",
      "line": 9,
      "column": 5,
      "reason": "stand-in MMIO cell stays on this stack frame"
    },
    {
      "package": "Examples",
      "file": "Examples/LowLevel/PointerCastDeref.jet",
      "line": 11,
      "column": 5,
      "reason": "flag is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Examples",
      "file": "Examples/LowLevel/RawPointerTools.jet",
      "line": 7,
      "column": 5,
      "reason": "the block is used only through pointers derived from it, every index stays inside its four Ints, and it is released once"
    },
    {
      "package": "Examples",
      "file": "Examples/LowLevel/UnsafeObligations.jet",
      "line": 9,
      "column": 5,
      "reason": "cell stays live and the pointer remains local"
    },
    {
      "package": "Examples",
      "file": "Examples/Memory/Allocation/ContextAllocator.jet",
      "line": 49,
      "column": 5,
      "reason": "the counter's kept blocks were claimed from its region, which unmaps them all"
    },
    {
      "package": "Examples",
      "file": "Examples/Memory/Allocation/ContextAllocatorRead.jet",
      "line": 56,
      "column": 5,
      "reason": "the counter's kept blocks were claimed from its region, which unmaps them all"
    },
    {
      "package": "Examples",
      "file": "Examples/Memory/Allocation/Uninit.jet",
      "line": 12,
      "column": 5,
      "reason": "uninitialized plain-data storage is filled before every read"
    },
    {
      "package": "Examples",
      "file": "Examples/Memory/Allocation/UserAllocator.jet",
      "line": 72,
      "column": 5,
      "reason": "the pool's free blocks were claimed from its region, which unmaps them all"
    },
    {
      "package": "Examples",
      "file": "Examples/Memory/ExpiringSecret.jet",
      "line": 23,
      "column": 5,
      "reason": "vault key import: expiring secret lifetime"
    },
    {
      "package": "Examples",
      "file": "Examples/Memory/Pin.jet",
      "line": 42,
      "column": 5,
      "reason": "node storage is fixed for the returned pin; self_addr names this place"
    },
    {
      "package": "Examples",
      "file": "Examples/Memory/Unsafe/UnsafeSentries.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "Examples",
      "file": "Examples/Memory/Unsafe/UnsafeSentriesPackageOff/run.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this package-policy proof"
    },
    {
      "package": "Examples",
      "file": "Examples/Memory/Unsafe/UnsafeSentriesProvenance.jet",
      "line": 8,
      "column": 5,
      "reason": "external address must be a live allocation"
    },
    {
      "package": "Examples",
      "file": "Examples/Memory/Unsafe/UnsafeSentriesSourceOff.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/OSCPUTime.jet",
      "line": 40,
      "column": 5,
      "reason": "posix set_wall_clock: a time before 1970 is refused before it reaches the kernel"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/OSIDs.jet",
      "line": 17,
      "column": 5,
      "reason": "posix set_user_ids: the effective ID is set to the one it already is, and the out-of-range ID never reaches the kernel"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/OSMknod.jet",
      "line": 29,
      "column": 5,
      "reason": "make a device file in the temporary directory and remove it"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/OSProcessControl.jet",
      "line": 31,
      "column": 5,
      "reason": "POSIX process and pipe control for the core.sys surface"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/OSScheduling.jet",
      "line": 26,
      "column": 5,
      "reason": "posix set_scheduling: the out-of-range priority never reaches the kernel"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/OSScheduling.jet",
      "line": 38,
      "column": 5,
      "reason": "posix set_scheduling: this program moves itself to batch time sharing and back"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/ProcessExitCleanup.jet",
      "line": 23,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/ProcessExitUnmapped.jet",
      "line": 21,
      "column": 13,
      "reason": "this child reports a Jet defect status on purpose"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/SystemPrivilegedEntry.jet",
      "line": 10,
      "column": 5,
      "reason": "run only inside a disposable user namespace with a dedicated test identity"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/SystemProjection.jet",
      "line": 21,
      "column": 5,
      "reason": "the clock file was created above in this test-owned directory"
    },
    {
      "package": "Examples",
      "file": "Examples/Systems/Process/SystemProjection.jet",
      "line": 27,
      "column": 5,
      "reason": "fork and reap both children before returning"
    },
    {
      "package": "Examples",
      "file": "Examples/Text/CheckedText/CheckedTextRuntimeHole.jet",
      "line": 25,
      "column": 5,
      "reason": "checked text example output"
    },
    {
      "package": "Examples",
      "file": "Examples/Text/CheckedText/CheckedTextRuntimeHoleStop.jet",
      "line": 24,
      "column": 5,
      "reason": "checked text example output"
    },
    {
      "package": "Examples",
      "file": "Examples/Text/CheckedText/CheckedTextType.jet",
      "line": 20,
      "column": 5,
      "reason": "checked text example output"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Foundation/Source/TreeHash.jet",
      "line": 602,
      "column": 9,
      "reason": "the authority retains the held directory until the process provider duplicates its descriptor; the child descriptor slot is supplied by the compiler"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Provider/Batteries/embedded/embedded.jet",
      "line": 183,
      "column": 5,
      "reason": "host replay uses a stack cell for the CMSDK UART0 control register and never dereferences the target address"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 75,
      "column": 5,
      "reason": "export a fresh Core signing seed only to descriptor-relative owner-only key publication; the Core key owner is wiped on every outcome"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 139,
      "column": 5,
      "reason": "import the protected seed into Core signing without a second raw-secret copy; close the temporary Core Secret on success and failure"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 165,
      "column": 5,
      "reason": "package author signatures require Core's strict Ed25519 verification; no raw verifier or helper process"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Publication/Tests/Index.jet",
      "line": 346,
      "column": 5,
      "reason": "the test owns the planted lock file it ages"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Store/Tests/ClosureGraph.jet",
      "line": 65,
      "column": 13,
      "reason": "the test owns the journal file it just wrote"
    },
    {
      "package": "Jetpack",
      "file": "Jetpack/Store/Tests/Seal.jet",
      "line": 33,
      "column": 5,
      "reason": "the store test owns the fixture file whose mtime it pins"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BackendIce/Fixtures/AsmRail/rail.jet",
      "line": 10,
      "column": 1,
      "reason": "the program claims the runtime's stack switch"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BackendIce/Fixtures/AsmRail/rail.jet",
      "line": 36,
      "column": 1,
      "reason": "the program claims the runtime's TLS base"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BackendIce/Fixtures/AsmRail/rail.jet",
      "line": 44,
      "column": 5,
      "reason": "call the leaf the build refuses"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BackendIce/Fixtures/AsmRail/rail.jet",
      "line": 48,
      "column": 5,
      "reason": "call the leaf the build refuses"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BackendIce/Fixtures/AsmRdtsc/stamp/stamp.jet",
      "line": 9,
      "column": 1,
      "reason": "rdtsc writes only rax and rdx, both declared"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BackendIce/Fixtures/AsmRdtsc/stamp/stamp.jet",
      "line": 20,
      "column": 1,
      "reason": "lfence and rdtsc write only rax and rdx, and rdx is declared"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BackendIce/Fixtures/AsmRdtsc/stamp/stamp.jet",
      "line": 32,
      "column": 5,
      "reason": "call the audited rdtsc contracts"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BackendIce/Fixtures/AsmRdtsc/unencoded/unencoded.jet",
      "line": 12,
      "column": 1,
      "reason": "the body writes only its result register"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BackendIce/Fixtures/AsmRdtsc/unencoded/unencoded.jet",
      "line": 20,
      "column": 1,
      "reason": "the body writes only its result register"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BackendIce/Fixtures/AsmRdtsc/unencoded/unencoded.jet",
      "line": 28,
      "column": 5,
      "reason": "call the leaves sema refuses"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BuildEntry/MetaCodeNeverAuthorizesUnsafe.fixture/added/main.jet",
      "line": 8,
      "column": 17,
      "reason": "generated"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/BuildEntry/MetaCodeNeverAuthorizesUnsafe.fixture/rewritten/main.jet",
      "line": 20,
      "column": 25,
      "reason": "generated"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CLIPartsCore/Fixtures/GatePolicy/main.jet",
      "line": 2,
      "column": 5,
      "reason": "org refusal"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CLIPartsInspect/Fixtures/Foreign/app/main.jet",
      "line": 4,
      "column": 5,
      "reason": "calls the audited dependency"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CLIPartsInspect/Fixtures/Foreign/dep/dep.jet",
      "line": 3,
      "column": 1,
      "reason": "intentionally invalid address"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CLIPartsInspect/Fixtures/Release/hardened/main.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CLIPartsInspect/Fixtures/Release/release/main.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CLIPartsInspect/Fixtures/Team/unsafe/main.jet",
      "line": 2,
      "column": 5,
      "reason": "audit"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CLIPartsInspect/Fixtures/TestHardened/hardened/main.jet",
      "line": 5,
      "column": 5,
      "reason": "test deliberately reads after arena reset"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CLIPartsInspect/Fixtures/TestHardened/release/main.jet",
      "line": 5,
      "column": 5,
      "reason": "test deliberately reads after arena reset"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cffi/CffiBorrowedByteViewCrossesForTheCall.fixture/main.jet",
      "line": 13,
      "column": 5,
      "reason": "the block holds the four bytes for the whole call, C keeps no pointer to it, and it is released once"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cffi/CffiNullStringReturnReadsAsAbsent.fixture/main.jet",
      "line": 14,
      "column": 9,
      "reason": "a non-null result names a static NUL-terminated string C never frees"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cffi/CffiPartialReadReturnsItsStatusAndLength.fixture/main.jet",
      "line": 14,
      "column": 5,
      "reason": "both blocks are live for the call, C writes at most `output_cap` bytes and keeps no pointer, and each is released once"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cffi/CffiPointerAndCountCrossTogether.fixture/main.jet",
      "line": 13,
      "column": 5,
      "reason": "the block holds the three bytes for the whole call, C keeps no pointer to it, and it is released once"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cffi/CffiRawStatusOutPointerReadsOnlyOnSuccess.fixture/main.jet",
      "line": 23,
      "column": 5,
      "reason": "store_load receives a live non-null slot; bytes are read only after status zero"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cffi/CffiStatusAndWrittenCountBothReturn.fixture/main.jet",
      "line": 14,
      "column": 5,
      "reason": "both blocks are live for the calls, C writes at most `output_cap` bytes and keeps no pointer, and each is released once"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cffi/FFICapabilityBoundaryIsSafeOnlyInsideUnsafe/Input/o2/run.jet",
      "line": 8,
      "column": 5,
      "reason": "the foreign call may write through the borrowed value"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cffi/FFICapabilityBoundaryIsSafeOnlyInsideUnsafe/Input/o3/run.jet",
      "line": 8,
      "column": 5,
      "reason": "the foreign call may write through the borrowed value"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cffi/RuntimeExitImportExitsWithItsArgument.fixture/main.jet",
      "line": 1,
      "column": 1,
      "reason": "this program owns the exit status it passes"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CffiNativeMatrix/Fixtures/RequiredNativeCABIMatrix/default/matrix.jet",
      "line": 47,
      "column": 5,
      "reason": "live non-null out slot; read only after status zero"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CffiNativeMatrix/Fixtures/RequiredNativeCABIMatrix/sysv64/matrix.jet",
      "line": 47,
      "column": 5,
      "reason": "live non-null out slot; read only after status zero"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CollectionsAndMethods/SentryTypedIntStoreKeepsExactCarrier.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "the typed pointer refers to the live local cell for these stores"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Comptime/NativeO0SharedTasksSelect.fixture/ffi.jet",
      "line": 6,
      "column": 1,
      "reason": "the scalar ABI contract matches the C definition"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Comptime/NativeO0SharedTasksSelect.fixture/ffi.jet",
      "line": 12,
      "column": 5,
      "reason": "call the audited inline C contract"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ComptimeCLIParity/ComptimeChildIsReapedBeforeTheProgramRuns.fixture/run.jet",
      "line": 15,
      "column": 5,
      "reason": "fork and reap the child before returning"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ComptimeCLIParity/Fixtures/TierParityExamplesRunThroughAOTJITAndInterpreter/tooling_checked_text_type/checked_text_type.jet",
      "line": 20,
      "column": 5,
      "reason": "checked text example output"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ComptimeCorePureParity/CryptoExpertAeadSignArgonAOTStdoutMatchesKnownAnswers.fixture/main.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed RFC interop vectors"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ComptimeCorePureParity/CryptoValuesMatchAOTComptimeForcedInterpreterAndDefaultDev/Input/o1/run.jet",
      "line": 12,
      "column": 5,
      "reason": "fixed RFC interop vectors"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ComptimeCorePureParity/CryptoValuesMatchAOTComptimeForcedInterpreterAndDefaultDev/Input/o2/run.jet",
      "line": 12,
      "column": 5,
      "reason": "fixed RFC interop vectors"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ConcurrencyBoundaries/AGroupTaskReadsALentParameterInPlace.fixture/lend.jet",
      "line": 10,
      "column": 5,
      "reason": "reads the address of the graph's storage, not its bytes"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ConcurrencyBoundaries/AGroupTaskReadsALentParameterInPlace.fixture/lend.jet",
      "line": 29,
      "column": 5,
      "reason": "reads the address of the graph's storage, not its bytes"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Context/ContextAllocatorMemoEntryMovesToTheHeap.fixture/memo.jet",
      "line": 63,
      "column": 5,
      "reason": "the reuser's kept blocks were claimed from its region, which unmaps them all"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Context/ContextAllocatorMovesTextThatLeavesTheBlock.fixture/text.jet",
      "line": 104,
      "column": 5,
      "reason": "the reuser's kept blocks were claimed from its region, which unmaps them all"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Context/ContextAllocatorMovesWhatLeavesTheBlock.fixture/moves.jet",
      "line": 124,
      "column": 5,
      "reason": "the reuser's kept blocks were claimed from its region, which unmaps them all"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Context/ContextAllocatorPanicInAllocateStopsTheTask.fixture/refuse.jet",
      "line": 38,
      "column": 5,
      "reason": "the picky allocator's kept blocks were claimed from its region, which unmaps them all"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Context/ContextAllocatorReadsBackTheUserAllocator.fixture/main.jet",
      "line": 52,
      "column": 5,
      "reason": "the counter's kept blocks were claimed from its region, which unmaps them all"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Context/ContextAllocatorServesTheBlockAndItsLibraryCalls.fixture/served.jet",
      "line": 61,
      "column": 5,
      "reason": "the counter's kept blocks were claimed from its region, which unmaps them all"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Context/ContextScratchBlockRewindsAndMovesWhatLeaves.fixture/scratch.jet",
      "line": 18,
      "column": 5,
      "reason": "only reads the address of an item the caller keeps alive"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ControlAndData/ExplicitExitUnmappedKeeps101OnEveryTier.fixture/main.jet",
      "line": 5,
      "column": 5,
      "reason": "this case reports a Jet defect status on purpose"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ControlAndData/ExplicitProcessExitCleanupGoldenMatchesAllExecutionTiers.fixture/process_exit_cleanup.jet",
      "line": 22,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ControlAndData/ProcessExitLeavesHeapToKernelAfterUserCleanup.fixture/main.jet",
      "line": 35,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CoreCryptoArgon2ID/Argon2IDParallelLanesMatchRfcVectorAcrossTiers/Input/run.jet",
      "line": 8,
      "column": 5,
      "reason": "fixed Argon2id parallel-lane known-answer vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CoreLib/CoreMemRegionsHandOutBlocksAndAWindowChecksItsIndex.fixture/main.jet",
      "line": 16,
      "column": 5,
      "reason": "the caller's block holds the bytes and stays unused while the adopted region and its blocks live"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CoreLib/CoreMemRegionsHandOutBlocksAndAWindowChecksItsIndex.fixture/main.jet",
      "line": 43,
      "column": 5,
      "reason": "the shelf's blocks were claimed from regions that are closed, and nothing reads them again"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CoreLib/CoreMemRegionsHandOutBlocksAndAWindowChecksItsIndex.fixture/window.jet",
      "line": 6,
      "column": 5,
      "reason": "the four Ints are fresh heap storage, every access stays inside them, and they are released once"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/CoreLibNet/SharedMemoryWordsOrderAcrossProcesses.fixture/main.jet",
      "line": 25,
      "column": 5,
      "reason": "fork a peer that shares the mapping, and reap it"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cross/WebBuildRefusesInlineCBeforeCodegen.fixture/main.jet",
      "line": 1,
      "column": 1,
      "reason": "the scalar ABI contract matches the C definition"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Cross/WebBuildRefusesInlineCBeforeCodegen.fixture/main.jet",
      "line": 7,
      "column": 5,
      "reason": "call the audited inline C contract"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Custom/Fixtures/FromAddrSentryIsTierParity/fixture.jet",
      "line": 5,
      "column": 5,
      "reason": "value is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_alignment/unsafe_sentries_alignment.jet",
      "line": 5,
      "column": 5,
      "reason": "the tuple storage is live but this raw Int is misaligned"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_expired_recursive_stack/unsafe_sentries_expired_recursive_stack.jet",
      "line": 11,
      "column": 5,
      "reason": "the recursive address outlives its owning Jet frame"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_expired_stack/unsafe_sentries_expired_stack.jet",
      "line": 10,
      "column": 5,
      "reason": "the returned address outlives its owning Jet frame"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_misaligned_list/unsafe_sentries_misaligned_list.jet",
      "line": 5,
      "column": 5,
      "reason": "the list storage is live but this raw Int is misaligned"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_provenance/unsafe_sentries_provenance.jet",
      "line": 6,
      "column": 5,
      "reason": "external address must be a live allocation"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_quarantine/unsafe_sentries_quarantine.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Custom/Fixtures/SentrySourcePolicyOffIsTierParity/fixture.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Debug/AFaultNamesItsRoutineAndTask.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "the program copies from an address it never mapped, to fault"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/DevPartsRoutine/JIT1216AdversarialRegressions.fixture/raw_alias/run.jet",
      "line": 5,
      "column": 5,
      "reason": "the pointer stays inside this stack frame"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/DevPartsRoutine/UnsafeBlocksAreEvaluatedWithLiveSentries.fixture/fault.jet",
      "line": 4,
      "column": 5,
      "reason": "mapped address is valid and aligned"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/DiagnosticsCoverage/SentryMetadataUsesTheReturnedFaultExitCode.fixture/app/main.jet",
      "line": 4,
      "column": 5,
      "reason": "calls the audited dependency"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/DiagnosticsCoverage/SentryMetadataUsesTheReturnedFaultExitCode.fixture/dep/dep.jet",
      "line": 3,
      "column": 1,
      "reason": "intentionally invalid address"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Effects/ProcessExitNeedsOnlyTheExitLeaf.fixture/exit.jet",
      "line": 7,
      "column": 1,
      "reason": "the case checks the leaf of the unmapped exit"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Effects/ProcessExitNeedsOnlyTheExitLeaf.fixture/exit.jet",
      "line": 13,
      "column": 9,
      "reason": "the case checks the leaf of the unmapped exit"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FixturesCryptoBridgeEntropy/Argon2IDRfcAndExpertKnownAnswersKillArgon2IMutation.fixture/run.jet",
      "line": 44,
      "column": 5,
      "reason": "original public expert Argon2id KAT"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FixturesCryptoBridgeEntropy/ExpertAES256GcmMatchesNistCavsVector.fixture/run.jet",
      "line": 41,
      "column": 5,
      "reason": "original published known-answer vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FixturesCryptoBridgeEntropy/ExpertSurfaceMatchesReferenceVectorsAndBounds.fixture/run.jet",
      "line": 48,
      "column": 5,
      "reason": "original expert interoperability vectors"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FixturesCryptoBridgeEntropy/ExpertXchacha20Poly1305MatchesCfrgVector.fixture/run.jet",
      "line": 41,
      "column": 5,
      "reason": "original published known-answer vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FixturesCryptoBridgeEntropy/JetcV1ExpertOpenAcceptsOnlyThePinnedGrammar.fixture/run.jet",
      "line": 47,
      "column": 5,
      "reason": "original pinned JETC v1 migration grammar"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FixturesCryptoBridgeEntropy/JetcV1MigrationResealsToVerifiedV2WithoutTouchingSource.fixture/run.jet",
      "line": 24,
      "column": 5,
      "reason": "crypto expert: the fixture seals a pinned JETC v1 envelope and migrates it"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FixturesCryptoBridgeEntropy/KeygenZeroizesProviderCopyAndStackSeed.fixture/run.jet",
      "line": 46,
      "column": 5,
      "reason": "crypto expert: the test reads the generated seed to prove it is live and 32 bytes"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FixturesCryptoBridgeEntropy/PriorCryptoSuiteStillUsesLiveEntropy.fixture/run.jet",
      "line": 49,
      "column": 5,
      "reason": "crypto expert: the test reads the generated seed length"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FixturesCryptoBridgeEntropy/TypedSecretWrapSignKDFAndPasswordPathsPreserveRoles.fixture/run.jet",
      "line": 55,
      "column": 5,
      "reason": "crypto expert: the test derives from the secret's raw bytes"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Fmt/FmtPreservesCheckedTextType/Input/run.jet",
      "line": 19,
      "column": 5,
      "reason": "formatter fixture"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Fmt/FmtPreservesUnsafeSiteModesAndPostfixObligations/Input/run.jet",
      "line": 3,
      "column": 1,
      "reason": "caller keeps address live"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Fmt/FmtPreservesUnsafeSiteModesAndPostfixObligations/Input/run.jet",
      "line": 13,
      "column": 5,
      "reason": "calling audited reader"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Fmt/FmtUnsafeReasonsEscapeStrings/Input/run.jet",
      "line": 3,
      "column": 1,
      "reason": "caller says \"ok\""
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Fmt/FmtUnsafeReasonsEscapeStrings/Input/run.jet",
      "line": 8,
      "column": 5,
      "reason": "path C:\\tmp"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FmtProject/RawCImportsAndModuleBindingsRemainCleanThroughCLI.fixture/runtime.jet",
      "line": 7,
      "column": 1,
      "reason": "audited C boundary"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FmtProject/RawCImportsAndModuleBindingsRemainCleanThroughCLI.fixture/runtime.jet",
      "line": 10,
      "column": 1,
      "reason": "audited C boundary"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FmtRoundTripRegressions/RawCImportsAndModuleBindingMutabilityAreByteExactAndIdempotent/Input/run.jet",
      "line": 7,
      "column": 1,
      "reason": "audited C boundary"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/FmtRoundTripRegressions/RawCImportsAndModuleBindingMutabilityAreByteExactAndIdempotent/Input/run.jet",
      "line": 10,
      "column": 1,
      "reason": "audited C boundary"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/GateLedger/Fixtures/Gates/gates.jet",
      "line": 2,
      "column": 5,
      "reason": "test unsafe reason"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/JetTest/JetTestReportsEachFailingTestAndRunsTheRest.fixture/parked.jet",
      "line": 35,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/LanguageFeatures/CoreGroupImportNamesModuleTraitsAndTypes.fixture/main.jet",
      "line": 46,
      "column": 5,
      "reason": "the stack's kept block was claimed from its region, which unmaps it"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/LanguageFeatures/ImplMethodInheritsTraitEffectRowOnEveryTier.fixture/main.jet",
      "line": 38,
      "column": 5,
      "reason": "the case prints the checked text"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/LivenessLints/FallbackErrIsNeverAnUnusedLocal.fixture/main.jet",
      "line": 3,
      "column": 1,
      "reason": "probe"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/LivenessLints/FallbackErrIsNeverAnUnusedLocal.fixture/main.jet",
      "line": 9,
      "column": 5,
      "reason": "probe"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Memory/SentryPolicyOffSkipsR0801.fixture/checked.jet",
      "line": 11,
      "column": 5,
      "reason": "target's buffer holds eight Ints: its three items and spare capacity past them"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Memory/SentryPolicyOffSkipsR0801.fixture/off.jet",
      "line": 11,
      "column": 5,
      "reason": "target's buffer holds eight Ints: its three items and spare capacity past them"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/AMappingIsLiveStorageUntilItIsUnmapped.fixture/after_close.jet",
      "line": 19,
      "column": 5,
      "reason": "the copy writes into pages the region unmapped, so the sentry stops it before any byte moves"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/AMappingIsLiveStorageUntilItIsUnmapped.fixture/after_close.jet",
      "line": 25,
      "column": 5,
      "reason": "the shelf's block was claimed from the closed region, and nothing reads it again"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/AMappingIsLiveStorageUntilItIsUnmapped.fixture/keeper.jet",
      "line": 6,
      "column": 5,
      "reason": "the program claims pages it never mapped"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/AMappingIsLiveStorageUntilItIsUnmapped.fixture/main.jet",
      "line": 15,
      "column": 5,
      "reason": "the block holds three Ints of the open region's fresh pages"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/AMappingIsLiveStorageUntilItIsUnmapped.fixture/main.jet",
      "line": 23,
      "column": 5,
      "reason": "the shelf's block was claimed from the closed region, and nothing reads it again"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawBlockCopyAndFillMoveWholeRanges.fixture/copy_from_past.jet",
      "line": 4,
      "column": 5,
      "reason": "the copy reads one Int past the source block"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawBlockCopyAndFillMoveWholeRanges.fixture/copy_to_past.jet",
      "line": 4,
      "column": 5,
      "reason": "the copy writes one Int past the target block"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawBlockCopyAndFillMoveWholeRanges.fixture/main.jet",
      "line": 5,
      "column": 5,
      "reason": "every range stays inside the `values` tuple, which outlives each access"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawBlockCopyAndFillMoveWholeRanges.fixture/main.jet",
      "line": 16,
      "column": 5,
      "reason": "both Strings stay inside `names`; the static texts they replace need no drop"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawBlockCopyAndFillMoveWholeRanges.fixture/past.jet",
      "line": 5,
      "column": 5,
      "reason": "the fill runs one Int past the tuple, so the sentry stops it before any write"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawByteFillWritesExactlyItsRange.fixture/main.jet",
      "line": 16,
      "column": 5,
      "reason": "every fill and read stays inside the block's `room` bytes, and the block is freed once after them"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawByteMoveMatchesABufferedMoveAtEverySizeAndOverlap.fixture/main.jet",
      "line": 17,
      "column": 5,
      "reason": "every move and every byte written or read stays inside the block, which is freed once after the moves"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "every raw range stays inside the `values` tuple, which outlives each access"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/main.jet",
      "line": 17,
      "column": 5,
      "reason": "each append reads the Ints `many` holds, which move before their old buffer goes"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/main.jet",
      "line": 27,
      "column": 5,
      "reason": "both ranges stay inside `names`; every String is a static text, which needs no single owner"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/negative.jet",
      "line": 6,
      "column": 5,
      "reason": "the two Ints of `cell` take the copy"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/outside.jet",
      "line": 6,
      "column": 5,
      "reason": "the two Ints of `cell` take the copy"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/past_append.jet",
      "line": 6,
      "column": 5,
      "reason": "the append reads one Int past the tuple, so the sentry stops it before the List changes"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/past_copy.jet",
      "line": 6,
      "column": 5,
      "reason": "the copy writes one Int past the tuple, so the sentry stops it before any byte moves"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawPointerCastReadsTheSameAddress.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "each cast names the live local cell, at an alignment its type allows"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawPointerCastReadsTheSameAddress.fixture/misaligned.jet",
      "line": 6,
      "column": 5,
      "reason": "the cast Int starts one byte into the cell, so the sentry stops the read"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawPointerStepsByElementsOrBytes.fixture/main.jet",
      "line": 5,
      "column": 5,
      "reason": "every step stays inside the `values` tuple, which outlives each read"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawPointerStepsByElementsOrBytes.fixture/past.jet",
      "line": 6,
      "column": 5,
      "reason": "the step runs one Int past the local cell, so the sentry stops the read"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/fromaddrfreed.jet",
      "line": 4,
      "column": 5,
      "reason": "the address names an Int of a block already released"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "every address names a live, aligned value of the block, which outlives each access"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/misaligned.jet",
      "line": 4,
      "column": 5,
      "reason": "the read starts one byte into the block, so its Int is not aligned"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/readfreed.jet",
      "line": 4,
      "column": 5,
      "reason": "the read takes an Int of a block already released"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/readpast.jet",
      "line": 4,
      "column": 5,
      "reason": "the read takes the Int just past the block"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/writepast.jet",
      "line": 4,
      "column": 5,
      "reason": "the write stores the Int just past the block"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawWindowChecksEachIndex.fixture/main.jet",
      "line": 5,
      "column": 5,
      "reason": "the window covers exactly the tuple's four Ints, which outlive it"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawWindowChecksEachIndex.fixture/past.jet",
      "line": 5,
      "column": 5,
      "reason": "the window covers exactly the tuple's four Ints, which outlive it"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawWindowChecksEachIndex.fixture/released.jet",
      "line": 10,
      "column": 9,
      "reason": "`base` names `count` live I64s while the capability lives"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawWindowChecksEachIndex.fixture/released.jet",
      "line": 17,
      "column": 5,
      "reason": "the block is used only through this capability and released once by `release`"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/ARawWindowChecksEachIndex.fixture/released.jet",
      "line": 27,
      "column": 5,
      "reason": "`base` came from `alloc_raw` in `acquire`, and the capability is spent here, once"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/AllocResizeFreeFuzz.fixture/main.jet",
      "line": 26,
      "column": 5,
      "reason": "`address` starts a live block of `size` bytes"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/AllocResizeFreeFuzz.fixture/main.jet",
      "line": 38,
      "column": 5,
      "reason": "the block is written only inside its `size` bytes, and released once by `release`"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/AllocResizeFreeFuzz.fixture/main.jet",
      "line": 47,
      "column": 5,
      "reason": "`address` came unchanged from `alloc_raw` in `claim`, and its block is released here once"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/AllocResizeFreeFuzz.fixture/main.jet",
      "line": 91,
      "column": 13,
      "reason": "both blocks are live, `old` holds at least `shared` bytes and `fresh` holds `size`"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/RawHeapStorageIsTakenAndReleased.fixture/aliased.jet",
      "line": 4,
      "column": 5,
      "reason": "a second release through a copy of the pointer, which the ownership check cannot see, so the runtime stops it"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/RawHeapStorageIsTakenAndReleased.fixture/kept.jet",
      "line": 4,
      "column": 5,
      "reason": "a copy of the pointer is read after its block is released, so the sentry stops the read"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/RawHeapStorageIsTakenAndReleased.fixture/main.jet",
      "line": 4,
      "column": 5,
      "reason": "each block is used only through pointers derived from it, and released once"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemoryKernel/RawHeapStorageIsTakenAndReleased.fixture/twice.jet",
      "line": 4,
      "column": 5,
      "reason": "the second release is refused before the program runs"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemorySemantics/ClosureBoxesFreeAtTheirLastOwner.fixture/main.jet",
      "line": 313,
      "column": 5,
      "reason": "the exit callback only reads its captured List"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/MemorySemantics/UnsafeAddressKeepsStringAliveToBlockEnd.fixture/main.jet",
      "line": 9,
      "column": 5,
      "reason": "label is live on this frame until this block ends"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ModuleStorage/ModuleGlobalsStartLaidOutUnlessTheyHoldAHandle.fixture/main.jet",
      "line": 114,
      "column": 5,
      "reason": "one task writes the globals"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/OSNative/CredentialsChangeOnEveryThread.fixture/main.jet",
      "line": 78,
      "column": 5,
      "reason": "the process is root of a disposable user namespace; it drops to a mapped user"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/OSNative/ForkUnderLoadChildCompletes.fixture/main.jet",
      "line": 47,
      "column": 9,
      "reason": "the child only allocates, resolves and exits; the parent reaps it"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Observe/PanicContextUsesOnlyLexicallyLiveLocals.fixture/unsafe/panic_lexical_unsafe.jet",
      "line": 4,
      "column": 5,
      "reason": "the region only binds and prints a local; it reads and writes no raw memory"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/OutputCallable/RunnableContractsAndSelectionFailInSema.fixture/probe3/main.jet",
      "line": 3,
      "column": 1,
      "reason": "raw boundary"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/PerfRun/NoCounters.fixture/main.jet",
      "line": 13,
      "column": 1,
      "reason": "`program` is the address of a live struct sock_fprog whose filter block outlives the call; the kernel copies the filter before it returns"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/PerfRun/NoCounters.fixture/main.jet",
      "line": 90,
      "column": 5,
      "reason": "the block holds the filter while seccomp copies it into the kernel, and is released once, after the call"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/Pin/Fixtures/LibraryWiresSelfRefUnderUnsafeAndExposesSafePinAPI/fixture.jet",
      "line": 9,
      "column": 5,
      "reason": "node storage is fixed for the returned pin; self_addr names this place"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/PlaceWindowLifetimes/BorrowedWindowStaysCallerOwned.fixture/main.jet",
      "line": 5,
      "column": 5,
      "reason": "the borrowed value is live in the caller"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/PlaceWindowLifetimes/HeapElementWindowStaysAllocatorOwned.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "the list element remains heap-owned"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/PlaceWindowLifetimes/StackWindowUsesExpiringSentryAddress.fixture/main.jet",
      "line": 6,
      "column": 5,
      "reason": "cell is live on this stack frame"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/PolicyScope/UnsafeObligationsAreCheckedBeforeCodegen.fixture/run.jet",
      "line": 5,
      "column": 5,
      "reason": "local pointer"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ProcessSessions/Subjects/native/session_fixture.jet",
      "line": 54,
      "column": 5,
      "reason": "the fixture's descendant rejoins the process group of the tree that spawned it"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ProcessSessions/Subjects/native/session_fixture.jet",
      "line": 96,
      "column": 9,
      "reason": "kill -0 only asks whether the killed descendant still exists"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ProcessSignalDrain/AHandlerRunsWhenItsTaskIsAtItsGuard.fixture/main.jet",
      "line": 29,
      "column": 9,
      "reason": "the program signals itself: 0 only checks that it may, SIGINT is the subscribed signal"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/ResourceClose/ReplacedListElementMapValueAndModuleStorageCloseBeforeTheyDrop.fixture/main.jet",
      "line": 19,
      "column": 5,
      "reason": "the test runs one task"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/RunEntry/ModuleWriteFromFnIsE0111.fixture/ok.jet",
      "line": 11,
      "column": 5,
      "reason": "this program starts no task, so nothing else writes `raw`"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/SingleUse/DropErasesNoUnsafeInCodegen.fixture/source-1.jet",
      "line": 11,
      "column": 5,
      "reason": "gone"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/SingleUse/DropInsideUnsafeBlockSatisfiesSingleUse.fixture/source-1.jet",
      "line": 11,
      "column": 5,
      "reason": "the resource is already gone; nothing to release"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/SingleUse/DropInsideUnsafeFnSatisfiesSingleUse.fixture/source-1.jet",
      "line": 9,
      "column": 1,
      "reason": "voids a freshly-acquired lock whose resource is already gone"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/SingleUse/DropInsideUnsafeFnSatisfiesSingleUse.fixture/source-1.jet",
      "line": 16,
      "column": 5,
      "reason": "calling the audited voider"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/SingleUse/HolderConsumedCompiles.fixture/main.jet",
      "line": 28,
      "column": 5,
      "reason": "the lease's job is done by hand here"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/SingleUse/HolderConsumedCompiles.fixture/main.jet",
      "line": 35,
      "column": 5,
      "reason": "the slot's lease is done by hand here"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/SingleUse/HolderDroppedIsE0140.fixture/main.jet",
      "line": 33,
      "column": 5,
      "reason": "the lease's job is done by hand here"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/SingleUse/ReuseAfterDropIsE0121.fixture/source-1.jet",
      "line": 11,
      "column": 5,
      "reason": "done with it"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/TargetMachines/Fixtures/SelectedTargetMachineRejectsDirectMMIOOutsideRegion/main.jet",
      "line": 4,
      "column": 5,
      "reason": "this address is intentionally outside the board MMIO region"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/TargetMachines/Fixtures/SelectedTargetMachineValidatesDirectMMIOAccesses/main.jet",
      "line": 4,
      "column": 5,
      "reason": "timer register is mapped by board.sensor_v1"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/ASentryFaultNamesTheAccessAndTheRange.fixture/steps.jet",
      "line": 5,
      "column": 5,
      "reason": "the second read steps one Int past the tuple, so the sentry stops it there"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/AnOptionalRawPointerIsNoneWhenNull.fixture/main.jet",
      "line": 12,
      "column": 5,
      "reason": "each pointer names the live local cell, or is null and never read"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/DerefPlaceLendsThePointeeToAWriteParameter.fixture/main.jet",
      "line": 13,
      "column": 5,
      "reason": "c lives in place until `run` returns"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/DerefPlaceReceivesATraitSelfMethodThroughAPointer.fixture/main.jet",
      "line": 21,
      "column": 5,
      "reason": "address is the address of a live A that outlives this call"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/DerefPlaceReceivesATraitSelfMethodThroughAPointer.fixture/main.jet",
      "line": 30,
      "column": 5,
      "reason": "c lives in place until `run` returns"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/EveryUnsafeRegionKindEntersItsOwnSentryScope.fixture/regions.jet",
      "line": 1,
      "column": 1,
      "reason": "the cell is live on this frame"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/EveryUnsafeRegionKindEntersItsOwnSentryScope.fixture/regions.jet",
      "line": 9,
      "column": 5,
      "reason": "call the gated reader"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/EveryUnsafeRegionKindEntersItsOwnSentryScope.fixture/regions.jet",
      "line": 13,
      "column": 5,
      "reason": "test region calls the gated reader"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/FFICapabilityEdgeUsesSharedSentryPrelude/main.jet",
      "line": 8,
      "column": 5,
      "reason": "the foreign call may write through the borrowed value"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/MemAddressOfNeverFoldsPlainOrParenthesized/paren/main.jet",
      "line": 3,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be a live, valid Int"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/MemAddressOfNeverFoldsPlainOrParenthesized/paren/main.jet",
      "line": 12,
      "column": 5,
      "reason": "addr is the address of `cell`, a live Int on this stack frame"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/MemAddressOfNeverFoldsPlainOrParenthesized/plain/main.jet",
      "line": 3,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be a live, valid Int"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/MemAddressOfNeverFoldsPlainOrParenthesized/plain/main.jet",
      "line": 12,
      "column": 5,
      "reason": "addr is the address of `cell`, a live Int on this stack frame"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/SentryKernelIsSharedByAOTAndRuntimeAdapters/main.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/UnsafeFnBlockAndPtrOps/main.jet",
      "line": 3,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be a live, valid Int"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/UnsafeFnBlockAndPtrOps/main.jet",
      "line": 12,
      "column": 5,
      "reason": "addr is the address of `cell`, a live Int on this stack frame"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/UnsafeTierEmitIsByteExact/main.jet",
      "line": 3,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be valid"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/UnsafeTierEmitIsByteExact/main.jet",
      "line": 12,
      "column": 5,
      "reason": "safe: cell is live"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/Fixtures/VolatileWriteEmitIsByteExact/main.jet",
      "line": 3,
      "column": 1,
      "reason": "UART TX register is mapped by the target machine"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/HardenedProfileStopsAnOutOfBoundsRawAccessAtEveryLevel.fixture/read/read.jet",
      "line": 6,
      "column": 5,
      "reason": "reads the one Int the arena allocated"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/HardenedProfileStopsAnOutOfBoundsRawAccessAtEveryLevel.fixture/write/write.jet",
      "line": 6,
      "column": 5,
      "reason": "writes the one Int the arena allocated"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/RawPointerToolsKeepTheirChecksInHardenedBuilds.fixture/freed.jet",
      "line": 4,
      "column": 5,
      "reason": "no copy of the pointer is read after the block is released"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/RawPointerToolsKeepTheirChecksInHardenedBuilds.fixture/misaligned.jet",
      "line": 5,
      "column": 5,
      "reason": "every Int read starts on an Int boundary"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/UnsafeAndRuntime/RawPointerToolsKeepTheirChecksInHardenedBuilds.fixture/range.jet",
      "line": 5,
      "column": 5,
      "reason": "the steps stay inside the tuple's four Ints"
    },
    {
      "package": "Tests",
      "file": "Tests/Cases/WebBuild/ModuleLocalDevIsValidatedAsWebRuntime.fixture/main.jet",
      "line": 5,
      "column": 9,
      "reason": "exercise the web gate"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/AES/nist_gcm.jet",
      "line": 13,
      "column": 5,
      "reason": "published KAT or frozen seed AES-GCM fixture"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/AES/seed_differential.jet",
      "line": 13,
      "column": 5,
      "reason": "published KAT or frozen seed AES-GCM fixture"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/aes256gcm_open.jet",
      "line": 8,
      "column": 5,
      "reason": "fixed NIST AES-256-GCM vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/aes256gcm_seal.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed NIST AES-256-GCM vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/argon2id.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed Argon2id known-answer vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/ed25519_sign.jet",
      "line": 6,
      "column": 5,
      "reason": "fixed RFC 8032 Ed25519 vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/ed25519_verify_strict.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed RFC 8032 Ed25519 vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/hkdf_sha256_raw.jet",
      "line": 8,
      "column": 5,
      "reason": "fixed RFC 5869 HKDF vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/open_v1.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed JETC v1 AES vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/secret_bytes.jet",
      "line": 9,
      "column": 5,
      "reason": "inspect fixed secret bytes"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/shared_secret_bytes.jet",
      "line": 11,
      "column": 5,
      "reason": "inspect shared-secret length"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/signing_key_bytes.jet",
      "line": 9,
      "column": 5,
      "reason": "inspect generated signing-key bytes"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/x25519_raw.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed RFC 7748 X25519 vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/x25519_secret_bytes.jet",
      "line": 9,
      "column": 5,
      "reason": "inspect generated X25519 secret bytes"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/xchacha20poly1305_open.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed CFRG XChaCha20-Poly1305 vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Expert/xchacha20poly1305_seal.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed CFRG XChaCha20-Poly1305 vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Vault/commit_import_signing.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed signing key into the vault"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Vault/commit_import_x25519.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed X25519 secret into the vault"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Vault/guard_errors.jet",
      "line": 33,
      "column": 5,
      "reason": "vault key import: fixed guard vectors that never hold key material"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Vault/prepare_import_signing.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed signing-key vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/Vault/prepare_import_x25519.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed X25519 secret vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 15,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 19,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 32,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 36,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 49,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 53,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 66,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 70,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 83,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 87,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 98,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 109,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 120,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 131,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 142,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 153,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 164,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 175,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 186,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 197,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 208,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 219,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 230,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 241,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 252,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 263,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 274,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 285,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 296,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 307,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 323,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 334,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 350,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 361,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 377,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 388,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 404,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 415,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 431,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 442,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 458,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 469,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 485,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 496,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 512,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 523,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 539,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 550,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 566,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 577,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 593,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 604,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 620,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 631,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 647,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 658,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 674,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 685,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 701,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 712,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 728,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 739,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 755,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 766,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 782,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 793,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 809,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 820,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 836,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 847,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 863,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 874,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 890,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 901,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 917,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 928,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 944,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 955,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 971,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 982,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 998,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1009,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1025,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1036,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1052,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1063,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1079,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1090,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1106,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1117,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1133,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1144,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1160,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1171,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1187,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1198,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1214,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1225,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1241,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1252,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1268,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1279,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1295,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1306,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1322,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1333,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1344,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1355,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1366,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1377,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1388,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1399,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1410,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1426,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1442,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1458,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1476,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1480,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1493,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1497,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1510,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1514,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1527,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1531,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1544,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1548,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1561,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1565,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1578,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1582,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1595,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1599,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1612,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1616,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1629,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1633,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1646,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1650,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1663,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1667,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1680,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1684,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1697,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1701,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1714,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1718,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1731,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1735,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1748,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1752,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1765,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1769,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1782,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1786,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1799,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1803,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1816,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1820,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1833,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1837,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1850,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1854,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1867,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1871,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1884,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1888,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1901,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1905,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1918,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1922,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1935,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1939,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1952,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1956,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1969,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1973,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1986,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 1990,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2003,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2007,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2020,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2024,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2037,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2041,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2054,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2058,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2071,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2075,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2088,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2092,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2105,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2109,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2122,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2126,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2139,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2143,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2156,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2160,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2173,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2177,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2190,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2194,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2207,
      "column": 5,
      "reason": "frozen Ed25519 sign witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2211,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2222,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet",
      "line": 2238,
      "column": 5,
      "reason": "public Ed25519 acceptance witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/ct_x25519.jet",
      "line": 5,
      "column": 5,
      "reason": "published/frozen X25519 test inputs"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Crypto/x25519.jet",
      "line": 11,
      "column": 5,
      "reason": "inspect shared-secret length"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Files/Sealed/migrate_v1.jet",
      "line": 12,
      "column": 5,
      "reason": "fixed JETC v1 source failure"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Mem/volatile_read.jet",
      "line": 6,
      "column": 5,
      "reason": "value is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Mem/volatile_write.jet",
      "line": 6,
      "column": 5,
      "reason": "value is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/atexit.jet",
      "line": 5,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/close_fd.jet",
      "line": 7,
      "column": 13,
      "reason": "close an invalid descriptor without changing process state"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/fork.jet",
      "line": 5,
      "column": 5,
      "reason": "fork and let the child return immediately"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/getpriority.jet",
      "line": 5,
      "column": 5,
      "reason": "read the current process priority"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/initgroups.jet",
      "line": 5,
      "column": 5,
      "reason": "probe initgroups with a stable missing user"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/kill.jet",
      "line": 5,
      "column": 5,
      "reason": "check the process group without sending a signal"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/mkfifo.jet",
      "line": 5,
      "column": 5,
      "reason": "probe FIFO creation with an empty path"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/mknod.jet",
      "line": 5,
      "column": 5,
      "reason": "probe device-file creation with a major number out of range"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/pipe.jet",
      "line": 5,
      "column": 5,
      "reason": "open a pipe and observe its descriptor pair"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/reset_signal.jet",
      "line": 5,
      "column": 5,
      "reason": "restore the default disposition of SIGUSR1, which this program never handles, then refuse signal 0"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/set_group_ids.jet",
      "line": 5,
      "column": 5,
      "reason": "set the effective group ID to the one it already is, which needs no privilege, then refuse an ID out of range"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/set_scheduling.jet",
      "line": 5,
      "column": 5,
      "reason": "move this short-lived witness to batch time sharing and back; neither needs privilege"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/set_user_ids.jet",
      "line": 5,
      "column": 5,
      "reason": "set the effective user ID to the one it already is, which needs no privilege, then refuse an ID out of range"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/set_wall_clock.jet",
      "line": 8,
      "column": 5,
      "reason": "a time before 1970 is refused before it reaches the kernel, so the clock never changes"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/setgid.jet",
      "line": 5,
      "column": 5,
      "reason": "probe setgid with an invalid identifier"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/setpgid.jet",
      "line": 5,
      "column": 5,
      "reason": "set the current process into its existing process group"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/setpgrp.jet",
      "line": 5,
      "column": 5,
      "reason": "set the current process group"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/setpriority.jet",
      "line": 5,
      "column": 5,
      "reason": "lower the current process priority for this short-lived witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/setsid.jet",
      "line": 5,
      "column": 5,
      "reason": "create a session when the host permits it"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/setuid.jet",
      "line": 5,
      "column": 5,
      "reason": "probe setuid with an invalid identifier"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/stop.jet",
      "line": 7,
      "column": 13,
      "reason": "stop this dedicated witness successfully"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/umask.jet",
      "line": 5,
      "column": 5,
      "reason": "set a deterministic mask for this short-lived witness"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/utime.jet",
      "line": 5,
      "column": 5,
      "reason": "probe timestamp update with an empty path"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/wait.jet",
      "line": 5,
      "column": 5,
      "reason": "wait for a child that this witness does not have"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Sys/waitpid.jet",
      "line": 5,
      "column": 5,
      "reason": "poll for a child that this witness does not have"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Tasks/queued_handle.jet",
      "line": 17,
      "column": 5,
      "reason": "the handles stay in this program and never reach the runtime's queue"
    },
    {
      "package": "Tests",
      "file": "Tests/Conformance/Corpus/Core/Tasks/queued_task.jet",
      "line": 5,
      "column": 5,
      "reason": "the handle stays in this program and never reaches the runtime's queue"
    },
    {
      "package": "Tests",
      "file": "Tests/Fixtures/native_inline_c_shared_state.jet",
      "line": 4,
      "column": 1,
      "reason": "updates this program's private counter"
    },
    {
      "package": "Tests",
      "file": "Tests/Fixtures/native_inline_c_shared_state.jet",
      "line": 13,
      "column": 1,
      "reason": "reads the counter defined by fixture_bump"
    },
    {
      "package": "Tests",
      "file": "Tests/Fixtures/native_inline_c_shared_state.jet",
      "line": 25,
      "column": 5,
      "reason": "the counter is private to these two bodies"
    },
    {
      "package": "Tests",
      "file": "Tests/Fuzz/Sema/Differential/ex_crypto_crypto_migration.jet",
      "line": 29,
      "column": 5,
      "reason": "AES-256-GCM protocol interoperability"
    },
    {
      "package": "Tests",
      "file": "Tests/Fuzz/Sema/Valid/ex_crypto_crypto_suite.jet",
      "line": 40,
      "column": 5,
      "reason": "known-answer check reads the RFC 7748 shared secret"
    },
    {
      "package": "Tests",
      "file": "Tests/Fuzz/Sema/Valid/ex_lowlevel_lowlevel.jet",
      "line": 8,
      "column": 5,
      "reason": "`cell` is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Tests",
      "file": "Tests/Fuzz/Sema/Valid/ex_lowlevel_lowlevel.jet",
      "line": 22,
      "column": 5,
      "reason": "`cell` is a live Int on this stack frame; the pointer never escapes"
    },
    {
      "package": "Tests",
      "file": "Tests/Fuzz/Sema/Valid/ex_lowlevel_pointer_cast_deref.jet",
      "line": 11,
      "column": 5,
      "reason": "flag is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/Comptime/ForeignBranchNotTaken.jet",
      "line": 12,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/Comptime/ForeignBranchNotTaken.jet",
      "line": 24,
      "column": 9,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/Comptime/MutableGlobalStartsFromAnyStorableValue.jet",
      "line": 56,
      "column": 5,
      "reason": "one task writes the globals"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/Comptime/MutableGlobalStartsFromComputedValue.jet",
      "line": 32,
      "column": 5,
      "reason": "one task writes the frame"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/Comptime/MutableGlobalStartsFromComputedValue.jet",
      "line": 43,
      "column": 5,
      "reason": "one task writes the globals"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/CoreCryptoFoundations/CryptoAeadTamperingIsRejectedAcrossTiers.jet",
      "line": 64,
      "column": 5,
      "reason": "fixed AEAD tamper vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/CoreCryptoFoundations/PrimitiveMatrix.jet",
      "line": 101,
      "column": 5,
      "reason": "known-answer check reads the RFC 7748 shared secret"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/CoreCryptoFoundations/PrimitiveMatrix.jet",
      "line": 163,
      "column": 5,
      "reason": "expert known-answer vectors need raw key bytes"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/CoreIO/CleanupMatrix.jet",
      "line": 35,
      "column": 5,
      "reason": "the stop runs the shared cleanup boundary"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/CoreIO/CleanupMatrix.jet",
      "line": 83,
      "column": 5,
      "reason": "the exit callbacks have no captured state"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/Dyn/NumberInUnsafe.jet",
      "line": 7,
      "column": 5,
      "reason": "the literal below is a valid JSON number lexeme"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/FailureFoundation/TransactOSStop.jet",
      "line": 34,
      "column": 5,
      "reason": "the stop runs the shared cleanup boundary"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/PatternsAndFields/FixedSizeListWidensAtCoreCall.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed signature vector"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/UnsafeAndRuntime/AsmSevenInputsPushToTheStack.jet",
      "line": 7,
      "column": 1,
      "reason": "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/UnsafeAndRuntime/AsmSevenInputsPushToTheStack.jet",
      "line": 54,
      "column": 5,
      "reason": "Linux getpid syscall 39 ignores all argument registers and retains no stack pointer"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/UnsafeAndRuntime/MemFromAddrDirect.jet",
      "line": 11,
      "column": 5,
      "reason": "the borrowed value is live in the caller and the pointer never escapes"
    },
    {
      "package": "Tests",
      "file": "Tests/Run/UnsafeAndRuntime/NativeGlobalWord.jet",
      "line": 6,
      "column": 5,
      "reason": "run, the program's one thread, is the only writer"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/AuditedGateLadderForbidden/run.jet",
      "line": 3,
      "column": 5,
      "reason": "the organization policy refuses this audited escape"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/E3113PackageWithoutKernelFiles/run.jet",
      "line": 6,
      "column": 1,
      "reason": "the caller releases the storage with `free_raw`, exactly once"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/E3113RawMemoryOutsideKernel/kernel.jet",
      "line": 4,
      "column": 1,
      "reason": "the caller releases the storage with `free_raw`, exactly once"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/E3113RawMemoryOutsideKernel/run.jet",
      "line": 5,
      "column": 1,
      "reason": "the caller releases the storage with `free_raw`, exactly once"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/E3113RuntimeUnitSourceOutsideKernel/run.jet",
      "line": 6,
      "column": 1,
      "reason": "each caller vouches for the address"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/E3113RuntimeUnitSourceOutsideKernel/run.jet",
      "line": 10,
      "column": 5,
      "reason": "the kernel's own leaf reads a word its caller lends"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/E3113RuntimeUnitSourceOutsideKernel/run.jet",
      "line": 16,
      "column": 5,
      "reason": "the host holds the word at this address"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/E3114RawAddressIntoKernel/run.jet",
      "line": 8,
      "column": 5,
      "reason": "the caller hands the address of a live Int"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/E3114RawAddressIntoKernel/run.jet",
      "line": 18,
      "column": 5,
      "reason": "kernel code hands the address of a live Int"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/GatesLoadPackageCeiling/run.jet",
      "line": 2,
      "column": 5,
      "reason": "audited load-only gate"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/Ownership/raw_window_read_after_its_owner_is_released.jet",
      "line": 11,
      "column": 5,
      "reason": "the cells hold no resource, so spending them frees nothing"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/Ownership/raw_window_read_after_its_owner_is_released.jet",
      "line": 16,
      "column": 5,
      "reason": "the window covers exactly the two Int fields, which `cells` keeps"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/Ownership/single_use_holder_dropped_owes_consume.jet",
      "line": 33,
      "column": 5,
      "reason": "the lease's job is done by hand here"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/SecretsExpiry/expiring_secret_loan_rejects_move_variadic_and_drop_paths_3.jet",
      "line": 13,
      "column": 9,
      "reason": "attempt to discard a loan"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/UnsafeAndRuntime/VolatileWriteEmitIsByteExact/run.jet",
      "line": 3,
      "column": 1,
      "reason": "UART TX register is mapped by the target machine"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/UnsafeEffectBudget/unsafedep/unsafedep.jet",
      "line": 3,
      "column": 5,
      "reason": "audited peek"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/UnsafeForbidden/run.jet",
      "line": 2,
      "column": 5,
      "reason": "not allowed"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/UnsafeForbiddenByOrganizationPolicy/run.jet",
      "line": 5,
      "column": 5,
      "reason": "the organization policy forbids this escape"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/UnsafePerSite/run.jet",
      "line": 2,
      "column": 5,
      "reason": "selection required"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/cffi_out_pointer_requires_unsafe.jet",
      "line": 14,
      "column": 1,
      "reason": "the local slot remains live for the complete C call"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/changeset_a_failed_handler_publishes_none.jet",
      "line": 28,
      "column": 25,
      "reason": "generated"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/checked_text_raw_requires_unsafe.jet",
      "line": 22,
      "column": 5,
      "reason": "audited literal"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_argon2id_literal_policy.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed password-hash vectors"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_argon2id_literal_policy_type_precedence.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed password-hash vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_argon2id_literal_policy_valid.jet",
      "line": 9,
      "column": 5,
      "reason": "protocol-selected password policy"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_argon2id_literal_policy_valid.jet",
      "line": 17,
      "column": 5,
      "reason": "fixed password-hash vectors"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_material_length.jet",
      "line": 12,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_material_length_type_precedence.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_material_length_valid.jet",
      "line": 5,
      "column": 5,
      "reason": "protocol-selected interop material"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_material_length_valid.jet",
      "line": 17,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_nonce_callback_bound_precedence.jet",
      "line": 13,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_nonce_correct.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_nonce_effect_bound_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_nonce_effect_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_nonce_effect_prohibition_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_nonce_effect_unknown_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_nonce_length.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_nonce_region_authority_precedence.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_nonce_trait_dispatch_precedence.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_expert_nonce_type_precedence.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_hkdf_output_length.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_hkdf_output_type_precedence.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_hkdf_output_valid.jet",
      "line": 8,
      "column": 5,
      "reason": "protocol-selected output length"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/crypto_hkdf_output_valid.jet",
      "line": 16,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/discarded_pure_quiet.jet",
      "line": 47,
      "column": 5,
      "reason": "the test deliberately keeps this stderr write behind its audit boundary"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/e3106_unsafe_skip_without_per_site.jet",
      "line": 3,
      "column": 5,
      "reason": "reads a checked constant"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_asm_duplicate_clobber.jet",
      "line": 3,
      "column": 1,
      "reason": "each discarded register is named once"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_asm_float_signature.jet",
      "line": 3,
      "column": 1,
      "reason": "register-only increment"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_asm_missing_return_anchor.jet",
      "line": 3,
      "column": 1,
      "reason": "cycle counter"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_asm_no_mem.jet",
      "line": 1,
      "column": 1,
      "reason": "register-only increment"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_asm_return_clobber.jet",
      "line": 3,
      "column": 1,
      "reason": "the result register is overwritten by the body"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_asm_unencoded_mnemonic.jet",
      "line": 3,
      "column": 1,
      "reason": "the body writes only its result register"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_asm_unknown_clobber.jet",
      "line": 3,
      "column": 1,
      "reason": "the body declares only audited registers"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_body_not_string.jet",
      "line": 1,
      "column": 1,
      "reason": "demo"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_inline_asm_call_needs_unsafe_region.jet",
      "line": 3,
      "column": 1,
      "reason": "the operands are scalar registers and r10 is a declared clobber"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_inline_asm_call_needs_unsafe_region.jet",
      "line": 14,
      "column": 5,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_inline_c_call_needs_unsafe_region.jet",
      "line": 1,
      "column": 1,
      "reason": "the scalar ABI contract matches the C definition"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_inline_c_call_needs_unsafe_region.jet",
      "line": 7,
      "column": 5,
      "reason": "call the audited inline C contract"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_inline_c_mismatch.jet",
      "line": 1,
      "column": 1,
      "reason": "deliberate mismatch fixture"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_inline_cpp_call_needs_unsafe_region.jet",
      "line": 1,
      "column": 1,
      "reason": "the scalar ABI contract matches the C++ definition"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_inline_cpp_call_needs_unsafe_region.jet",
      "line": 7,
      "column": 5,
      "reason": "call the audited inline C++ contract"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_unknown_argument.jet",
      "line": 1,
      "column": 1,
      "reason": "demo"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/ffi_unknown_language.jet",
      "line": 1,
      "column": 1,
      "reason": "demo"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/inline_always_foreign.jet",
      "line": 5,
      "column": 1,
      "reason": "register-only increment"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/inline_always_foreign.jet",
      "line": 10,
      "column": 1,
      "reason": "the scalar ABI contract matches the C definition"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/inline_always_foreign.jet",
      "line": 15,
      "column": 1,
      "reason": "this program owns the exit status it passes"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/inline_always_foreign.jet",
      "line": 19,
      "column": 5,
      "reason": "call the audited foreign contracts"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/lowlevel_e3102.jet",
      "line": 2,
      "column": 5,
      "reason": "addr is valid"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/lowlevel_e3103.jet",
      "line": 3,
      "column": 1,
      "reason": "reads through caller-provided raw pointer"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/lowlevel_volatile_write_without_mem.jet",
      "line": 2,
      "column": 5,
      "reason": "addr is valid"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/lowlevel_volatile_write_wrong_ptr.jet",
      "line": 4,
      "column": 5,
      "reason": "plain ints are not raw pointers"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/lowlevel_volatile_write_wrong_value.jet",
      "line": 6,
      "column": 5,
      "reason": "slot is live and pointer never escapes"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/meta_ladder_add_never_authorizes_unsafe.jet",
      "line": 7,
      "column": 13,
      "reason": "generated"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/module_write_from_fn_is_e0111.jet",
      "line": 13,
      "column": 5,
      "reason": "this program starts no task, so nothing else writes `raw`"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/runtime_r0801.jet",
      "line": 8,
      "column": 5,
      "reason": "external address must be a live allocation"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/runtime_r0801_misaligned_without_storage.jet",
      "line": 7,
      "column": 5,
      "reason": "a fixed odd address stands for a pointer with no storage"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/runtime_r0802.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/runtime_r0803.jet",
      "line": 7,
      "column": 5,
      "reason": "the read starts one byte into a live Int"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/secret_read_requires_unsafe.jet",
      "line": 8,
      "column": 5,
      "reason": "the length leaves this function, never the key bytes"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/sink_needs_checked_text.jet",
      "line": 26,
      "column": 5,
      "reason": "audited: the text is a fixed literal"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/unsafe_block_after_statement_parse_error.jet",
      "line": 8,
      "column": 5,
      "reason": "target outlives the read in this frame"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/unsafe_effect_module_cap.jet",
      "line": 4,
      "column": 5,
      "reason": "audited peek"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/unsafe_effect_prohibition_transitive.jet",
      "line": 4,
      "column": 5,
      "reason": "audited peek"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/unsafe_obligation_missing.jet",
      "line": 5,
      "column": 5,
      "reason": "local pointer"
    },
    {
      "package": "Tests",
      "file": "Tests/UI/unsafe_obligation_no_bleed.jet",
      "line": 6,
      "column": 5,
      "reason": "two independent pointer operations"
    },
    {
      "package": "Tests",
      "file": "Tests/UILint/inline_asm_reads_its_core_mem_gate.jet",
      "line": 8,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Tests",
      "file": "Tests/UILint/inline_asm_reads_its_core_mem_gate.jet",
      "line": 19,
      "column": 5,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Tests",
      "file": "Tests/UILint/raw_pointer_reads_its_core_mem_gate.jet",
      "line": 10,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "Tools",
      "file": "Tools/Agent/Cache/Lanes.jet",
      "line": 325,
      "column": 21,
      "reason": "the guard signals only compiler, build and test jobs of this user, chosen by name from /proc"
    },
    {
      "package": "Tools",
      "file": "Tools/Agent/Orchestrator/LaneCommit.jet",
      "line": 224,
      "column": 5,
      "reason": "signal 0 delivers nothing: it only asks whether a queued job's caller still runs"
    },
    {
      "package": "Tools",
      "file": "Tools/CI/CompiledWorkloadPeerLauncher.jet",
      "line": 655,
      "column": 9,
      "reason": "posix kill: every process in the sandbox's private PID namespace except its init and this supervisor belongs to the supervised command"
    },
    {
      "package": "Tools",
      "file": "Tools/CI/CompiledWorkloadPeerLauncher.jet",
      "line": 669,
      "column": 5,
      "reason": "posix signal reset + kill: restore SIG_DFL for one signal number, then re-raise it on this process so the caller observes the supervised command's signal"
    },
    {
      "package": "Tools",
      "file": "Tools/CI/CompiledWorkloadPeerProbe.jet",
      "line": 78,
      "column": 5,
      "reason": "posix kill: the probe ends itself with SIGTERM so the check observes the launcher propagating a signal death"
    },
    {
      "package": "Tools",
      "file": "Tools/Packaging/NativeCCompiler.jet",
      "line": 77,
      "column": 9,
      "reason": "re-raise the compiler's terminating signal on this process, as the parent shell expects"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/AsmParity/Allocfree/run.jet",
      "line": 48,
      "column": 5,
      "reason": "each slot holds the address of one block alloc_raw gave and nothing else does; the block is released once, before its slot takes the next one or after the passes"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/AsmParity/Memcopy/run.jet",
      "line": 64,
      "column": 5,
      "reason": "the block holds the source, the target and the moved bytes, LARGEST bytes and their room each, every offset stays inside its room, and the block is freed once after the passes"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/AsmParity/Memfill/run.jet",
      "line": 52,
      "column": 5,
      "reason": "the block holds LARGEST bytes and their room, every fill stays inside it, and the block is freed once after the passes"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/AsmParity/Rawword/run.jet",
      "line": 44,
      "column": 5,
      "reason": "the block holds WORDS words, every index is below WORDS, and the block is freed once after the passes"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/AsmParity/Resize/run.jet",
      "line": 54,
      "column": 5,
      "reason": "every append reads the first `op.size` of the 64 bytes `source` holds, and `source` neither grows nor drops while the appends run"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Direct/main.jet",
      "line": 25,
      "column": 9,
      "reason": "each index is checked against the block's `count` words, and the block is released once, at the end of the turn"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Direct/main.jet",
      "line": 47,
      "column": 5,
      "reason": "each index is checked against the block's `count` words, and each block is released once"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Direct/main.jet",
      "line": 76,
      "column": 5,
      "reason": "both blocks hold WORDS words, each index is below WORDS, and each block is released once, at the end"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Direct/main.jet",
      "line": 133,
      "column": 9,
      "reason": "each index is checked against the mapping's `count` words, and the region stays mapped until its close"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Direct/main.jet",
      "line": 158,
      "column": 5,
      "reason": "the block holds `count` words, each index is below `count`, and the block is released once, at the end"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 14,
      "column": 9,
      "reason": "the block is used only through this owner, which releases it once, at close"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 24,
      "column": 9,
      "reason": "`at` is one of the owner's live words"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 29,
      "column": 9,
      "reason": "`at` is one of the owner's live words"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 36,
      "column": 9,
      "reason": "the owner keeps its `count` words alive while the View lives"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 43,
      "column": 9,
      "reason": "the owner's words are live I64s, which own nothing"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 51,
      "column": 9,
      "reason": "both owners hold at least `count` live I64s"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 59,
      "column": 9,
      "reason": "both owners hold `count` live I64s"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 72,
      "column": 9,
      "reason": "the fresh block takes the owner's words, and the old block is released once, here"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 86,
      "column": 13,
      "reason": "`base` came from `alloc_raw` at `claim` or `grow`, and the owner is spent here"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 122,
      "column": 9,
      "reason": "the region's pages stay mapped, private and writable until the owner closes it"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 132,
      "column": 9,
      "reason": "`at` is one of the mapping's words"
    },
    {
      "package": "Tools",
      "file": "Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet",
      "line": 139,
      "column": 9,
      "reason": "`at` is one of the mapping's words"
    }
  ]
}
-->

### Counts

| crate/package | regions |
| --- | ---: |
| Compiler | 825 |
| Core | 179 |
| Docs | 18 |
| Examples | 45 |
| Jetpack | 8 |
| Tests | 567 |
| Tools | 28 |
| **total** | **1670** |

### Regions

| crate/package | file | line | reason |
| --- | --- | ---: | --- |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 270:5 | "`address` is the fresh mapping just made, page-rounded past the NUL" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 284:5 | "memfd_create reads the NUL-terminated name at `name`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 298:5 | "the caller owns `fd` and nothing uses it any more" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 308:9 | "the caller mapped `count` writable bytes at `address`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 544:9 | "a private read-only view of the memfd aliases nothing writable" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 614:17 | "loader_map_file mapped this segment private and writable at its vaddr above" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 636:5 | "MAP_FIXED_NOREPLACE never replaces an existing mapping" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 679:5 | "MAP_FIXED replaces only the image's own reserved pages" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 684:9 | "the bytes lie in the segment's last file page, mapped private and writable above" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 694:1 | "the caller mapped [from, to) + delta private and writable" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 756:5 | "MAP_FIXED replaces only the view's own fresh pages" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 760:5 | "the bytes lie in the view's last image page, mapped private and writable above" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 763:5 | "the view's image pages are mapped private and writable above" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 767:9 | "MAP_FIXED replaces only the view's own fresh pages; the memfd is never written again" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 773:9 | "MAP_FIXED replaces only the view's own fresh pages past the program part; the table goes to more of them, the ELF header to the view's first page, mapped private and writable above" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1241:5 | "openat reads the NUL-terminated path at `name`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1269:5 | "the timespec lies in the fresh zeroed page; nanosleep only reads it" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1277:1 | "the caller owns the stack bytes below `at`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1287:1 | "the caller owns the stack bytes below `at`" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1410:1 | "the map's 104 bytes lie in the caller's data page, in its mm-map room; brk(0) only reports the current break" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1502:1 | "the caller's executor page is its own fresh read+write mapping of one page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1549:5 | "the data page is this plan's own fresh mapping" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1562:5 | "`executor` is the one-page read+write mapping just made for it" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1576:13 | "F_GETFD only reads the descriptor's flags" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1588:9 | "rt_sigaction with no new action writes the old one (32 bytes) to the scratch" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1606:5 | "a fresh anonymous reservation aliases nothing" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1628:5 | "getrandom fills the 16 stack bytes at `random`, and each string goes below them, in the fresh main stack this plan owns" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1692:5 | "the words lie in the stack mapping, above its guard page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1695:5 | "the data page's mm-map room is this plan's own" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1718:5 | "the block is this plan's own fresh mapping of plan_size bytes" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1740:5 | "the executor's records only reset process state; it then enters the loaded image on its own stack" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1820:5 | "`executor` is the one-page read+write mapping just made for it" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1836:5 | "the block is this plan's own fresh mapping of plan_size bytes" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1868:5 | "the record block holds its count, then that many records of eight words; madvise only marks whether a fork copies the mappings the child unmaps; the executor's first record forks, and the child runs only the executor and enters the image, whose pages and the plan's it keeps" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1907:5 | "landlock_create_ruleset reads the 8-byte handled-access word at the start of the fresh page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1917:9 | "`page` is this function's fresh scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1922:5 | "no_new_privs and landlock_restrict_self change only this thread's own rights; the ruleset fd is this function's" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1941:1 | "landlock_add_rule reads the packed 12-byte rule this writes at the start of the caller's scratch `page`; openat reads the NUL-terminated path at `name`, and the O_PATH fd is this function's" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1968:5 | "wait4 writes one int status to the fresh page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1978:5 | "SIGKILL to a child this process started and has not reaped" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 1998:5 | "a kernel sigaction with handler SIG_IGN on a fresh page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2014:9 | "a zeroed kernel sigaction (SIG_DFL) and an empty signal set on a fresh page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2035:5 | "execve reads the NUL-terminated strings and the two NULL-ended arrays in the block; the empty signal set at the block's start" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2075:5 | "the bytes below `at` and the two arrays lie in the fresh block mapped above, sized for every string and word" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2448:9 | "PTRACE_SETOPTIONS on the helper's own stopped tracee" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2471:5 | "PTRACE_SETOPTIONS on the helper's own stopped tracee" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2521:9 | "fork copies this thread alone; the helper and the program it starts make only system calls on words computed before the fork and on the shared scratch page, allocating nothing, so no lock another thread held at the fork can stop them; execve reads the argument block" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2577:9 | "the program stored its refused PTRACE_TRACEME's result in the shared scratch page before it exited" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2596:1 | "`call` is the call words of the trace's scratch page, which the helper shares with this process; the helper makes the system calls the words name" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2618:1 | "`call` is the call words of the trace's scratch page, which the helper shares with this process; wait4 and PTRACE_GETREGS write only the scratch page's status word, rusage and register block" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2643:5 | "the helper's step writes the status word, the rusage and the register block of the trace's scratch page, which are read once it answers" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2672:5 | "socketpair and pipe2 each write their two int descriptors to the trace's scratch word, which is read before the next call" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2709:1 | "fcntl, dup2, dup3, close_range, rt_sigprocmask and exit_group of the helper's own descriptors and itself; `call` is the call words of the trace's scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2758:9 | "fork copies this thread alone; the helper makes only system calls on words computed before the fork and on the shared scratch page, allocating nothing, so no lock another thread held at the fork can stop it" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2803:5 | "the helper's PTRACE_SEIZE and PTRACE_INTERRUPT of a thread of the program it is to trace" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2817:5 | "the helper's PTRACE_DETACH of its own stopped tracee" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2826:1 | "the helper makes the system call as given; an argument that points into memory points into the trace's scratch page, the only memory it shares with this process" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2831:5 | "the call's number and arguments fill the first seven words of the trace's call; sendto reads its signal byte" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2853:5 | "read writes one byte to the trace's call signal byte; the helper stored the result before it answered" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2869:5 | "the helper's wait4 writes one int status to the trace's scratch word" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2890:9 | "the helper's wait4 writes one int status to the trace's scratch word; poll reads and writes the one pollfd at LOADER_TRACE_POLL, in the scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2912:5 | "tgkill of one thread of our own tracee" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2926:5 | "PTRACE_SETOPTIONS on a stopped thread of the helper's own tracee" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2959:5 | "the helper's PTRACE_CONT, PTRACE_SINGLESTEP or PTRACE_SYSCALL on its own stopped tracee" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2968:5 | "the helper's PTRACE_PEEKDATA stores the tracee's word to the trace's scratch word" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2979:5 | "the helper's PTRACE_POKEDATA writes one word of its own stopped tracee" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 2995:9 | "process_vm_readv reads the two iovecs at LOADER_TRACE_IOV and writes at most `want` bytes from LOADER_TRACE_BYTES, inside the trace's scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3027:9 | "the `want` bytes are copied to LOADER_TRACE_BYTES of the trace's scratch page, inside it, and pwrite64 reads them there" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3056:5 | "the helper's PTRACE_GETREGS writes the register block at the start of the trace's scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3068:5 | "the helper's PTRACE_GETFPREGS writes the 512-byte block at LOADER_TRACE_BYTES of the trace's scratch page, inside it, and the block is copied from there" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3084:5 | "the helper's PTRACE_SETREGS reads the register block written at the start of the trace's scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3096:5 | "the helper's PTRACE_PEEKUSER stores one word of its own stopped tracee's user area to the trace's scratch word" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3109:5 | "the helper's PTRACE_POKEUSER writes one debug register of its own stopped tracee" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3127:5 | "the helper's PTRACE_GETEVENTMSG stores the message to the trace's scratch word" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3143:5 | "the helper's PTRACE_GETSIGINFO writes the 128-byte siginfo at the start of the trace's scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3160:5 | "the helper's wait4 writes one int status to the trace's scratch word" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3183:9 | "the helper's wait4 writes one int status to the trace's scratch word; nanosleep reads the timespec at LOADER_TRACE_SLEEP, in the scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3207:5 | "tgkill of one thread of the helper's own tracee" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3217:9 | "SIGKILL to the helper's own tracee" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3228:17 | "the helper's PTRACE_CONT of a stopped thread of its own tracee, which the kill ends" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3247:9 | "wait4 of our own child, the helper, keeping no status" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3393:5 | "`attr` is the fresh zeroed page just mapped: the attribute block at its start, the clock's two words at its middle" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3405:1 | "the caller's zeroed attribute block `attr`, LOADER_PERF_ATTR_SIZE bytes" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3420:1 | "`attr` is the caller's attribute block, which perf_event_open only reads; the ring is a fresh shared mapping of the event's own descriptor" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3446:1 | "clock_gettime writes two words at the caller's `scratch`, read before the next" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3460:9 | "the ring is this sampler's live shared mapping: its header page, then `size` data bytes the kernel wrote up to data_head, read before data_tail lets it write there again" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3580:5 | "the attribute block goes to the command's fresh page's attribute words" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3593:5 | "clock_gettime writes the command's page's read words" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3731:5 | "each pipe2 writes its two descriptors to the fresh page's status word, which is read before the next" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3747:9 | "fork copies this thread alone; the child makes only system calls on words computed before the fork and on its copy of the page, allocating nothing, so no lock another thread held at the fork can stop it; execve reads the argument block" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3783:5 | "the go byte of the command's page to the child's pipe" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3791:5 | "read fills the failed execve's word of the command's page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3810:5 | "wait4 writes the int status and the rusage to the command's page" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3821:5 | "the command's page holds the wait status and rusage its reap wrote" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3850:5 | "the attributes go to the command's fresh page's attribute words" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3870:5 | "each read goes to the command's page's read words" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3884:1 | "pipe2 writes two int descriptors to the caller's word `word`, read at once" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3898:1 | "close, read, rt_sigaction, rt_sigprocmask, execve, write and exit_group of the child's own descriptors and itself; `page` is its copy of the counted run's page, whose start is a zeroed sigaction, and execve reads the argument block" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3921:1 | "the attributes go to the caller's zeroed counted-run page `page`, which perf_event_open only reads" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3953:5 | "the attributes go to the fresh page's attribute words" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3972:5 | "each read goes to the counters' own page's read words" |
| Compiler | Compiler/JetBackend/Source/Image/Loader.jet | 3985:1 | "read fills three words of the caller's counted-run page `page`" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 67:9 | "the scratch mapping is the child's own, made above" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 144:9 | "getrandom fills the first word of our scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 151:9 | "getrandom wrote the scratch page's first word" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 187:1 | "pipe2 writes two descriptors into the first word of the caller's scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 197:9 | "fcntl duplicates our own descriptor" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 211:5 | "closes our own descriptor" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 227:5 | "fcntl F_SETOWN names our own child as the owner of our own descriptor's signals" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 232:5 | "fcntl F_SETFL sets O_ASYNC on our own descriptor" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 241:1 | "the caller owns the `size` bytes at `buffer`, a multiple of 8" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 263:5 | "the buffer is the child's scratch mapping past its first page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 288:9 | "the poll set is our scratch page's first three entries, which poll reads and writes; the clock words lie past them" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 311:9 | "poll wrote the entries' returned events" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 324:13 | "os_read just wrote `moved` bytes past the scratch page's first page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 340:9 | "the poll set is our scratch page's first two entries" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 345:9 | "poll reads and writes the two entries of our scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 353:9 | "poll wrote the entries' returned events" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 373:5 | "os_read just wrote `moved` bytes past the scratch page's first page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 383:1 | "the caller's `count` bytes at `address` lie in our mapping" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 391:1 | "clock_gettime writes two words at scratch + 64, past the poll set of the caller's scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 404:9 | "kills our own child" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 415:9 | "wait4 writes the status word into our scratch page" |
| Compiler | Compiler/JetBackend/Source/Image/SessionHost.jet | 420:5 | "wait4 wrote the status word" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 153:1 | "a raw Linux x86-64 system call; the caller passes the call number and arguments that call expects" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 179:5 | "the caller owns `count` writable bytes at `address`" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 186:5 | "the caller owns `count` readable bytes at `address`" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 205:5 | "the caller owns `address .. address + size` when it names one, so a fixed mapping replaces only its own pages; the pages mapped are the caller's alone" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 214:5 | "the caller owns the mapping it changes" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 222:5 | "the caller owns the mapping and nothing uses it any more" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 236:5 | "the caller owns `address .. address + size` when it names one, so a fixed mapping replaces only its own pages; the pages mapped are the caller's alone" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 244:5 | "the caller owns the mapping and nothing uses it any more" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 254:5 | "the caller owns the mapping and keeps no pointer into it past this call but the result" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 259:1 | "stores one word at an address the caller owns and has mapped writable" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 266:1 | "reads one word at an address the caller owns and has mapped readable" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 273:1 | "reads one byte at an address the caller owns and has mapped readable" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 280:1 | "stores one byte at an address the caller owns and has mapped writable" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 291:1 | "the caller owns `bytes.len()` rounded up to 8 bytes at `address`" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 297:1 | "the caller owns `count` rounded up to 8 bytes at `address`" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 306:1 | "calls machine code; the caller vouches for the code and its System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 319:1 | "calls machine code; the caller vouches for the code and its two-word System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 334:1 | "calls machine code; the caller holds the mapping and its checked zero-argument Unit System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 344:1 | "calls machine code; the caller holds the mapping and its checked three-word Unit System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 360:1 | "calls machine code; the caller holds the mapping and its checked one-word Unit System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 372:1 | "calls machine code; the caller holds the mapping and its checked two-word Unit System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 389:1 | "a raw Linux x86-64 syscall; the caller supplies the syscall's valid machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 406:1 | "reads one aligned machine word in live allocator-owned mapped memory" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 413:1 | "stores one aligned machine word in live allocator-owned mapped memory; x86 TSO makes this a release store" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 422:1 | "atomically compares and exchanges one aligned live machine word; every non-atomic access to protected data holds its lock" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 437:1 | "x86 pause only hints that this thread is waiting in a spin loop" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 447:1 | "cpuid only reports processor identification; rbx is saved and restored" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 460:1 | "cpuid only reports processor identification; rbx is saved and restored" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 474:1 | "cpuid only reports processor identification; rbx is saved and restored" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 487:1 | "cpuid only reports processor identification; rbx is saved and restored" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 502:1 | "xgetbv only reads an extended control register; the caller has seen CPUID.1:ECX.OSXSAVE" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 516:1 | "rdtsc only reads the time-stamp counter; the caller has seen it enabled for this process" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 532:1 | "reads 32 state bytes at state, 64 block bytes at data and 272 table bytes at table, and writes the 32 state bytes; the caller has seen the SHA extensions" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 729:1 | "reads 176 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 807:1 | "reads 208 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 895:1 | "reads 240 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 993:1 | "reads 176 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1071:1 | "reads 208 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1159:1 | "reads 240 round-key bytes at keys and 64 bytes at src and at mask, and writes 64 bytes at out; the caller has seen AES-NI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1261:1 | "reads 176 round-key bytes at keys and 16 bytes at src, pre and post, and writes 16 bytes at chain and at out; the caller has seen AES-NI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1297:1 | "reads 208 round-key bytes at keys and 16 bytes at src, pre and post, and writes 16 bytes at chain and at out; the caller has seen AES-NI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1337:1 | "reads 240 round-key bytes at keys and 16 bytes at src, pre and post, and writes 16 bytes at chain and at out; the caller has seen AES-NI" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1390:1 | "reads and writes 16 bytes at y and reads 16 bytes at data and at h and the 16-byte mask at table; the caller has seen PCLMULQDQ and SSSE3" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1465:1 | "reads and writes 16 bytes at y and reads 64 bytes at data and at powers and the 16-byte mask at table; the caller has seen PCLMULQDQ and SSSE3" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1589:1 | "reads the stack pointer register after one balanced push; no memory is read" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1605:1 | "the caller owns count readable bytes at source and count writable bytes at target; an overlap has target below source" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1626:1 | "the caller owns count readable bytes at source and count writable bytes at target, and count is 1 through 3" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1649:1 | "the caller owns count readable bytes at source and count writable bytes at target, and count is 4 through 7" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1664:1 | "the caller owns count readable bytes at source and count writable bytes at target, and count is 8 through 16" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1679:1 | "the caller owns count readable bytes at source and count writable bytes at target, and count is 16 through 32" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1694:1 | "the caller owns count readable bytes at source and count writable bytes at target, and count is 32 through 64" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1713:1 | "the caller owns count readable bytes at source and count writable bytes at target, and count is 64 through 128" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1742:1 | "the caller owns 64 readable bytes at source and 64 writable bytes at target" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1760:1 | "the caller holds 64 readable bytes at left and at right" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1788:1 | "the caller owns count writable words starting at target" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1808:1 | "the caller owns count writable bytes at target, and count is 1 through 3" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1824:1 | "the caller owns count writable bytes at target, and count is 4 through 7" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1836:1 | "the caller owns count writable bytes at target, and count is 8 through 16" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1847:1 | "the caller owns count writable bytes at target, and count is 16 through 32" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1860:1 | "the caller owns count writable bytes at target, and count is 32 through 64" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1877:1 | "the caller owns count writable bytes at target, and count is 64 through 128" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1903:1 | "the caller owns 64 writable bytes at target" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1919:1 | "the caller owns count writable bytes at target" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1934:1 | "reads one byte from a live readable mapped address supplied by the caller" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1941:1 | "writes one byte to a live writable mapped address supplied by the caller" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1951:1 | "writes at most eight bytes from a stack-local word while that word is live; the balanced pushes and pops do not expose stack storage after return" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 1972:1 | "bytes lends length readable bytes; the second iovec lends a stack-local LF; writev retains neither pointer and every push is balanced" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2009:1 | "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2056:1 | "the calling thread is attached and GS points at its live Jet TCB" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2065:1 | "ARCH_GET_GS writes one initialized aligned stack word only during this syscall; no TLS or heap is read, and the push/pop is balanced" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2085:1 | "base is a live self-addressed Jet TCB the calling thread runs on until the next switch, and the kernel enabled FSGSBASE" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2099:1 | "only a signal restorer's own frame calls this, on the stack its handler returned to" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2117:1 | "save is a writable word the resumer reads; target is a stack this switch or a fresh fiber seed left, owned by no running thread" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2142:1 | "atomically adds to one aligned live word and returns its previous value" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2155:1 | "atomically exchanges one aligned live word; xchg has an implicit full barrier" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2168:1 | "calls executable System V code with one raw word argument" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2181:1 | "calls a Jet closure's code with the live closure header and one raw word argument" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2199:1 | "calls C code; the caller vouches for the code and its word-argument System V signature" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2226:1 | "raw clone child runs only Jet/syscall code, never Rust or musl; supplied stack contains entry and argument" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2267:1 | "calls checked System V Unit code; the caller lends one live raw machine-word argument" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2279:1 | "calls checked System V Unit code; the caller vouches for both raw machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2293:1 | "calls checked System V Unit code; the caller vouches for all three raw machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2309:1 | "calls a checked native runtime callback with one machine-word argument" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2322:1 | "calls a checked native runtime callback with two machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2337:1 | "calls a checked native runtime callback with three machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2354:1 | "calls a checked native runtime callback with four machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2373:1 | "calls a checked native runtime callback with five machine-word arguments" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2394:1 | "ioctl reads one stack-local machine word with the supplied audited request and live fd; it retains no pointer and all pushes are balanced" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2417:1 | "object is a live fd for fstat or a terminated path for stat/lstat; output lends 128 writable bytes; the initialized stack stat frame never escapes" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2506:1 | "the audited caller lends the stat scalar row and valid syscall object" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2508:5 | "packed Linux 32-bit fields become independent portable machine words without any allocation" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2519:1 | "calls a checked System V void(void) process entry that remains executable for the entire call" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2529:1 | "directory and path identify a source-approved statx object; output lends three writable words; the zeroed 256-byte statx stack buffer never escapes" |
| Compiler | Compiler/JetBackend/Source/OS/Linux.jet | 2631:1 | "calls machine code; the caller vouches for its three-word System V signature" |
| Compiler | Compiler/JetCLI/Source/CLI/Host.jet | 2521:9 | "I2: jet alone among Jet programs ends a Jet defect with status 101, which process.exit maps to 1 for every program (S36), so its ICE exit takes the unmapped process exit" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 201:5 | "pointer is a fresh payload whose align &gt;= 8 block reserved the word 24 bytes before it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 209:5 | "pointer is an align &gt;= 8 payload being freed with its allocated size; its block reserved the count word 24 bytes before it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 241:5 | "anonymous private mmap owns fresh writable bytes and cannot alias a live heap block; a failed syscall produces no allocation" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 261:5 | "the caller owns this mapping and uses no pointer into it again" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 271:5 | "a fresh shared mapping of the descriptor aliases no heap block; a failed syscall maps nothing" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 283:5 | "the caller owns the reservation the fixed mapping replaces, and a shared mapping of the descriptor aliases no heap block" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 291:5 | "the caller owns these whole pages of its own mapping and keeps no live value in them" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 331:5 | "the root and span tables are process-lifetime mappings, zero where nothing is registered, whose aligned words are read atomically on x86" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 362:5 | "the caller holds the page lock, which serializes every table write; a fresh table is zero-filled before the aligned store that publishes it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 387:5 | "the root and span tables of the range are mapped for the process's lifetime, and each unit's aligned word is written only by the range's owner" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 470:5 | "a fresh inaccessible NORESERVE mapping aliases nothing; its link and record units are remapped writable inside it and its span tables mapped before its base is published, and a caller that loses the publication unmaps only its own mapping" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 515:9 | "row is the running TCB's own run stack of this class, and the link word of a run on it is this thread's alone" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 553:5 | "the fresh run is this caller's alone, inside the window and inaccessible; the fixed mapping replaces only its own units" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 590:5 | "row is the running TCB's own run stack of this class, and the ended run's link word is no other thread's" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 671:5 | "the region has ended, so nothing reads or writes its pages; MADV_DONTNEED drops only its own units inside the window, which read as zero again" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 765:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 768:5 | "the fresh state page is private to initialization, which precedes every allocation" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 784:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 813:5 | "initialization wrote the arena table word once, before any allocation" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 828:5 | "reads the stack pointer register only; nothing is dereferenced" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 842:9 | "the live totals are process-lifetime arena words; the quiescent caller needs no lock to read them" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 871:5 | "root is an aligned lock word in the process-lifetime state mapping; CAS/xchg acquire all metadata it protects and the futex wait only reads that word" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 890:5 | "the calling thread holds this lock; xchg publishes its completed metadata updates and reports sleepers, and the futex wake only names the word" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 931:5 | "slab and its linked neighbors are live heap-owned headers protected by the held heap lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 949:5 | "head and slab are live heap-owned metadata protected by the held heap lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 985:5 | "the slab-region lock owns the cursor, end and free-slab stack words and every stacked slab's link word" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1021:5 | "the slab-region lock owns the free-slab stack; the slab is unlinked from every class list and holds no live or cached block" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1037:5 | "madvise only changes the page size of the fresh private region" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1049:5 | "the slab is this arena's alone: fresh from its region or off the free-slab stack; the held arena lock excludes all list readers" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1075:5 | "the held arena lock protects the class list, the slab's free links, carve word and live count, the retained count and the arena live total; row is the caller's exclusive cache row; the blocks leave the slab before any caller sees them" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1124:5 | "the held arena lock protects the slab's free links, carve word, live count, class list, retained count and live total; the block is no longer any payload or cache entry" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1157:5 | "the held arena lock makes the arena's scratch row exclusive; it is emptied again before the lock is released" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1166:5 | "block is this caller's exclusive block; its two cached words are read before the payload header overwrites them" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1181:5 | "row is this thread's private cache row" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1199:5 | "row is this thread's private cache row; the flag is idempotent: every writer stores the same 1" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1214:5 | "row is this thread's private cache row and block its cached head; both cached words are read before the payload header overwrites them" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1229:5 | "row is this thread's private cache row; each cached block's slab is live while the block is cached, so its arena word is readable, and the block returns under that arena's lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1260:9 | "row is one of this thread's eight private cache rows" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1274:5 | "tcb is a live TCB whose eight cache rows start at @320" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1283:9 | "row is one of the TCB's eight cache rows, which only the thread running on it touches" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1308:13 | "row is one of the running TCB's eight private cache rows" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1351:9 | "row is one of the switched-out fiber's eight cache rows, which no thread runs on now" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1440:9 | "row is one of the running TCB's kept-mapping rows, which only the thread running on it touches" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1462:9 | "row is one of the running TCB's kept-mapping rows, which only the thread running on it touches" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1479:5 | "the large-cache lock owns every class list; the mapping's first word is no live payload once it is freed" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1540:5 | "the large-cache lock owns every class list and its link words" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1580:5 | "pointer's two header words and the aligned payload are wholly within this page-rounded class mapping" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1603:5 | "pointer is a live direct allocation of exactly `size` bytes at align 1 whose two header words name its mapping and the mapping's bytes; the mode and tracked-count words are aligned words read atomically on x86, as alloc_reserve and alloc_untrack read them; a refused mremap leaves the mapping as it was" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1643:9 | "row is this thread's private cache row" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1662:5 | "the caller holds the policy lock and cell is a counter in alloc_root" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1670:5 | "the aligned mode word is read atomically on x86; system allocation needs no policy lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1676:5 | "the policy lock exclusively owns the mode, cap and accounting words" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1705:5 | "the policy lock exclusively owns live bytes; restoration does not erase outstanding tracked blocks" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1719:5 | "the held policy lock serializes all native allocator configuration and reservations" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1736:5 | "the policy lock serializes restoration with native reserve and free; tracked blocks remain in the side table" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1750:5 | "the policy lock protects this bounded fact word in alloc_root" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1765:5 | "the private raw-heap node is fully initialized before linking under the policy/tracker lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1778:5 | "the process-lifetime tracker count is an aligned x86 word; a zero count avoids any lookup or lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 1785:5 | "the policy/tracker lock exclusively owns all side-table links and count" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2085:5 | "reads the stack pointer register only; nothing is dereferenced" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2338:13 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row (TCB @320 + 16 per class, alloc_cache_row) and block its cached head, whose two cached words are read before the payload header overwrites them; an align &lt;= 8 payload at block + header is aligned because blocks start 32-aligned" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2385:13 | "as in jet_rt_alloc: GS:0 is the attached thread's TCB or the readable zero word; the aligned mode word is read atomically on x86; row is this thread's private cache row and block its cached head, whose two cached words are read before the payload header overwrites them; an align &lt;= 8 payload at block + header is aligned because blocks start 32-aligned" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2421:13 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count and the page-map root are aligned words read atomically, as alloc_untrack and alloc_page_region read them; row is this thread's private cache row; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2446:5 | "pointer is an align-8 payload being freed; its block reserved the count word 24 bytes before it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2486:17 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row of the size's class and block its cached head, whose next word is read before the count overwrites it; the block's second word is its slab word, the payload header's; the payload at block + 24 is 8-aligned because blocks start 32-aligned" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2519:21 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count and the page-map root are aligned words read atomically, as alloc_untrack and alloc_page_region read them; row is this thread's private cache row of the size's class; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's next word overwrites it; the block's second word stays its slab word; a count word with the zero-at-free mark takes the general entry, which wipes the payload" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2555:13 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row (TCB @320 + 16 per class, alloc_cache_row) and block its cached head, whose two cached words are read before the payload header overwrites them; an align &lt;= 8 payload at block + header is aligned because blocks start 32-aligned" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2600:17 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row of the size's class and block its cached head, whose next word is read before the count overwrites it; the block's second word is its slab word, the payload header's; the payload at block + 24 is 8-aligned because blocks start 32-aligned" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2643:13 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the aligned mode word is read atomically on x86, as alloc_reserve reads it; row is this thread's private cache row of the size's class and block its cached head, whose two cached words are read before the payload header overwrites them; an align-1 payload at block + 16 needs no alignment" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2671:13 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); the tracked count and the page-map root are aligned words read atomically, as alloc_untrack and alloc_page_region read them; row is this thread's private cache row of the size's class; the caller frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2709:9 | "row is this thread's private cache row; the lowering frees exactly once with the allocation's original layout, so the payload header is read before the block's cached words overwrite it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2725:5 | "the lowering returns this still-live direct allocation with exactly its original size and alignment" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2737:5 | "the caller's live payload header names its slab, whose arena word is fixed when the slab is created" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2742:5 | "the lowering frees exactly once with the allocation's original layout; its slab remains live until this last payload is returned under its arena's lock" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2808:5 | "the fresh state page is private to initialization, which precedes every allocation" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2811:5 | "checked-heap start runs during runtime-init, before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2825:5 | "the state page's stub word, written only here" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2832:5 | "the fresh stub page is private until its address is stored; it becomes read+execute before any call" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2851:5 | "the frame stub (alloc_frames_start) only copies rbp to rax and returns; a missing stub leaves no chain" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2859:13 | "frame is an active caller's [saved rbp][return address] pair on this thread's stack: each followed link ascends by less than 16 MiB or crosses a stack segment's bridge frame" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2864:9 | "out lends count writable words: a checked block's chain words or a trap's scratch page" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2879:5 | "the frame stub (alloc_frames_start) only copies rbp to rax and returns; a missing stub leaves no chain" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2888:9 | "frame is an active caller's [saved rbp][return address] pair on this thread's stack: each followed link ascends by less than 16 MiB or crosses a stack segment's bridge frame" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2895:13 | "into lends count writable words: a host call row's site words" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2914:5 | "block is this caller's fresh heap block of prefix + size + 8 bytes; every word written lies in block .. pointer + size + 8" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2954:5 | "a checked payload's canary word precedes it; the quarantine lock orders racing frees of one block and guards the live list" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2968:5 | "the live canary proves pointer is a checked payload whose header and tail this free now owns" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 2985:5 | "the quarantine lock owns the ring and its counters; every parked block is freed and owned by the quarantine" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3020:5 | "aligned whole-word stores; each checked take or give loads its observer word once" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3032:5 | "the observation start runs before the entry; a take links a block only once the entry starts the list" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3047:5 | "an aligned whole-word store; a take on the observation's writer thread that loads the switch before it links no block, which its free leaves alone" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3059:5 | "an aligned whole-word store; a take that loads the word before it is a take of another thread's" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3074:5 | "GS:0 is the attached thread's TCB or, unattached, the readable zero word (thread_base); only an attached TCB's own-takes word is read" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3096:5 | "GS:0 is the attached thread's TCB, whose own-takes word only this thread writes; an unattached thread's takes link no block, so it keeps no count" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3107:5 | "pointer is this take's fresh checked payload, whose freeing-chain words are unused while it lives; the held quarantine lock guards the list and its blocks' link words" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3120:5 | "the live canary proves pointer a checked payload; the held quarantine lock guards the list and its blocks' link words" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3138:5 | "the held quarantine lock guards the count" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3154:1 | "pointer is 0 or a tracked live payload of the held live list" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3161:1 | "pointer is a tracked live payload of the held live list" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3172:1 | "the caller holds the live list (alloc_check_track_hold), or is a snapshot's forked child, whose one thread is the holder, and puts the head back before it lets the list go" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3184:5 | "the frame stub (alloc_frames_start) only copies rbp to rax and returns; a missing stub leaves no chain" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3190:5 | "frame is this call's live [saved rbp][return address] pair on this thread's stack" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3198:5 | "the freeing caller owns size payload bytes at pointer" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3213:5 | "pointer is a parked checked payload owned by the quarantine; its header, payload and tail stay mapped until retired" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3240:5 | "the freed canary proved the checked header" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3249:5 | "pointer is an inspected parked checked payload; its header still names the enclosing block and layout" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3280:5 | "the quarantine lock owns the ring and its counters" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3296:5 | "data lends length readable bytes; write retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3333:5 | "path is a fresh private page; the image is mapped read-only and only read" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3357:5 | "image is a read-only private mapping of this executable's ELF64 file; its program header table lies inside it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3386:9 | "image is a read-only private mapping of this executable's ELF64 file; every offset read comes from its own section and symbol tables" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3429:9 | "words lends count readable chain words" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3475:9 | "the live canary proved the checked header" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3524:5 | "remaining is in 1..=8, so write borrows only the initialized stack word; a short write advances within it" |
| Compiler | Compiler/JetRuntime/Source/Alloc.jet | 3583:5 | "the stack-word syscalls unblock SIGABRT and, if necessary, restore its default disposition before raising it again; no heap pointer is needed" |
| Compiler | Compiler/JetRuntime/Source/Authority.jet | 399:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CABISession.jet | 21:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CABISession.jet | 27:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Channel.jet | 198:5 | "an idempotent flag: every writer stores the same 1" |
| Compiler | Compiler/JetRuntime/Source/CompilerFS.jet | 266:9 | "the prefix remains terminated and scratch lends 128 writable portable stat-fact bytes" |
| Compiler | Compiler/JetRuntime/Source/CompilerFS.jet | 288:5 | "name is NUL-terminated and scratch lends 128 writable portable stat-fact bytes" |
| Compiler | Compiler/JetRuntime/Source/CompilerHost.jet | 12:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CompilerHost.jet | 164:5 | "page is a fresh private page: the path, then the [type, value] pairs read into it" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 59:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 260:5 | "the descriptor is this map's own; mmap creates exactly length read-only private bytes; the map owner alone unmaps them" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 273:5 | "the map owner unmaps its own mapping once, after its pages left the sentry's live storage" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 282:5 | "closes a temp-file descriptor this map alone owns" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 290:5 | "fcntl releases the lease on the map's own descriptor" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 301:5 | "fstatfs writes one 120-byte statfs into the map's 160-byte scratch" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 348:5 | "FICLONE shares the file's blocks with the empty unnamed spare this map owns" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 366:9 | "copy_file_range and sendfile move bytes between two descriptors this map owns; the offset word is the caller's" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 380:5 | "ftruncate sizes the unnamed temp file this map owns" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 417:5 | "fcntl names the lease-break signal for, then takes a read lease on, the map's own read-only descriptor" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 442:9 | "rt_sigaction borrows the four-word action on the stack; the handler is this runtime's three-word C-ABI export and the restorer its rt_sigreturn rail" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 446:9 | "written under the map registry mutex, before any lease can name a slot" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 509:9 | "memfd_create reads the slot's NUL-terminated name word; MFD_CLOEXEC" |
| Compiler | Compiler/JetRuntime/Source/CompilerMapped.jet | 531:5 | "MAP_FIXED replaces exactly the held map's own read-only private pages with the same length of its copy; borrowed views keep their addresses" |
| Compiler | Compiler/JetRuntime/Source/Comptime.jet | 18:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/ComptimeServe.jet | 95:5 | "the filter is installed once, on the image's only thread, before any call it traps" |
| Compiler | Compiler/JetRuntime/Source/ComptimeServe.jet | 181:13 | "the compiler admits between calls of the one thread it drives; the handler only reads the flag" |
| Compiler | Compiler/JetRuntime/Source/ComptimeServe.jet | 196:5 | "the serve entry runs once, on the image's only thread, before it serves" |
| Compiler | Compiler/JetRuntime/Source/Context.jet | 363:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Coverage.jet | 25:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CryptoAES.jet | 26:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CryptoAES.jet | 71:5 | "the caller holds (rounds + 1) round keys at keys and four blocks at src, mask and out" |
| Compiler | Compiler/JetRuntime/Source/CryptoAES.jet | 99:5 | "the caller holds (rounds + 1) round keys at keys and one block at src, pre, post, out and chain" |
| Compiler | Compiler/JetRuntime/Source/CryptoAES.jet | 246:5 | "y and h hold one block, the range check keeps every block inside the length borrowed bytes, and the powers table holds four" |
| Compiler | Compiler/JetRuntime/Source/CryptoEntropy.jet | 23:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/CryptoEntropy.jet | 69:5 | "Linux getrandom flags0 writes only the exclusively borrowed writable span and retains no address; kernel negative errno is preserved" |
| Compiler | Compiler/JetRuntime/Source/DynCodec.jet | 137:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/DynCodec.jet | 167:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/EnvOverlay.jet | 40:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 63:5 | "the image's entry calls this before it starts any other thread" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 78:9 | "jet_rt_main calls this on the main thread before the entry runs" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 97:5 | "rt_sigaction borrows the four-word action on the stack; handler is this runtime's C-ABI export with the (signal, info, context) signature" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 110:5 | "stack is a fresh private mapping; sigaltstack copies its first three words (ss_sp, ss_flags, ss_size)" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 130:5 | "stack is this thread's registered alternate stack, unused outside a handler; sigaltstack copies its first three words" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 177:5 | "rt_sigaction borrows the four-word SIG_DFL action on the stack; tgkill signals this thread" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 234:5 | "stub is a fresh private page that holds `mov rax, rbp; ret` and becomes read+execute before the call" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 256:9 | "path is a fresh private page; the executable is mapped read-only and only read" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 383:5 | "the trace made this mapping and holds no pointer into it past this call" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 473:5 | "image is a read-only mapping of this executable's ELF64 file; every offset read comes from its own header and section table" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 508:9 | "frame is an active caller's [saved rbp][return address] pair on this thread's stack" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 515:9 | "each followed link ascends by less than 16 MiB or crosses a stack segment's bridge frame, so frame stays on this thread's chain" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 550:9 | "frame is an active caller's [saved rbp][return address] pair on this thread's stack" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 563:9 | "each followed link ascends by less than 16 MiB or crosses a stack segment's bridge frame, so frame stays on this thread's chain" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 633:9 | "block + 12 is inside this block, which lies in the read-only table" |
| Compiler | Compiler/JetRuntime/Source/Fault.jet | 726:5 | "at lies in a mapping the trace reads: the executable's read-only ELF header, program or section table, notes or location table, or the page the host's frames were written to" |
| Compiler | Compiler/JetRuntime/Source/FaultAllocation.jet | 21:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/FaultSchedule.jet | 34:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Fiber.jet | 133:5 | "the carrier saved its stack at carrier+16 when it switched to this fiber and runs nothing else until this switch resumes it" |
| Compiler | Compiler/JetRuntime/Source/Fiber.jet | 160:5 | "sched_yield retains no memory and yields this OS thread" |
| Compiler | Compiler/JetRuntime/Source/Fiber.jet | 230:9 | "base is a live self-addressed TCB this carrier runs on until its next switch, and the kernel reported FSGSBASE" |
| Compiler | Compiler/JetRuntime/Source/Fiber.jet | 266:9 | "the fiber's saved stack is owned by no running thread: it is fresh, pooled or committed PARKED/RUNNABLE, and this carrier alone took it" |
| Compiler | Compiler/JetRuntime/Source/Fiber.jet | 349:9 | "an idempotent install: every writer stores 1, and a carrier that read the old 0 left its slice unstamped, which the read below stamps" |
| Compiler | Compiler/JetRuntime/Source/Fiber.jet | 476:13 | "written once, under the scheduler root lock, before any fiber can wait; a reader sees 0 (no fiber waiter exists) or the lot" |
| Compiler | Compiler/JetRuntime/Source/Fiber.jet | 527:9 | "a fresh private anonymous stack mapping, unreserved until touched" |
| Compiler | Compiler/JetRuntime/Source/Fiber.jet | 539:5 | "the first page of the fresh, untouched mapping becomes the stack's guard: a guard marker, else PROT_NONE; madvise only changes the mapping's page size" |
| Compiler | Compiler/JetRuntime/Source/Fiber.jet | 694:9 | "sched_yield retains no memory; the firing carrier clears the word right after its wake" |
| Compiler | Compiler/JetRuntime/Source/FilesPathOps.jet | 38:5 | "the terminated raw path and the 128-byte stat fact row are live for the syscall" |
| Compiler | Compiler/JetRuntime/Source/FilesStreams.jet | 34:9 | "the freshly opened owned descriptor and the 128-byte stat fact row are live for fstat" |
| Compiler | Compiler/JetRuntime/Source/FilesWalk.jet | 46:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 28:5 | "TCGETS writes its termios into the fresh mapping's scratch at 32 and retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 32:5 | "runtime startup publishes the root before any program thread exists" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 41:5 | "rt_sigaction synchronously borrows the four-word SIG_IGN action; no signal handler or restorer runs for ignored SIGPIPE" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 58:5 | "the caller lends length readable bytes for the complete synchronous write" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 75:5 | "the caller lends length readable bytes; writev borrows its own LF until return" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 104:5 | "cpuid only reports processor identification" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 124:5 | "the output lock exclusively owns the initialized buffer and length; write borrows its pending bytes" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 244:5 | "the held host lock lends enough scratch for Linux TCGETS" |
| Compiler | Compiler/JetRuntime/Source/Host.jet | 314:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/HostDescriptors.jet | 40:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/HostDescriptors.jet | 180:9 | "bytes owns cap writable bytes; pread retains no pointer and the caller's Fd keeps the descriptor open" |
| Compiler | Compiler/JetRuntime/Source/HostDescriptors.jet | 373:9 | "scratch lends the pollfd and timespec and ppoll retains neither; the caller's Fd keeps the descriptor open" |
| Compiler | Compiler/JetRuntime/Source/HostDescriptors.jet | 400:9 | "bytes owns cap writable bytes; read retains no pointer and the caller's Fd keeps the descriptor open" |
| Compiler | Compiler/JetRuntime/Source/HostDescriptors.jet | 436:9 | "packed lends count bytes; write retains no pointer and the caller's Fd keeps the descriptor open" |
| Compiler | Compiler/JetRuntime/Source/HostDirectoryList.jet | 9:9 | "getdents supplies a live terminated name; openat describes the entry without following links" |
| Compiler | Compiler/JetRuntime/Source/HostDirectoryList.jet | 15:13 | "the owned descriptor and full Linux stat buffer live through fstat and close" |
| Compiler | Compiler/JetRuntime/Source/HostDirectoryList.jet | 47:9 | "an owned fresh directory descriptor and writable getdents64 buffer" |
| Compiler | Compiler/JetRuntime/Source/HostDirectoryList.jet | 82:5 | "close the fresh stream descriptor once after reading" |
| Compiler | Compiler/JetRuntime/Source/HostFileLock.jet | 83:5 | "the borrowed lock descriptor and 128 writable stat-fact bytes live through fstat" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 287:9 | "the caller lends 128 writable bytes for portable stat facts" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 383:5 | "the terminated borrowed path and 128-byte stat fact output are live for the syscall" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 451:9 | "the held directory and terminated source-approved single name stay live through openat" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 474:9 | "the held directory and terminated final name live through O_PATH&#124;O_NOFOLLOW&#124;O_CLOEXEC openat; no read access or link traversal" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 482:9 | "openat returned an exclusively owned descriptor and the caller lends StatFacts128; fstat copies initialized scalar facts" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 512:9 | "the descriptor is held live, output begins with the borrowed empty pathname, statx writes only its private stack frame" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 523:5 | "the terminated source-approved path and three writable birth-time output words stay live through statx" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 581:5 | "both terminated borrowed names and the caller's optional compact byte output remain live only through lgetxattr" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 613:9 | "the held directory, terminated plain name and writable byte range are borrowed only through readlinkat" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 641:5 | "held directory duplicates and terminated names prevent any descriptor-close/reuse race during renameat2" |
| Compiler | Compiler/JetRuntime/Source/HostFiles.jet | 665:9 | "the held fd and stack-local source-validated winsize word live only through ioctl" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 18:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 215:13 | "the owned walk descriptor and terminated part live through openat" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 280:13 | "the walk directory and terminated final part live through openat" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 446:5 | "the terminated borrowed path and the 256-byte statx buffer are live for the syscall" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 550:13 | "the walk directory and terminated final part live through O_PATH&#124;O_NOFOLLOW&#124;O_CLOEXEC openat" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 556:17 | "the owned O_PATH descriptor and the caller's 128-byte stat facts live through fstat" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 563:21 | "the owned O_PATH descriptor, birth's zeroed empty pathname and three writable words live through statx" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 642:5 | "both walk directories and terminated final parts live through renameat/renameat2" |
| Compiler | Compiler/JetRuntime/Source/HostHeldDirectory.jet | 656:9 | "both walk directories and terminated final parts live through linkat" |
| Compiler | Compiler/JetRuntime/Source/HostNetLookup.jet | 271:5 | "socket(AF_UNIX, SOCK_DGRAM&#124;SOCK_CLOEXEC, 0) takes no pointer" |
| Compiler | Compiler/JetRuntime/Source/HostNetLookup.jet | 286:5 | "ioctl(SIOCGIFINDEX) reads and writes exactly the owned 40-byte ifreq; close releases the descriptor opened here" |
| Compiler | Compiler/JetRuntime/Source/HostNetLookup.jet | 348:5 | "open(2) reads the owned NUL-terminated path; O_RDONLY&#124;O_CLOEXEC retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/HostNetLookup.jet | 364:5 | "close(2) releases the descriptor lk_open opened" |
| Compiler | Compiler/JetRuntime/Source/HostNetLookup.jet | 380:9 | "read(2) fills at most LK_READ bytes of the reader's owned buffer" |
| Compiler | Compiler/JetRuntime/Source/HostNetPoller.jet | 45:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/HostNetPoller.jet | 104:5 | "the shard lock owns a packed 12B epoll_event; state retains the native fd through this synchronous call" |
| Compiler | Compiler/JetRuntime/Source/HostNetPoller.jet | 140:9 | "events owns 64 packed epoll_event entries; epoll_wait retains no buffer pointer" |
| Compiler | Compiler/JetRuntime/Source/HostParallelism.jet | 486:5 | "sched_getaffinity writes at most the owned 128-byte mask; pid0 names this thread" |
| Compiler | Compiler/JetRuntime/Source/HostParallelism.jet | 563:9 | "the probe answer is fixed for the process; a racing probe stores the same word" |
| Compiler | Compiler/JetRuntime/Source/HostParallelism.jet | 578:9 | "xgetbv only reports the OS-enabled state, which CPUID.1:ECX has just shown the OS reports (OSXSAVE)" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 8:5 | "the audited caller supplies the Linux syscall's valid word arguments" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 36:1 | "a raw Linux x86-64 syscall; the caller supplies the syscall's valid machine-word arguments" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 213:5 | "rt_sigaction borrows the four-word SIG_DFL action on the stack and writes no old action" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 255:5 | "buffer owns the sixteen-byte struct rlimit64 the kernel writes" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 283:5 | "buffer owns the sixteen-byte struct rlimit64 the kernel reads and does not retain" |
| Compiler | Compiler/JetRuntime/Source/HostProcess.jet | 329:5 | "status lends an eight-byte out slot; wait4 writes its low four bytes and retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 14:9 | "the FD72 lock prevents native descriptor close/reuse throughout this nonblocking syscall" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 62:13 | "accept4 retains no address pointers and atomically sets CLOEXEC/NONBLOCK on the accepted socket" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 69:13 | "private scratch lends sockaddr bytes and its exact 4B socklen; accept4 retains neither pointer" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 108:9 | "private scratch lends one native int and its 4B socklen; getsockopt retains neither pointer" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 124:9 | "private scratch lends exactly one native int; setsockopt retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 166:9 | "the held FD72 lock lends its live nonblocking socket; packed lends exactly length bytes; no pointer survives recvfrom/sendto" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 208:9 | "the FD72 lock retains the socket; private scratch lends sockaddr bytes; packed lends length bytes; sendto retains no pointers" |
| Compiler | Compiler/JetRuntime/Source/HostSockets.jet | 228:9 | "private scratch lends sockaddr/socklen and packed lends length mutable bytes; MSG_TRUNC returns the complete datagram length while writing at most length bytes" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 333:13 | "rt_sigaction borrows four initialized stack words; no allocator, mutex or TLS is touched after fork" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 440:13 | "rt_sigaction writes the four-word old action into this spawn's block; no allocator, mutex or TLS is touched in the child" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 450:5 | "rt_sigprocmask reads the parent's saved mask word in this spawn's block" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 505:5 | "the masks are words of this spawn's block; the child runs only jet_rt_process_spawn_child on the mapped stack and leaves through execve or exit_group before the parent resumes" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 904:9 | "ioctl borrows one stack word of source-validated winsize dimensions; no pointer escapes" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 987:21 | "readlink borrows the NUL-terminated output path and writes at most p (or the one dummy) bytes of the owned stack" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 1028:5 | "getcwd writes at most 4097 bytes of the owned stack" |
| Compiler | Compiler/JetRuntime/Source/HostSpawn.jet | 1087:5 | "source lends a NUL-terminated raw path including non-Unicode bytes and a writable 128-byte stat fact row" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 38:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 101:5 | "host argument registration is single-threaded and precedes user code" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 112:5 | "host argument registration is single-threaded and precedes user code" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 144:5 | "registration is single-threaded and precedes user code" |
| Compiler | Compiler/JetRuntime/Source/HostStartup.jet | 191:5 | "the index is published once: by single-threaded registration, or inside host_environment_require's once before it completes" |
| Compiler | Compiler/JetRuntime/Source/HostStartupOutputFacts.jet | 113:5 | "Host startup already initialized its process-lifetime mapping; the held lock owns ioctl scratch32..47 and FIRST raw lookup only borrows the immutable captured index" |
| Compiler | Compiler/JetRuntime/Source/HostTime.jet | 19:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/HostTiming.jet | 28:9 | "the probe answer is fixed for the process; a racing probe stores the same word" |
| Compiler | Compiler/JetRuntime/Source/Int.jet | 459:13 | "the final owner has reached zero; the remaining shared hazard domain decides immediate free or retirement" |
| Compiler | Compiler/JetRuntime/Source/IntHazard.jet | 21:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/JourneyTLS.jet | 26:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/LogicalEnvironment.jet | 123:5 | "host argument registration is single-threaded and precedes user code" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 27:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 55:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 71:5 | "an idempotent flag: every writer stores the same 1" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 86:5 | "a whole-word setting: each setter stores one aligned word and the last setter wins, which is what setting it means" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 129:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 135:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 144:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 220:5 | "only the main thread running jet_rt_main writes its root control" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 312:5 | "only the main thread running jet_rt_main writes its root control" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 316:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 328:5 | "only the main thread running jet_rt_main writes its root control" |
| Compiler | Compiler/JetRuntime/Source/Main.jet | 355:5 | "the entry has returned and drained its workers; only the main thread runs here" |
| Compiler | Compiler/JetRuntime/Source/MemRegistry.jet | 22:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/NativeLayoutABI.jet | 98:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/NativeLayoutText.jet | 65:5 | "runtime-init (jet_rt_layout_init) runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/NetUnix.jet | 431:5 | "private scratch lends one native int and its 4B socklen; getsockopt retains neither pointer" |
| Compiler | Compiler/JetRuntime/Source/NetUnix.jet | 551:13 | "the FD72 lock keeps the stream's descriptor live and unreused for the whole ppoll; the caller's Fd keeps the wake descriptor open; scratch lends the pollfds and timespec" |
| Compiler | Compiler/JetRuntime/Source/Observe.jet | 29:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Observe.jet | 35:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Observe.jet | 235:5 | "an idempotent flag: every writer stores the same 1" |
| Compiler | Compiler/JetRuntime/Source/Observe.jet | 485:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/ObserveLive.jet | 83:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/ObserveLive.jet | 184:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/ObserveLive.jet | 234:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/ObserveLive.jet | 403:5 | "the vetted Linux calls of Observe and the profiler take words alone or borrow native buffers only for their duration" |
| Compiler | Compiler/JetRuntime/Source/ObserveLive.jet | 708:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 711:5 | "the observation start runs before the timer whose handler reads these is armed" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 729:9 | "no timer is armed yet, so no handler reads these before they are set" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 743:9 | "no timer is armed, so no handler reads the descriptor" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 757:5 | "only the exit reads it, and no exit runs before the start returns" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 780:5 | "rt_sigaction borrows the four-word action on the stack; the handler is this runtime's three-word C-ABI export and the restorer its rt_sigreturn rail" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 820:5 | "clock_gettime writes the sampler's two words of the thread-owned TCB; no pointer escapes" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 878:5 | "timer_create reads the sigevent and writes the timer id in the scratch words" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 896:13 | "timer_settime reads the itimerspec in the scratch words" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 984:5 | "only the observation writer changes it, under the flight recorder's lock; a starting thread reads it again after it arms" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 1001:17 | "timer_settime reads the itimerspec this call owns" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 1031:5 | "set once, before the table is published; a host call or spawn that reads perf_system 0 keeps no row" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 1312:9 | "the table and the depth are set before the first sampler is armed, so no handler reads them before" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 1322:17 | "no sampler is armed yet, so no thread reads the source or its period" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 1329:21 | "no sampler is armed yet, so no thread reads the source" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 1338:13 | "no sampler is armed yet, so no thread reads the source" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 1345:13 | "no sampler is armed, so no handler reads the table" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 1356:9 | "eight writes of a stack-local word, which the kernel copies before each call returns" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 1414:5 | "perf_event_open reads the attribute words of the scratch" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 1496:5 | "perf_event_open reads the attribute words of the scratch" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 2389:9 | "one write of a stack-local word, which the kernel copies before the call returns" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 2589:5 | "the exit and a window's close, under the flight recorder's lock, alone clear it; a handler reads it only while it holds its buffer, which the claims below order" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 2624:5 | "clock_gettime writes the two timespec words of the scratch" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 2832:9 | "registrations never overlap, since constructors run one at a time, and the fence below orders this store before a turn reads the tables" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 2876:9 | "registrations never overlap, and a turn reads the page only once it is stored" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 2891:5 | "the observation start, before the entry, and then its writer store the same roots" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 2907:9 | "turns run one at a time, the observation start's before the entry and then its writer's, and a registration reads the code only after the zones word" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 2915:9 | "turns run one at a time, the observation start's before the entry and then its writer's, and a zone route reads the rings' head and a registration the lock only after the zones word" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 2920:5 | "turns run one at a time; the lock orders their patches with a registration's" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3026:5 | "pwrite64 copies one byte from the thread-owned TCB scratch and retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3072:5 | "rdtsc only reads the time-stamp counter, which the zones' calibration found invariant and enabled for this process" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3089:5 | "the first turn of zones runs alone, before it stores the zones word that every zone route reads first" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3123:9 | "x86 pause only hints that this thread is waiting in a spin loop" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3631:5 | "fork copies the held live list into a child whose only thread writes the snapshot and exits, never returning from here" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3679:5 | "this thread holds the live list, or is the fork's child, whose copy of it its one thread held" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3690:9 | "block is a tracked live payload of the held live list" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3703:9 | "block is a tracked live payload of the held live list" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3761:5 | "this thread holds the live list, or is the fork's child, whose copy of it its one thread held" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3774:5 | "this thread holds the live list, or is the fork's child, whose copy of it its one thread held" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3785:5 | "this thread holds the live list, or is the fork's child, whose copy of it its one thread held" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3790:9 | "block is a tracked live payload of the held live list" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3816:9 | "block is a tracked live payload of the held live list" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3847:9 | "link is a live [saved rbp][return address] pair of this thread's frame pointer chain, above the snapshot's own frames" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3874:9 | "link is a live [saved rbp][return address] pair of this thread's frame pointer chain, above the snapshot's own frames" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3906:5 | "this thread holds the live list, or is the fork's child, whose copy of it its one thread held; the head goes back below" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 3946:5 | "the head this scan took from the live list goes back before the list is let go" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 4126:5 | "the observation start runs before the program's entry, whose takes read it" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 4222:5 | "the held quarantine lock guards the count; every take reads it under the lock" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 4290:5 | "the observation start runs before the entry, whose span closes read it" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 4689:9 | "two writes of a stack-local word, which the kernel copies before each call returns" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 4743:5 | "fcntl reads the two-word f_owner_ex built at `owner`" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 4771:5 | "perf_event_open reads the attribute words the held buffer keeps" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 5028:5 | "five writes of a stack-local word, which the kernel copies before each call returns" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 5068:9 | "four writes of a stack-local word, which the kernel copies before each call returns" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 5077:5 | "the observation start runs before the exit that reads these" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 5092:5 | "the exit runs on one thread, after the start set these" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 5677:5 | "the observation start runs before the timer whose handler reads these is armed" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 5769:9 | "one write of a stack-local word, which the kernel copies before the call returns" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 5862:9 | "each followed link ascends by less than 16 MiB or crosses a stack segment's bridge frame, so frame stays on the thread's chain" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 5976:9 | "only the observation writer opens a window, under the flight recorder's lock" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 5987:5 | "only the observation writer opens a window, under the flight recorder's lock; a handler reads the descriptor once, and the buffering only while it holds its buffer (perf_samples_flush)" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 5998:5 | "only the observation writer opens a window, under the flight recorder's lock" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 6030:5 | "only the observation writer and the exit close a window, under the flight recorder's lock; a handler reads the descriptor once" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 6310:5 | "socketpair writes its two descriptors into the fresh state's receive buffer" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 6323:5 | "the observation start runs before the writer and the stop that read it" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 6345:9 | "read fills the state's receive buffer with one datagram" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 7048:9 | "only the observation writer opens a window, under the flight recorder's lock, and a record reads the state only once it is stored" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 7392:5 | "openat reads the image copy's NUL-terminated path in the state's text page" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 7740:5 | "the dump's calls take words alone or borrow the state's scratch or the process's own mappings for their duration" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 7844:5 | "the vectors name the state's copy and a range of this process's own mappings; the kernel copies between them, faulting on neither" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 8344:9 | "openat reads the dump's NUL-terminated path in the state's text page" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 8885:5 | "getrusage writes the struct rusage in this thread's own entry" |
| Compiler | Compiler/JetRuntime/Source/PerfSamples.jet | 9083:5 | "getcpu writes the CPU's 32-bit number into this thread's own entry" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 28:5 | "the kernel enters this restorer only from a returning handler, on its signal frame" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 40:5 | "rt_sigaction borrows the four-word action on the stack; handler is this runtime's one-word C-ABI export and the restorer its rt_sigreturn rail" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 63:9 | "written under the scheduler root mutex" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 224:5 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 284:9 | "written under the scheduler root mutex, after every word the handler and dispatcher read" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 334:5 | "rt_sigaction writes the signal's four-word action into the hub's preallocated table and sets none" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 338:5 | "rt_sigaction borrows the four-word action on the stack; the handler is this runtime's one-word C-ABI export and the restorer its rt_sigreturn rail" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 347:5 | "rt_sigaction reads the saved four-word action from the hub's table and writes no old action" |
| Compiler | Compiler/JetRuntime/Source/ProcessSignal.jet | 594:9 | "rt_sigaction borrows the four-word SIG_IGN or SIG_DFL action on the stack and writes no old action" |
| Compiler | Compiler/JetRuntime/Source/ProveJournal.jet | 104:5 | "the harness main arms the journal before any test, so before any other thread reads it" |
| Compiler | Compiler/JetRuntime/Source/ProveJournal.jet | 171:5 | "into lends `capacity` writable bytes for a raw Linux read of stdin" |
| Compiler | Compiler/JetRuntime/Source/ProveJournal.jet | 187:5 | "cell lends 24 writable bytes to TIOCGPGRP and getpgrp takes none" |
| Compiler | Compiler/JetRuntime/Source/ProveJournal.jet | 197:9 | "cell lends the new and old signal masks and the group id to rt_sigprocmask and TIOCSPGRP" |
| Compiler | Compiler/JetRuntime/Source/ProveJournal.jet | 205:9 | "cell lends the group id to TIOCSPGRP and the old mask to rt_sigprocmask" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 11:5 | "address borrows an initialized aligned word in a live native layout the caller holds" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 17:5 | "address is an aligned word of native storage the caller owns or holds the lock for" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 23:5 | "address borrows an initialized byte inside the caller's live buffer" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 29:5 | "address is one exclusively owned writable byte and value is a byte" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 47:5 | "native lowering owns both live ranges of count bytes; each leaf loads all its bytes before it stores any, and the steps run in the order the overlap requires" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 94:5 | "native lowering owns total writable bytes at target and period readable bytes at source, outside the target bytes" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 144:5 | "native lowering owns count writable bytes at target; each leaf stores only inside its class's count" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 202:5 | "the caller (C, or the List equality's lowering) holds count readable bytes at left and at right" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 226:5 | "checked native value-returning callback; its raw argument stays valid for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 232:5 | "checked native value-returning callback; both raw arguments stay valid for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 238:5 | "checked unary closure adapter with borrowed environment, input and output slots" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 244:5 | "checked binary closure adapter with live environment, inputs and output slots" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 250:5 | "checked native Unit callback; its raw argument stays valid for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 256:5 | "checked Unit clone callback; source remains live and target lends writable output storage" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 262:5 | "checked native entry or runtime export whose System V signature takes five words, all live for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 268:5 | "checked executable System V void(void) address" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 272:5 | "checked native Unit callback; its three raw arguments stay valid for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 280:5 | "checked code(box) address; the box stays live for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/RawWords.jet | 286:5 | "checked code(box, argument) address; the box and argument stay live for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/Reactive.jet | 41:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Reactive.jet | 81:5 | "the observer id is a label nothing reads back; a racing creator can only repeat a label" |
| Compiler | Compiler/JetRuntime/Source/ReceiptSections.jet | 245:5 | "the terminated path and the 128-byte stat fact row are live for the syscall" |
| Compiler | Compiler/JetRuntime/Source/RecordReplay.jet | 176:5 | "the observation start runs on the main thread before any task" |
| Compiler | Compiler/JetRuntime/Source/RecordReplay.jet | 211:5 | "the observation start runs on the main thread before any task" |
| Compiler | Compiler/JetRuntime/Source/RecordReplay.jet | 255:5 | "the observation start runs on the main thread before any task" |
| Compiler | Compiler/JetRuntime/Source/RouteAEncoding.jet | 99:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteAEncoding.jet | 369:5 | "the terminated raw path and the 128-byte stat fact row are live for the syscall" |
| Compiler | Compiler/JetRuntime/Source/RouteAFiles.jet | 36:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteAFiles.jet | 523:5 | "the audited caller supplies the Linux syscall's valid word arguments and live buffers" |
| Compiler | Compiler/JetRuntime/Source/RouteAFiles.jet | 529:5 | "the audited caller supplies the Linux syscall's valid word arguments and live buffers" |
| Compiler | Compiler/JetRuntime/Source/RouteALog.jet | 90:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteALog.jet | 124:5 | "the log start runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteALog.jet | 131:5 | "the entry function configures before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteAMath.jet | 14:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/RouteSys.jet | 114:5 | "close(2) releases a descriptor this route opened" |
| Compiler | Compiler/JetRuntime/Source/RouteSys.jet | 124:9 | "read(2) fills at most the caller's remaining owned bytes" |
| Compiler | Compiler/JetRuntime/Source/RouteSys.jet | 146:9 | "socket(AF_UNIX, SOCK_STREAM&#124;SOCK_CLOEXEC) retains no memory" |
| Compiler | Compiler/JetRuntime/Source/RouteSys.jet | 162:9 | "connect(2) reads the owned 23-byte address" |
| Compiler | Compiler/JetRuntime/Source/RouteSys.jet | 178:9 | "sendto(2) with MSG_NOSIGNAL reads the owned request" |
| Compiler | Compiler/JetRuntime/Source/RouteSys.jet | 620:5 | "utimensat borrows the NUL-terminated path and the two owned timespecs for the synchronous call" |
| Compiler | Compiler/JetRuntime/Source/RouteTerm.jet | 333:13 | "written under the host lock" |
| Compiler | Compiler/JetRuntime/Source/RouteTerm.jet | 342:5 | "TCGETS writes the kernel termios into the caller's writable 64-byte struct" |
| Compiler | Compiler/JetRuntime/Source/RouteTerm.jet | 352:5 | "TCSETS reads the kernel termios from the caller's initialized struct" |
| Compiler | Compiler/JetRuntime/Source/RouteTesting.jet | 131:13 | "written under the host lock" |
| Compiler | Compiler/JetRuntime/Source/RouteTesting.jet | 140:9 | "written under the host lock" |
| Compiler | Compiler/JetRuntime/Source/SHA256.jet | 255:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/SHA256.jet | 283:9 | "the state list holds eight U32 words, and the range check keeps each block inside the length borrowed bytes" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 54:5 | "idempotent flags: every writer stores the same 1, the held one first" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 63:5 | "an idempotent flag: every writer stores the same 1" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 77:5 | "the monitor is the only thread that clears the flag; an arm it races restores it" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 253:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 378:5 | "the cold OS leaf borrows its initialized stack word; a successful nonzero base is this attached thread's live Jet TCB or the detached zero word" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 454:5 | "written under the scheduler root mutex" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 463:9 | "written under the scheduler root mutex" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 477:9 | "written under the scheduler root mutex" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 891:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 1048:9 | "an idempotent install: every writer stores the same code address" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 1365:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 1705:5 | "clock_gettime writes two words of thread-owned TCB scratch; no pointer escapes" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 1757:5 | "openat, read and close the path and fill only the caller's own scratch; the descriptor is closed before return" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 2058:9 | "x86 pause only hints that this thread is waiting in a spin loop" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 2417:9 | "sched_yield retains no memory and yields this OS thread" |
| Compiler | Compiler/JetRuntime/Source/Scheduler.jet | 2480:9 | "sched_yield retains no memory and yields this OS thread" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 13:5 | "address names bytes bytes of live storage the caller owns or is freeing; effectful asm stores wipe every byte before release or reuse" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 41:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 164:17 | "secret is a registered live Secret header (Close unregisters before it clears); its buffer holds capacity bytes" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 205:17 | "secret is a registered live Secret header; its length and data words are read" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 274:17 | "secret is a registered live Secret header; its capacity and data words are read" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 300:5 | "start..start+length covers the Secret buffer's mapped pages; madvise and mlock change only their dump and residency attributes" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 318:5 | "bytes is the moved native [U8] header; secret is fresh aligned24B storage and takes over its buffer reference without copying secret material" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 342:5 | "secret is fresh aligned24B storage this call owns exclusively" |
| Compiler | Compiler/JetRuntime/Source/Secret.jet | 359:5 | "secret is a live exclusively borrowed native24B Secret header; len/cap/data follow the native List layout and all byte owners remain inside its buffer" |
| Compiler | Compiler/JetRuntime/Source/Sentry.jet | 18:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Shared.jet | 69:5 | "x86 pause only hints that this thread is waiting in a spin loop; no memory is touched" |
| Compiler | Compiler/JetRuntime/Source/Shared.jet | 571:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/SharedTransactions.jet | 20:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/SharedTransactions.jet | 24:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/StackSegment.jet | 115:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/StackSegment.jet | 210:9 | "target..frontier lies inside the segment, this thread's own mapping above its guard page; populating only backs its pages" |
| Compiler | Compiler/JetRuntime/Source/StackSegment.jet | 283:5 | "a fresh private anonymous mapping, owned by this thread's segment chain; its first page becomes the guard" |
| Compiler | Compiler/JetRuntime/Source/StackSegment.jet | 301:5 | "no frame runs on a returned segment and nothing else holds its address" |
| Compiler | Compiler/JetRuntime/Source/StackSegment.jet | 322:1 | "frame is a live [saved rbp][return address] pair of the calling thread's frame-pointer chain" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 12:5 | "borrows an aligned live synchronization word" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 16:5 | "publishes an aligned live synchronization word with x86 release ordering" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 22:5 | "atomic exchange on an aligned live synchronization word" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 28:5 | "atomic fetch-add on an aligned live synchronization word" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 34:5 | "atomic compare-exchange on an aligned live synchronization word" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 65:5 | "futex wake borrows a live aligned word and retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 88:5 | "futex borrows live state; timed wait uses this attached thread's reserved TCB scratch until the syscall returns" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 104:5 | "futex borrows a live aligned word and retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 207:5 | "base points into the kernel's vDSO image, mapped readable for the process lifetime" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 231:9 | "an idempotent install: every writer stores the same resolved address" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 237:9 | "the vDSO's clock_gettime(clockid_t, struct timespec *) writes the caller's two timespec words" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 245:9 | "clock_gettime writes the caller's two timespec words" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 253:5 | "clock_gettime writes two words of thread-owned TCB scratch; no pointer escapes" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 262:5 | "clock_gettime writes two words of thread-owned TCB scratch; no pointer escapes" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 306:9 | "x86 pause only hints that this thread is waiting in a spin loop" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 350:5 | "sync_sole_inflight is this sole thread's own count until sync_sole is cleared; sync_sole_end reads it only after a membarrier" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 367:5 | "an elided section: only this thread has written the lock word and the count since sync_sole_enter" |
| Compiler | Compiler/JetRuntime/Source/Sync.jet | 381:5 | "membarrier and sched_yield retain no memory; sync_sole only ever goes from 1 to 0" |
| Compiler | Compiler/JetRuntime/Source/Task.jet | 774:9 | "written once, zeroed first, under the TLS registry mutex" |
| Compiler | Compiler/JetRuntime/Source/TerminalColor.jet | 8:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/TerminalInput.jet | 26:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/TerminalProcessRoutes.jet | 58:9 | "TIOCSWINSZ borrows one initialized stack winsize word; the Child104 lock prevents close/recycling until ioctl returns" |
| Compiler | Compiler/JetRuntime/Source/Test.jet | 134:9 | "the OS leaf borrows at most eight initialized bytes from its own stack word" |
| Compiler | Compiler/JetRuntime/Source/TestCapture.jet | 34:9 | "written under the TLS registry mutex" |
| Compiler | Compiler/JetRuntime/Source/TestContracts.jet | 30:5 | "the harness main opens contracts before any test, so before any other thread observes one" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 37:5 | "startup is single-threaded and precedes user code" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 53:5 | "the startup entry is a live NUL-terminated path for open" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 66:5 | "an idempotent install: every racing first use opens the same log" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 111:13 | "buffer lends 4096 writable bytes after the kind byte to getcwd" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 118:13 | "link is a live NUL-terminated /proc path and buffer lends 4096 writable bytes after the kind byte" |
| Compiler | Compiler/JetRuntime/Source/TestReads.jet | 178:5 | "data lends `length` initialized bytes to one write" |
| Compiler | Compiler/JetRuntime/Source/TestSuite.jet | 27:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/TestSuite.jet | 84:5 | "`test: fn` calls `t.run()` on the harness's main thread, before any test task exists" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 119:5 | "every caller has attached before running Jet TLS code; an unattached thread's GS names the readable zero word, which the allocator alone treats as no TCB" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 131:5 | "GS is reserved for Jet and base is a live self-addressed TCB or the detached zero word" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 138:5 | "gettid retains no memory; every caller runs after thread_set_base, so a counted call reads a live GS" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 192:5 | "getpid retains no memory" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 260:9 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 356:9 | "the registry owns this live closure header whose code accepts header and one I64 value" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 462:5 | "rt_sigprocmask reads the spawner's mask from the live start record and retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 484:5 | "the caller transferred a System V I64-&gt;I64 code address and one argument to this thread" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 501:5 | "getrlimit writes its two-word record into the owned 16-byte scratch and retains no pointer" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 595:5 | "a fresh private anonymous mapping, owned by the new thread's handle; its first page becomes the guard and madvise only changes its page size" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 650:5 | "the child inherits a fully blocked mask and starts on its own guarded stack seeded with jet_rt_thread_entry and the start record; the handle's tid word lives until join or reap" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 690:5 | "the startup hands these over once, before the init arrays and any other thread" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 700:5 | "the link's thread functions take words: the handle's live thread id word, jet_rt_thread_entry, its start record and a stack size, or a thread id" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 733:5 | "rt_sigprocmask reads and writes the owned masks record; create writes the thread id into the live handle and hands jet_rt_thread_entry the start record, which the thread frees" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 968:5 | "a credential system call takes word arguments; setgroups' list is the synccall caller's, live until every thread has run" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 1005:9 | "kill(getpid(), SIGKILL) retains no memory" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 1023:5 | "getpid retains no memory" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 1048:13 | "tgkill names a live registered thread of this process" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 1086:9 | "a credential system call takes word arguments and retains no memory" |
| Compiler | Compiler/JetRuntime/Source/Thread.jet | 1098:13 | "written under the registry mutex, before any handler can run" |
| Compiler | Compiler/JetRuntime/Source/Trap.jet | 21:5 | "audited runtime syscall: write borrows live text, exit_group ends the process; getcwd, mkdir, open and close name caller-owned scratch or a descriptor" |
| Compiler | Compiler/JetRuntime/Source/Trap.jet | 118:9 | "the OS leaf borrows a stack word; generated calls have 1..8 valid bytes" |
| Compiler | Compiler/JetRuntime/Source/UIBackends.jet | 76:5 | "a web page's module runs on one thread" |
| Compiler | Compiler/JetRuntime/Source/UIBackends.jet | 86:5 | "only an attached web build records, on its one thread" |
| Compiler | Compiler/JetRuntime/Source/UIBackends.jet | 96:5 | "a web page's module runs on one thread" |
| Compiler | Compiler/JetRuntime/Source/UIBackends.jet | 145:9 | "written under the host lock" |
| Compiler | Compiler/JetRuntime/Source/UIBackends.jet | 158:9 | "written under the host lock" |
| Compiler | Compiler/JetRuntime/Source/UIBackends.jet | 194:5 | "a web page's module runs on one thread" |
| Compiler | Compiler/JetRuntime/Source/UIBackends.jet | 207:5 | "a web page's module runs on one thread" |
| Compiler | Compiler/JetRuntime/Source/VaultTestHooks.jet | 29:5 | "written under the TLS registry mutex" |
| Compiler | Compiler/JetRuntime/Source/VaultTestHooks.jet | 34:5 | "written under the TLS registry mutex" |
| Compiler | Compiler/JetRuntime/Source/VaultTestHooks.jet | 219:5 | "written under the vault test backup mutex" |
| Compiler | Compiler/JetRuntime/Source/World.jet | 647:5 | "the entry function sets the device store before any other thread exists" |
| Compiler | Compiler/JetRuntime/Source/World.jet | 719:5 | "device_mutex is held" |
| Compiler | Compiler/JetRuntime/Source/WsRoutes.jet | 613:5 | "runtime-init runs before any other thread exists" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 17:5 | "the witnesses run one at a time on the main thread" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 24:5 | "a failed allocator witness writes three process-static texts to stderr with raw writes, which take no heap, and terminates this isolated test process" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 45:5 | "both allocations own count bytes; touching their endpoints must not alias each other or their metadata" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 105:5 | "first's private header names its live slab, whose arena word is fixed" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 123:5 | "the flushed cache left the slab linked at its class head; its live count and the class cell are stable while this thread is the heap's only user" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 130:9 | "pointer's private header identifies its still-live slab, which must be the same mapping" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 135:9 | "no other thread touches the slab or its class list" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 148:5 | "first's private header holds its mapping base and length" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 230:9 | "the test owns the full payload and can read its allocator header; the slab's live count and this thread's cache row hold still while it is the heap's only user" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 251:9 | "the chain word remains readable until its payload is freed" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 258:5 | "after all payloads are freed, the class keeps its emptied slabs linked up to ALLOC_RETAINED_EMPTY_SLABS and counts each; any further empty slab went onto the free-slab stack; a first slab the host's blocks share never empties, so it stays linked" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 289:9 | "the test owns the full payload and keeps its chain word there" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 297:9 | "the chain word remains readable until its payload is freed" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 304:5 | "this thread is the heap's only user; the stack word is read once and the stacked slab's link word stays mapped" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 312:5 | "the retired slab is the stack's top again; its pages were never unmapped" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 339:5 | "the test owns this fresh align-8 block and its count word" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 368:5 | "the test owns this fresh align-8 block of size bytes" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 377:5 | "the test holds both owners of this block and its count word" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 383:5 | "the test still owns this block" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 389:5 | "a cached small block stays mapped; its payload bytes are not cache links" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 404:5 | "the test owns these fresh align-8 blocks and their count words" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 408:5 | "the test owns these fresh align-8 blocks and their count words" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 437:9 | "the test owns the payload and keeps its chain word there" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 446:5 | "row is this thread's own cache row" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 450:9 | "the chain word remains readable until its payload is freed" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 456:9 | "row is this thread's own cache row" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 463:5 | "after the flush the row is empty" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 473:5 | "row is this thread's own cache row" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 482:5 | "the test owns this fresh align-8 block and its header words" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 545:9 | "the region owns its 64 KiB until its release below" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 559:5 | "the region owns its 64 KiB until its release below" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 566:5 | "the region owns its two units until its release below" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 573:5 | "the run's pages were dropped at its release, and it stays writable for this region" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 587:5 | "the region owns its two units until its release below" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 622:5 | "runs holds count words this test owns, and the window's link words stay mapped" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet | 722:5 | "the exact success line is two bounded stack words written to stdout" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/CollectionsStopTests.jet | 22:5 | "the single-threaded fixture host owns the snapshot from a case to its check" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/CollectionsStopTests.jet | 37:5 | "the single-threaded fixture host owns the snapshot from a case to its check" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/EnvOverlayKernel.jet | 8:5 | "failed witness terminates its isolated real-runtime process" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/PerfRingTests.jet | 108:5 | "the test runs on one thread and arms no sampler, so no handler reads these" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/PerfRingTests.jet | 197:5 | "the test runs on one thread and arms no sampler, so no handler reads these" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet | 7:5 | "an isolated failed scheduler witness exits with no further runtime cleanup" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 11:5 | "a failed isolated Secret witness terminates this test process" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 21:5 | "header is fresh aligned24B storage owned by the test; data owns cap times the host U8 stride bytes or is zero for an empty List" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 33:5 | "the pointer is a live small allocation; this isolated test is the sole allocator user and can inspect its slab's class metadata" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 49:5 | "input and secret are distinct live headers; adoption leaves one live buffer owner and borrowed bytes alias only that parent's header" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 70:5 | "the test owns data and checks the initialized U8 slots, including epoch padding, before any release" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 81:5 | "secret still owns its live24B header after Close; the buffer was released, and no pointer into it is used" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 93:5 | "each block is a fresh live allocation this witness owns exclusively and chains through its first word; reused is the newly allocated buffer, not a read of freed storage" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 142:5 | "data is this test's live List allocation; every capacity byte is dirtied before the wipe" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 150:5 | "the same live allocation remains exclusively owned while this test checks all wiped bytes" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 170:5 | "data is this test's fresh live allocation of bytes bytes" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 182:5 | "secret is a live Secret header over its own buffer; data stays live through the retained reference" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 196:5 | "data is still live: the other holder's reference was never released" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 207:5 | "frozen is a live Secret header over its own buffer" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 256:5 | "both buffers are this test's fresh live allocations of bytes bytes" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 273:5 | "the single-threaded fixture host owns the kept Secret from the entry to its ended check" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 296:5 | "kept is the live Secret header the entry registered and kept_data its live buffer; the process end only zeroed the buffer" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/SecretTests.jet | 309:5 | "this isolated native witness writes its17B success literal without introducing any text or collection runtime dependency" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/TextTests.jet | 6:5 | "a failed text runtime witness terminates only its isolated process" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 8:5 | "a failed isolated runtime witness ends the process without allocation or libc" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 37:5 | "gettid verifies identity without trusting the TCB under test" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 47:5 | "ARCH_GET_GS writes a caller-owned word; the inherited pointer is compared, never dereferenced" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 120:5 | "rt_sigprocmask writes this thread's mask to a caller-owned word" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 207:13 | "sched_yield widens both sides of the check-to-wait race without timing assumptions" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 245:5 | "rt_sigprocmask writes this thread's mask to a caller-owned word" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet | 379:5 | "heap-free fixture success line is one eight-byte write" |
| Compiler | Compiler/JetRuntime/Tests/Fragments.fixture/UnicodeTests.jet | 8:5 | "a failed Unicode runtime witness ends only its isolated test process" |
| Compiler | Compiler/JetSema/Tests/ScalarGlobalFixtures.jet | 33:5 | "single-threaded test: plain module writes of each scalar type" |
| Compiler | Compiler/JetSema/Tests/XMLCheckerRootsFixtures.jet | 13:5 | "no raw operations in this regression" |
| Compiler | Compiler/JetSema/Tests/XMLCheckerRootsFixtures.jet | 14:9 | "nested tail retains its value grammar" |
| Compiler | Compiler/JetTestKit/Assets/no_nix.jet | 57:9 | "the no-nix launcher passes its subject's status 101 through unchanged; process.exit would report it as 1 (S36)" |
| Compiler | Compiler/JetTestKit/Source/Exec.jet | 621:13 | "the testkit signals the child of its own step child, as the row's `@signal_child` asks" |
| Compiler | Compiler/JetTestKit/Source/Exec.jet | 715:5 | "the testkit stops its own step child at a resource guard" |
| Compiler | Compiler/JetTestKit/Source/Exec.jet | 975:9 | "the testkit stops a process its own case left running in the case scratch" |
| Compiler | Compiler/JetTestKit/Source/StepsControl.jet | 193:5 | "kill(pid, 0) observes the case-owned child without sending a signal" |
| Compiler | Compiler/JetTestKit/Source/StepsSessions.jet | 370:5 | "the case explicitly authorizes signaling its own spawned child" |
| Compiler | Compiler/JetTestKit/Tests/Fixtures/Golden/compiler.jet | 32:9 | "the CLI double stands in for jet, whose internal compiler error alone ends with status 101 (I2); process.exit would report it as 1 (S36)" |
| Core | Core/Auth/Auth.jet | 228:5 | "auth PASETO v4.public verification uses the fixed Ed25519 primitive" |
| Core | Core/Crypto/Crypto.jet | 273:5 | "vault key import: validate a serialized wrapped carrier at the typed crypto boundary" |
| Core | Core/Crypto/Crypto.jet | 280:5 | "vault key import: expose the authenticated wrapped carrier, not plaintext key material" |
| Core | Core/Crypto/Crypto.jet | 286:5 | "vault key import: construct the typed recipient unlock request" |
| Core | Core/Crypto/Crypto.jet | 292:5 | "vault key import: construct the typed passphrase unlock request" |
| Core | Core/Crypto/Crypto.jet | 818:5 | "crypto expert: Argon2id password hashing" |
| Core | Core/Crypto/Crypto.jet | 845:5 | "crypto expert: Argon2id password verification" |
| Core | Core/Crypto/Crypto.jet | 1146:5 | "crypto expert: the authenticated envelope owns the one-time Poly1305 key and MAC framing" |
| Core | Core/Crypto/Crypto.jet | 1244:5 | "crypto expert: typed key agreement validates key widths and rejects all-zero shared secrets" |
| Core | Core/Crypto/Crypto.jet | 1253:5 | "crypto expert: typed signing keys validate the Ed25519 seed width" |
| Core | Core/Crypto/Crypto.jet | 1259:5 | "crypto expert: typed verification validates public key and signature widths before strict verification" |
| Core | Core/Crypto/Expert.jet | 289:1 | "crypto expert: the caller vouches for the password it hands the hash; its bytes stay in this call" |
| Core | Core/Crypto/Expert.jet | 331:1 | "crypto expert: the caller takes custody of the secret's raw bytes and wipes them" |
| Core | Core/Crypto/Expert.jet | 334:1 | "crypto expert: the caller takes custody of the shared secret's raw bytes and wipes them" |
| Core | Core/Crypto/Expert.jet | 337:1 | "crypto expert: the caller takes custody of the signing seed's raw bytes and wipes them" |
| Core | Core/Crypto/Expert.jet | 340:1 | "crypto expert: the caller takes custody of the X25519 private key's raw bytes and wipes them" |
| Core | Core/Crypto/Jetc.jet | 120:5 | "Core JETC2 applies the fixed XChaCha20-Poly1305 suite and protocol-owned nonce/AAD domains" |
| Core | Core/Crypto/Jetc.jet | 129:5 | "Core JETC2 authenticates a bounded record before releasing its plaintext" |
| Core | Core/Crypto/Jetc.jet | 137:5 | "Core JETC2 borrows shared material through expert, wiping its copy before propagating HKDF success or failure" |
| Core | Core/Crypto/Jetc.jet | 153:5 | "Core JETC2 exposes only an operation-scoped key copy and wipes it before either seal outcome escapes" |
| Core | Core/Crypto/Jetc.jet | 169:5 | "Core JETC2 exposes only an operation-scoped key copy and wipes it before either authentication outcome escapes" |
| Core | Core/Crypto/Jetc.jet | 185:5 | "Core JETC2 copies its derived wrap nonce and file key only for sealing, wiping both copies on either outcome" |
| Core | Core/Crypto/Jetc.jet | 204:5 | "Core JETC2 copies its derived wrap nonce only for authentication, wiping the copy on either outcome" |
| Core | Core/Crypto/Jetc.jet | 267:5 | "Core JETC2 checks secret draw widths through the audited exposure and wipes the temporary copy" |
| Core | Core/Crypto/Jetc.jet | 312:5 | "Core JETC2 copies a validated ephemeral Secret into a zeroizing X25519SecretKey; both owners retain their Close duty" |
| Core | Core/Crypto/Vault.jet | 233:1 | "vault key import: the caller's unlock request borrows the private key's bytes for the unlock only" |
| Core | Core/Crypto/Vault.jet | 238:1 | "vault key import: the caller's unlock request borrows the passphrase's bytes for the unlock only" |
| Core | Core/Crypto/Vault.jet | 253:5 | "vault typed load provider boundary" |
| Core | Core/Crypto/Vault.jet | 258:5 | "vault typed current provider boundary" |
| Core | Core/Crypto/Vault.jet | 262:5 | "vault typed status provider boundary" |
| Core | Core/Crypto/Vault.jet | 267:5 | "vault typed versions provider boundary" |
| Core | Core/Crypto/Vault.jet | 272:5 | "vault typed generation provider boundary" |
| Core | Core/Crypto/Vault.jet | 277:5 | "vault typed store provider boundary" |
| Core | Core/Crypto/Vault.jet | 283:5 | "vault key import: signing key material" |
| Core | Core/Crypto/Vault.jet | 291:5 | "vault key import: X25519 secret material" |
| Core | Core/Crypto/Vault.jet | 301:5 | "vault wrapped import provider boundary" |
| Core | Core/Crypto/Vault.jet | 308:5 | "vault typed rotation provider boundary" |
| Core | Core/Crypto/Vault.jet | 313:5 | "vault typed retirement provider boundary" |
| Core | Core/Crypto/Vault.jet | 320:5 | "vault typed revocation provider boundary" |
| Core | Core/Crypto/Vault.jet | 327:5 | "vault write authorization provider boundary" |
| Core | Core/Crypto/Vault.jet | 334:5 | "vault wrapped authorization provider boundary" |
| Core | Core/Crypto/Vault.jet | 340:5 | "vault typed generation commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 346:5 | "vault typed store commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 352:5 | "vault key import commit: signing key material" |
| Core | Core/Crypto/Vault.jet | 358:5 | "vault key import commit: X25519 secret material" |
| Core | Core/Crypto/Vault.jet | 364:5 | "vault wrapped import commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 370:5 | "vault typed rotation commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 376:5 | "vault typed retirement commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 382:5 | "vault typed revocation commit provider boundary" |
| Core | Core/Crypto/Vault.jet | 388:5 | "vault passphrase export provider boundary" |
| Core | Core/Crypto/Vault.jet | 397:5 | "vault recipient export provider boundary" |
| Core | Core/DB/Query.jet | 172:5 | "the exact owned query bytes passed the shared strict UTF-8 validator" |
| Core | Core/Data/Data.jet | 3211:5 | "inspect existing private Decode carriers only inside checked Core data source" |
| Core | Core/Data/Data.jet | 3595:5 | "inspect existing private Decode carriers only inside checked Core data source" |
| Core | Core/Encoding/CBOR.jet | 110:5 | "inspect private typed carriers without changing public Dyn variants" |
| Core | Core/Encoding/CBOR.jet | 178:5 | "validated private carriers retain byte strings for typed Encode" |
| Core | Core/Encoding/CBORStream.jet | 248:9 | "the retained key copies exactly the already validated text bytes" |
| Core | Core/Encoding/CBORStream.jet | 357:17 | "the exact CBOR text allocation passed strict UTF-8 validation" |
| Core | Core/Encoding/CSV.jet | 1099:9 | "each completed UTF-8 scalar was validated before its original bytes entered this field" |
| Core | Core/Encoding/CSV.jet | 1263:25 | "every byte of the field is below 128, so it is whole UTF-8" |
| Core | Core/Encoding/Eetf.jet | 617:9 | "inspect private typed carriers without changing public Dyn variants" |
| Core | Core/Encoding/JSON.jet | 1323:13 | "preserve the native compiler-private Dyn carriers in Core JSON event projection" |
| Core | Core/Encoding/JSON.jet | 1328:13 | "preserve the native compiler-private Dyn carriers in Core JSON event projection" |
| Core | Core/Encoding/Stream.jet | 24:5 | "the shared parser validated this exact numeric lexeme" |
| Core | Core/Encoding/Stream.jet | 462:9 | "the shared UTF-8 validator just accepted this owned byte allocation" |
| Core | Core/Encoding/Stream.jet | 617:9 | "the shared UTF-8 validator just accepted this owned ASCII token" |
| Core | Core/Encoding/Stream.jet | 622:13 | "retain a validated number lexeme in the private event carrier until typed decoding" |
| Core | Core/Encoding/Stream.jet | 854:9 | "transfer already validated UTF-8 storage to inspect its actual owned capacity" |
| Core | Core/Encoding/Stream.jet | 864:9 | "the private numeric event is projected only into the matching typed tree carrier" |
| Core | Core/Encoding/Stream.jet | 1899:5 | "the shared UTF-8 validator accepted this exact owned formatter buffer" |
| Core | Core/Encoding/Stream.jet | 1914:13 | "every byte before the closing quote is printable ASCII, so it is whole UTF-8" |
| Core | Core/Encoding/Stream.jet | 2078:13 | "retain a validated number lexeme in the private event carrier" |
| Core | Core/Encoding/Stream.jet | 2083:13 | "project the native compiler-private Bytes carrier into its event" |
| Core | Core/Encoding/Stream.jet | 2149:13 | "classify the private Bytes carrier without borrowing or exposing its payload" |
| Core | Core/Encoding/Stream.jet | 2303:5 | "owned validated UTF-8 allocation transfers in the encoding source seam" |
| Core | Core/Encoding/Stream.jet | 2339:5 | "exercise the typed Encode byte carrier at the JSON preflight boundary" |
| Core | Core/Encoding/XMLEngine.jet | 279:5 | "the caller selected a scalar-aligned window of validated UTF-8 bytes" |
| Core | Core/Encoding/XMLEngine.jet | 3432:5 | "attribute whitespace compaction preserves the already validated UTF-8 byte sequence" |
| Core | Core/Encoding/YAML.jet | 556:5 | "inspect compiler-private Bytes and typed carriers; public Dyn variants are unchanged" |
| Core | Core/Files/Data.jet | 275:9 | "native path bytes originate from the host environment and are split at ASCII boundaries" |
| Core | Core/Files/Files.jet | 647:1 | "compiler store owns the explicit child descriptor slot and retains the held Directory until the process leaf duplicates its descriptor" |
| Core | Core/Files/Files.jet | 740:9 | "this test owns both held directories and child descriptor slots 198 and 199 until the child runs" |
| Core | Core/Files/Sealed.jet | 161:5 | "the expert API explicitly authorizes authenticated legacy-v1 migration" |
| Core | Core/HTTP/Client.jet | 704:5 | "checked resolver octets and port transferred into the socket carrier" |
| Core | Core/Jobs/Jobs.jet | 136:5 | "queue preflight reads final-component metadata without following a symlink; metadata failures are ignored exactly as the SQLite provider does" |
| Core | Core/Jobs/Jobs.jet | 143:5 | "create the lexical parent with the existing raw recursive mkdir kernel before SQLite open" |
| Core | Core/Jobs/Jobs.jet | 283:13 | "match the queue's existing raw wall-clock wait without advancing controlled scheduler time" |
| Core | Core/Jobs/Jobs.jet | 604:5 | "publish already committed queue observations through the existing validated Devtools event/sink boundary" |
| Core | Core/Jobs/Jobs.jet | 622:5 | "publish source-projected queue status through the existing validated Devtools event/sink boundary" |
| Core | Core/Mem/Mem.jet | 41:1 | "`addr` is the address of a live, aligned `T` that stays valid for every use of the returned pointer" |
| Core | Core/Mem/Mem.jet | 44:1 | "`pointer` names a live, aligned, readable `T` for the whole access" |
| Core | Core/Mem/Mem.jet | 47:1 | "`pointer` names a live, aligned, writable `T` for the whole access" |
| Core | Core/Mem/Mem.jet | 53:1 | "`to` and `from` each name `count` live, aligned `T`s, and a T that owns a resource has one owner after the copy" |
| Core | Core/Mem/Mem.jet | 58:1 | "`to` names `count` live, aligned, writable `T`s that hold no value to drop" |
| Core | Core/Mem/Mem.jet | 67:1 | "`from` names `count` live, aligned `T`s, and a T that owns a resource has one owner after the append" |
| Core | Core/Mem/Mem.jet | 76:1 | "`to` names `count` live, aligned, writable `T`s that hold no value to drop, and a T that owns a resource has one owner after the copy" |
| Core | Core/Mem/Mem.jet | 85:1 | "the storage is used only through pointers derived from the result, and `free_raw` releases it exactly once" |
| Core | Core/Mem/Mem.jet | 90:1 | "`pointer` came unchanged from `alloc_raw`, is not released yet, and no copy of it is used after" |
| Core | Core/Mem/Mem.jet | 101:1 | "`address` starts a mapping of `bytes` that the caller just made and alone owns" |
| Core | Core/Mem/Mem.jet | 104:1 | "`address` and `bytes` name pages of a mapping the caller owns, which it unmaps next, so nothing reads or writes them after" |
| Core | Core/Mem/Mem.jet | 114:1 | "`pointer` names `count` live, aligned Ts that `owner` keeps alive and unchanged by anything else while the View lives" |
| Core | Core/Mem/Mem.jet | 122:1 | "`pointer` names `count` live, aligned Ts that `owner` keeps alive, and `count` is not negative" |
| Core | Core/Mem/Mem.jet | 157:1 | "`pointer` and the T `count` elements past it lie in one live allocation" |
| Core | Core/Mem/Mem.jet | 160:1 | "`pointer` and the address `bytes` past it lie in one live allocation" |
| Core | Core/Mem/Mem.jet | 170:1 | "the pages are fresh and private; the caller unmaps them once, with `__region_release` at the length it reserved" |
| Core | Core/Mem/Mem.jet | 173:1 | "`address` and `bytes` are a mapping `__region_reserve` answered, and nothing reads or writes its pages again" |
| Core | Core/Mem/Mem.jet | 192:9 | "the runtime maps fresh private pages that no other owner can see; the region unmaps them once, at close" |
| Core | Core/Mem/Mem.jet | 201:5 | "`address` names `bytes` of live, writable memory that nothing else reads or writes while the region and its blocks live" |
| Core | Core/Mem/Mem.jet | 235:13 | "the region mapped these pages at reserve, and no block of it is used after close" |
| Core | Core/Mem/Mem.jet | 273:9 | "`next`'s bytes now belong to this block, so its release duty is this block's" |
| Core | Core/Mem/Mem.jet | 302:5 | "the allocator is a place the `#Context(allocator:)` block keeps in place until it ends" |
| Core | Core/Mem/Mem.jet | 307:9 | "the runtime calls this only inside the block, while the allocator at `address` is live, in place and borrowed by nothing else" |
| Core | Core/Mem/Mem.jet | 349:13 | "the block's bytes went back to the allocator that served them" |
| Core | Core/Net/Net.jet | 183:5 | "checked IPv4/IPv6 octets transferred into the native address carrier" |
| Core | Core/Net/Net.jet | 189:5 | "read native address octets; no parsing or formatting policy in the carrier leaf" |
| Core | Core/Net/Net.jet | 197:5 | "read the native IP address discriminant" |
| Core | Core/Net/Net.jet | 260:5 | "validated IP octets, port and IPv6 numeric scope transferred into the socket carrier" |
| Core | Core/Net/Net.jet | 267:5 | "read socket IP representation; formatting stays in Jet" |
| Core | Core/Net/Net.jet | 273:5 | "read native socket port" |
| Core | Core/Net/Net.jet | 287:5 | "read native IPv6 socket scope identifier" |
| Core | Core/Net/Net.jet | 480:5 | "read the native UDP packet's socket-address carrier" |
| Core | Core/Net/SSL/TLS.jet | 3653:1 | "`key` and `iv` are a vector record's, used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3656:5 | "a vector record's key and nonce, used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3662:1 | "`key` and `iv` are a vector record's, used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3665:5 | "a vector record's key and nonce, used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3671:1 | "`key` and `iv` are a vector record's, used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3674:5 | "a vector record's key and nonce, used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3680:1 | "`key` and `iv` are a vector record's, used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3683:5 | "a vector record's key and nonce, used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3690:5 | "each record's key and IV are used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3731:13 | "the record's key and IV are used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3755:13 | "the record's key and nonce are used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3767:1 | "`private` is a vector record's key, used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3770:5 | "a vector record's private key, used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3777:1 | "the arguments are a vector record's public key, message and signature" |
| Core | Core/Net/SSL/TLS.jet | 3780:5 | "verification of a vector record's public signature" |
| Core | Core/Net/SSL/TLS.jet | 3793:13 | "the record's private key is used for that record only" |
| Core | Core/Net/SSL/TLS.jet | 3809:13 | "the record's public key, message and signature" |
| Core | Core/Net/SSL/TLS.jet | 4124:1 | "`seed` is a timing run's test key, used for that run only" |
| Core | Core/Net/SSL/TLS.jet | 4127:5 | "a timing run's test key, used for that run only" |
| Core | Core/Net/SSL/TLS.jet | 4143:5 | "timing runs on test keys and nonces, each used for its run only" |
| Core | Core/Net/SSL/TLS.jet | 4166:1 | "a timing run's test key, used for that run only" |
| Core | Core/Net/SSL/TLS.jet | 4174:1 | "a timing run's test key, used for that run only" |
| Core | Core/Net/SSL/TLS.jet | 4214:5 | "timing runs on test keys, each used for its run only" |
| Core | Core/Net/SSL/TLS.jet | 4625:5 | "test keys and nonces, each used for this run only" |
| Core | Core/Net/SSL/TLS.jet | 4628:1 | "`r` draws test keys and nonces, each used for this run only" |
| Core | Core/Net/SSL/TLS.jet | 4658:5 | "test keys and nonces, each used for this run only" |
| Core | Core/Process/Process.jet | 245:5 | "transfer a fully initialized Jet process plan into the temporary native ProcessSpec carrier; paired environment columns have equal length" |
| Core | Core/Process/Process.jet | 351:1 | "process exit_unmapped: the caller reports a Jet defect and owns the 101 defect status" |
| Core | Core/Service/Service.jet | 1684:5 | "read the final path component's OS metadata kind without opening an identity handle" |
| Core | Core/Sys/Sys.jet | 1020:1 | "posix umask: the caller owns the process file-creation mask and restores it when other code relies on it" |
| Core | Core/Sys/Sys.jet | 1223:1 | "posix set_scheduling: the caller owns the CPU share of the process it changes" |
| Core | Core/Sys/Sys.jet | 1354:1 | "posix set_wall_clock: the caller owns the machine's wall clock, which every process reads" |
| Core | Core/Sys/Sys.jet | 1455:5 | "posix getpgid: inspect a process group" |
| Core | Core/Sys/Sys.jet | 1461:5 | "posix getsid: inspect a process session" |
| Core | Core/Sys/Sys.jet | 1466:1 | "posix getpriority: the caller vouches that `which` names this process or one it controls" |
| Core | Core/Sys/Sys.jet | 1474:1 | "posix setpriority: the caller owns the scheduling priority of the process it changes" |
| Core | Core/Sys/Sys.jet | 1479:1 | "posix utime: the caller owns the file whose timestamps it rewrites" |
| Core | Core/Sys/Sys.jet | 1489:1 | "posix stop: the caller accepts that the process ends here after the shared cleanup boundary" |
| Core | Core/Sys/Sys.jet | 1499:1 | "posix pipe: the caller owns both returned descriptors and closes each exactly once" |
| Core | Core/Sys/Sys.jet | 1506:1 | "posix close_fd: the caller owns `fd` and nothing uses it after this call" |
| Core | Core/Sys/Sys.jet | 1777:1 | "posix mkfifo: the caller owns `path` and removes the named pipe it creates" |
| Core | Core/Sys/Sys.jet | 1797:1 | "posix mknod: the caller owns `path` and grants every process that may open it access to the device it names" |
| Core | Core/Sys/Sys.jet | 1818:1 | "posix fork: the caller handles both the parent and the child return and reaps the child" |
| Core | Core/Sys/Sys.jet | 1821:1 | "posix setsid: the caller owns the process session topology it changes" |
| Core | Core/Sys/Sys.jet | 1824:1 | "posix wait: the caller owns the child processes it reaps" |
| Core | Core/Sys/Sys.jet | 1831:1 | "posix waitpid: the caller owns the child process it reaps" |
| Core | Core/Sys/Sys.jet | 1838:1 | "posix setuid: the caller drops or changes process credentials deliberately" |
| Core | Core/Sys/Sys.jet | 1841:1 | "posix setgid: the caller drops or changes process credentials deliberately" |
| Core | Core/Sys/Sys.jet | 1865:1 | "posix set_user_ids: the caller drops or changes process credentials deliberately" |
| Core | Core/Sys/Sys.jet | 1873:1 | "posix set_group_ids: the caller drops or changes process credentials deliberately" |
| Core | Core/Sys/Sys.jet | 2068:1 | "posix setpgrp: the caller owns the process group topology it changes" |
| Core | Core/Sys/Sys.jet | 2071:1 | "posix setpgid: the caller owns the process group topology it changes" |
| Core | Core/Sys/Sys.jet | 2076:1 | "posix kill: the caller owns or is entitled to signal the target process" |
| Core | Core/Sys/Sys.jet | 2084:1 | "posix signal reset: the caller owns the process signal disposition it resets" |
| Core | Core/Sys/Sys.jet | 2089:1 | "posix initgroups: the caller changes supplementary groups deliberately" |
| Core | Core/TUI/TUI.jet | 108:5 | "Core TUI reads owned encoded OS environment bytes from the current host; no UTF-8 assumption" |
| Core | Core/Tasks/Tasks.jet | 115:1 | "`handle` is a task the runtime submitted that no carrier has taken, and it goes back to the runtime once" |
| Core | Core/Testing/Testing.jet | 1513:5 | "all appended bytes came from complete UTF-8 strings or ASCII" |
| Docs | Docs/Audits/G15202611/Probes/c4599_28_comptime_asm.jet | 10:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Docs | Docs/Audits/G15202611/Probes/c4599_28_comptime_asm.jet | 25:5 | "call the audited register-only assembly contract" |
| Docs | Docs/Audits/G15202611/Probes/c4599_28_comptime_asm_reach.jet | 10:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Docs | Docs/Audits/G15202611/Probes/c4599_28_comptime_asm_reach.jet | 22:9 | "call the audited register-only assembly contract" |
| Docs | Docs/Audits/G15202611/Probes/c4599_28_runtime_asm_reach.jet | 10:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Docs | Docs/Audits/G15202611/Probes/c4599_28_runtime_asm_reach.jet | 22:9 | "call the audited register-only assembly contract" |
| Docs | Docs/Audits/PlaceDefault20261121/Probes/returns.jet | 107:5 | "a raw word stands in for the destination word of the task's context" |
| Docs | Docs/Audits/PlaceDefault20261121/Probes/returns.jet | 116:5 | "a raw word stands in for the destination word of the task's context" |
| Docs | Docs/Audits/PlaceDefault20261121/Probes/returns.jet | 206:17 | "a raw word stands in for the destination word of the task's context" |
| Docs | Docs/Audits/PlaceDefault20261121/Probes/returns.jet | 245:17 | "a raw word stands in for the destination word of the task's context" |
| Docs | Docs/Audits/PlaceDefault20261121/Probes/returns.jet | 284:17 | "a raw word stands in for the destination word of the task's context" |
| Docs | Docs/Proposals/DebugEvalNative20261118/Probes/fx_line.jet | 13:5 | "probe: a raw write under the empty row" |
| Docs | Docs/Proposals/DebugEvalNative20261118/Probes/row_compose.jet | 21:9 | "probe: a raw write under the empty row" |
| Docs | Docs/Proposals/DebugEvalNative20261118/Probes/unsafe_callee.jet | 7:5 | "probe: a raw write in a callee" |
| Docs | Docs/Proposals/JaiStyleSample/Data.jet | 1274:9 | "native path bytes originate from the host environment and are split at ASCII boundaries" |
| Docs | Docs/Proposals/JaiStyleSample/Data.jet | 1886:5 | "inspect existing private Decode carriers only inside checked Core data source" |
| Docs | Docs/Spec/Reference/syntax-surface.jet | 567:1 | "reads through a raw pointer; addr must be live and valid" |
| Docs | Docs/Spec/Reference/syntax-surface.jet | 570:5 | "addr is the address of a live Int on this frame" |
| Examples | Examples/Crypto/CryptoMigration.jet | 35:5 | "AES-256-GCM protocol interoperability" |
| Examples | Examples/Crypto/CryptoSuite.jet | 40:5 | "known-answer check reads the RFC 7748 shared secret" |
| Examples | Examples/Crypto/ExpertProjection.jet | 36:5 | "expert cryptography is an explicit opt-in" |
| Examples | Examples/Crypto/RandomAPISplit.jet | 36:5 | "compare the typed and raw HKDF rungs" |
| Examples | Examples/Crypto/VaultKeys.jet | 44:5 | "restore audited raw signing-key material" |
| Examples | Examples/ErrorsAndEffects/Effects/AuditedGateLadder.jet | 6:5 | "the audited block has no low-level operation" |
| Examples | Examples/ErrorsAndEffects/Effects/SingleUse.jet | 85:5 | "event canceled; the ticket admits to nothing, so voiding it is correct" |
| Examples | Examples/Interop/FFI/InlineC.jet | 7:1 | "the scalar ABI contract matches the C definition" |
| Examples | Examples/Interop/FFI/InlineC.jet | 14:1 | "the scalar ABI contract deliberately returns a mismatched result" |
| Examples | Examples/Interop/FFI/InlineC.jet | 20:5 | "call the audited inline C contract" |
| Examples | Examples/Interop/FFI/InlineC.jet | 25:9 | "call the audited inline C contract with bounded Int values" |
| Examples | Examples/Interop/FFI/InlineC.jet | 31:9 | "call the audited inline C contract with a deliberately wrong C result" |
| Examples | Examples/Interop/FFI/OpaqueHandles/run.jet | 36:5 | "fixed headers define handle close ownership, exclusive buffers, and bounded output lengths" |
| Examples | Examples/LowLevel/InlineAsm.jet | 9:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Examples | Examples/LowLevel/InlineAsm.jet | 20:5 | "call the audited register-only assembly contract" |
| Examples | Examples/LowLevel/LowLevel.jet | 8:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| Examples | Examples/LowLevel/LowLevel.jet | 22:5 | "`cell` is a live Int on this stack frame; the pointer never escapes" |
| Examples | Examples/LowLevel/MMIOBoardWrite.jet | 9:5 | "stand-in MMIO cell stays on this stack frame" |
| Examples | Examples/LowLevel/PointerCastDeref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
| Examples | Examples/LowLevel/RawPointerTools.jet | 7:5 | "the block is used only through pointers derived from it, every index stays inside its four Ints, and it is released once" |
| Examples | Examples/LowLevel/UnsafeObligations.jet | 9:5 | "cell stays live and the pointer remains local" |
| Examples | Examples/Memory/Allocation/ContextAllocator.jet | 49:5 | "the counter's kept blocks were claimed from its region, which unmaps them all" |
| Examples | Examples/Memory/Allocation/ContextAllocatorRead.jet | 56:5 | "the counter's kept blocks were claimed from its region, which unmaps them all" |
| Examples | Examples/Memory/Allocation/Uninit.jet | 12:5 | "uninitialized plain-data storage is filled before every read" |
| Examples | Examples/Memory/Allocation/UserAllocator.jet | 72:5 | "the pool's free blocks were claimed from its region, which unmaps them all" |
| Examples | Examples/Memory/ExpiringSecret.jet | 23:5 | "vault key import: expiring secret lifetime" |
| Examples | Examples/Memory/Pin.jet | 42:5 | "node storage is fixed for the returned pin; self_addr names this place" |
| Examples | Examples/Memory/Unsafe/UnsafeSentries.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| Examples | Examples/Memory/Unsafe/UnsafeSentriesPackageOff/run.jet | 7:5 | "the local cell is live for this package-policy proof" |
| Examples | Examples/Memory/Unsafe/UnsafeSentriesProvenance.jet | 8:5 | "external address must be a live allocation" |
| Examples | Examples/Memory/Unsafe/UnsafeSentriesSourceOff.jet | 7:5 | "the local cell is live for this raw read" |
| Examples | Examples/Systems/Process/OSCPUTime.jet | 40:5 | "posix set_wall_clock: a time before 1970 is refused before it reaches the kernel" |
| Examples | Examples/Systems/Process/OSIDs.jet | 17:5 | "posix set_user_ids: the effective ID is set to the one it already is, and the out-of-range ID never reaches the kernel" |
| Examples | Examples/Systems/Process/OSMknod.jet | 29:5 | "make a device file in the temporary directory and remove it" |
| Examples | Examples/Systems/Process/OSProcessControl.jet | 31:5 | "POSIX process and pipe control for the core.sys surface" |
| Examples | Examples/Systems/Process/OSScheduling.jet | 26:5 | "posix set_scheduling: the out-of-range priority never reaches the kernel" |
| Examples | Examples/Systems/Process/OSScheduling.jet | 38:5 | "posix set_scheduling: this program moves itself to batch time sharing and back" |
| Examples | Examples/Systems/Process/ProcessExitCleanup.jet | 23:5 | "the exit callback has no captured state" |
| Examples | Examples/Systems/Process/ProcessExitUnmapped.jet | 21:13 | "this child reports a Jet defect status on purpose" |
| Examples | Examples/Systems/Process/SystemPrivilegedEntry.jet | 10:5 | "run only inside a disposable user namespace with a dedicated test identity" |
| Examples | Examples/Systems/Process/SystemProjection.jet | 21:5 | "the clock file was created above in this test-owned directory" |
| Examples | Examples/Systems/Process/SystemProjection.jet | 27:5 | "fork and reap both children before returning" |
| Examples | Examples/Text/CheckedText/CheckedTextRuntimeHole.jet | 25:5 | "checked text example output" |
| Examples | Examples/Text/CheckedText/CheckedTextRuntimeHoleStop.jet | 24:5 | "checked text example output" |
| Examples | Examples/Text/CheckedText/CheckedTextType.jet | 20:5 | "checked text example output" |
| Jetpack | Jetpack/Foundation/Source/TreeHash.jet | 602:9 | "the authority retains the held directory until the process provider duplicates its descriptor; the child descriptor slot is supplied by the compiler" |
| Jetpack | Jetpack/Provider/Batteries/embedded/embedded.jet | 183:5 | "host replay uses a stack cell for the CMSDK UART0 control register and never dereferences the target address" |
| Jetpack | Jetpack/Publication/Source/Sign.jet | 75:5 | "export a fresh Core signing seed only to descriptor-relative owner-only key publication; the Core key owner is wiped on every outcome" |
| Jetpack | Jetpack/Publication/Source/Sign.jet | 139:5 | "import the protected seed into Core signing without a second raw-secret copy; close the temporary Core Secret on success and failure" |
| Jetpack | Jetpack/Publication/Source/Sign.jet | 165:5 | "package author signatures require Core's strict Ed25519 verification; no raw verifier or helper process" |
| Jetpack | Jetpack/Publication/Tests/Index.jet | 346:5 | "the test owns the planted lock file it ages" |
| Jetpack | Jetpack/Store/Tests/ClosureGraph.jet | 65:13 | "the test owns the journal file it just wrote" |
| Jetpack | Jetpack/Store/Tests/Seal.jet | 33:5 | "the store test owns the fixture file whose mtime it pins" |
| Tests | Tests/Cases/BackendIce/Fixtures/AsmRail/rail.jet | 10:1 | "the program claims the runtime's stack switch" |
| Tests | Tests/Cases/BackendIce/Fixtures/AsmRail/rail.jet | 36:1 | "the program claims the runtime's TLS base" |
| Tests | Tests/Cases/BackendIce/Fixtures/AsmRail/rail.jet | 44:5 | "call the leaf the build refuses" |
| Tests | Tests/Cases/BackendIce/Fixtures/AsmRail/rail.jet | 48:5 | "call the leaf the build refuses" |
| Tests | Tests/Cases/BackendIce/Fixtures/AsmRdtsc/stamp/stamp.jet | 9:1 | "rdtsc writes only rax and rdx, both declared" |
| Tests | Tests/Cases/BackendIce/Fixtures/AsmRdtsc/stamp/stamp.jet | 20:1 | "lfence and rdtsc write only rax and rdx, and rdx is declared" |
| Tests | Tests/Cases/BackendIce/Fixtures/AsmRdtsc/stamp/stamp.jet | 32:5 | "call the audited rdtsc contracts" |
| Tests | Tests/Cases/BackendIce/Fixtures/AsmRdtsc/unencoded/unencoded.jet | 12:1 | "the body writes only its result register" |
| Tests | Tests/Cases/BackendIce/Fixtures/AsmRdtsc/unencoded/unencoded.jet | 20:1 | "the body writes only its result register" |
| Tests | Tests/Cases/BackendIce/Fixtures/AsmRdtsc/unencoded/unencoded.jet | 28:5 | "call the leaves sema refuses" |
| Tests | Tests/Cases/BuildEntry/MetaCodeNeverAuthorizesUnsafe.fixture/added/main.jet | 8:17 | "generated" |
| Tests | Tests/Cases/BuildEntry/MetaCodeNeverAuthorizesUnsafe.fixture/rewritten/main.jet | 20:25 | "generated" |
| Tests | Tests/Cases/CLIPartsCore/Fixtures/GatePolicy/main.jet | 2:5 | "org refusal" |
| Tests | Tests/Cases/CLIPartsInspect/Fixtures/Foreign/app/main.jet | 4:5 | "calls the audited dependency" |
| Tests | Tests/Cases/CLIPartsInspect/Fixtures/Foreign/dep/dep.jet | 3:1 | "intentionally invalid address" |
| Tests | Tests/Cases/CLIPartsInspect/Fixtures/Release/hardened/main.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| Tests | Tests/Cases/CLIPartsInspect/Fixtures/Release/release/main.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| Tests | Tests/Cases/CLIPartsInspect/Fixtures/Team/unsafe/main.jet | 2:5 | "audit" |
| Tests | Tests/Cases/CLIPartsInspect/Fixtures/TestHardened/hardened/main.jet | 5:5 | "test deliberately reads after arena reset" |
| Tests | Tests/Cases/CLIPartsInspect/Fixtures/TestHardened/release/main.jet | 5:5 | "test deliberately reads after arena reset" |
| Tests | Tests/Cases/Cffi/CffiBorrowedByteViewCrossesForTheCall.fixture/main.jet | 13:5 | "the block holds the four bytes for the whole call, C keeps no pointer to it, and it is released once" |
| Tests | Tests/Cases/Cffi/CffiNullStringReturnReadsAsAbsent.fixture/main.jet | 14:9 | "a non-null result names a static NUL-terminated string C never frees" |
| Tests | Tests/Cases/Cffi/CffiPartialReadReturnsItsStatusAndLength.fixture/main.jet | 14:5 | "both blocks are live for the call, C writes at most `output_cap` bytes and keeps no pointer, and each is released once" |
| Tests | Tests/Cases/Cffi/CffiPointerAndCountCrossTogether.fixture/main.jet | 13:5 | "the block holds the three bytes for the whole call, C keeps no pointer to it, and it is released once" |
| Tests | Tests/Cases/Cffi/CffiRawStatusOutPointerReadsOnlyOnSuccess.fixture/main.jet | 23:5 | "store_load receives a live non-null slot; bytes are read only after status zero" |
| Tests | Tests/Cases/Cffi/CffiStatusAndWrittenCountBothReturn.fixture/main.jet | 14:5 | "both blocks are live for the calls, C writes at most `output_cap` bytes and keeps no pointer, and each is released once" |
| Tests | Tests/Cases/Cffi/FFICapabilityBoundaryIsSafeOnlyInsideUnsafe/Input/o2/run.jet | 8:5 | "the foreign call may write through the borrowed value" |
| Tests | Tests/Cases/Cffi/FFICapabilityBoundaryIsSafeOnlyInsideUnsafe/Input/o3/run.jet | 8:5 | "the foreign call may write through the borrowed value" |
| Tests | Tests/Cases/Cffi/RuntimeExitImportExitsWithItsArgument.fixture/main.jet | 1:1 | "this program owns the exit status it passes" |
| Tests | Tests/Cases/CffiNativeMatrix/Fixtures/RequiredNativeCABIMatrix/default/matrix.jet | 47:5 | "live non-null out slot; read only after status zero" |
| Tests | Tests/Cases/CffiNativeMatrix/Fixtures/RequiredNativeCABIMatrix/sysv64/matrix.jet | 47:5 | "live non-null out slot; read only after status zero" |
| Tests | Tests/Cases/CollectionsAndMethods/SentryTypedIntStoreKeepsExactCarrier.fixture/main.jet | 6:5 | "the typed pointer refers to the live local cell for these stores" |
| Tests | Tests/Cases/Comptime/NativeO0SharedTasksSelect.fixture/ffi.jet | 6:1 | "the scalar ABI contract matches the C definition" |
| Tests | Tests/Cases/Comptime/NativeO0SharedTasksSelect.fixture/ffi.jet | 12:5 | "call the audited inline C contract" |
| Tests | Tests/Cases/ComptimeCLIParity/ComptimeChildIsReapedBeforeTheProgramRuns.fixture/run.jet | 15:5 | "fork and reap the child before returning" |
| Tests | Tests/Cases/ComptimeCLIParity/Fixtures/TierParityExamplesRunThroughAOTJITAndInterpreter/tooling_checked_text_type/checked_text_type.jet | 20:5 | "checked text example output" |
| Tests | Tests/Cases/ComptimeCorePureParity/CryptoExpertAeadSignArgonAOTStdoutMatchesKnownAnswers.fixture/main.jet | 9:5 | "fixed RFC interop vectors" |
| Tests | Tests/Cases/ComptimeCorePureParity/CryptoValuesMatchAOTComptimeForcedInterpreterAndDefaultDev/Input/o1/run.jet | 12:5 | "fixed RFC interop vectors" |
| Tests | Tests/Cases/ComptimeCorePureParity/CryptoValuesMatchAOTComptimeForcedInterpreterAndDefaultDev/Input/o2/run.jet | 12:5 | "fixed RFC interop vectors" |
| Tests | Tests/Cases/ConcurrencyBoundaries/AGroupTaskReadsALentParameterInPlace.fixture/lend.jet | 10:5 | "reads the address of the graph's storage, not its bytes" |
| Tests | Tests/Cases/ConcurrencyBoundaries/AGroupTaskReadsALentParameterInPlace.fixture/lend.jet | 29:5 | "reads the address of the graph's storage, not its bytes" |
| Tests | Tests/Cases/Context/ContextAllocatorMemoEntryMovesToTheHeap.fixture/memo.jet | 63:5 | "the reuser's kept blocks were claimed from its region, which unmaps them all" |
| Tests | Tests/Cases/Context/ContextAllocatorMovesTextThatLeavesTheBlock.fixture/text.jet | 104:5 | "the reuser's kept blocks were claimed from its region, which unmaps them all" |
| Tests | Tests/Cases/Context/ContextAllocatorMovesWhatLeavesTheBlock.fixture/moves.jet | 124:5 | "the reuser's kept blocks were claimed from its region, which unmaps them all" |
| Tests | Tests/Cases/Context/ContextAllocatorPanicInAllocateStopsTheTask.fixture/refuse.jet | 38:5 | "the picky allocator's kept blocks were claimed from its region, which unmaps them all" |
| Tests | Tests/Cases/Context/ContextAllocatorReadsBackTheUserAllocator.fixture/main.jet | 52:5 | "the counter's kept blocks were claimed from its region, which unmaps them all" |
| Tests | Tests/Cases/Context/ContextAllocatorServesTheBlockAndItsLibraryCalls.fixture/served.jet | 61:5 | "the counter's kept blocks were claimed from its region, which unmaps them all" |
| Tests | Tests/Cases/Context/ContextScratchBlockRewindsAndMovesWhatLeaves.fixture/scratch.jet | 18:5 | "only reads the address of an item the caller keeps alive" |
| Tests | Tests/Cases/ControlAndData/ExplicitExitUnmappedKeeps101OnEveryTier.fixture/main.jet | 5:5 | "this case reports a Jet defect status on purpose" |
| Tests | Tests/Cases/ControlAndData/ExplicitProcessExitCleanupGoldenMatchesAllExecutionTiers.fixture/process_exit_cleanup.jet | 22:5 | "the exit callback has no captured state" |
| Tests | Tests/Cases/ControlAndData/ProcessExitLeavesHeapToKernelAfterUserCleanup.fixture/main.jet | 35:5 | "the exit callback has no captured state" |
| Tests | Tests/Cases/CoreCryptoArgon2ID/Argon2IDParallelLanesMatchRfcVectorAcrossTiers/Input/run.jet | 8:5 | "fixed Argon2id parallel-lane known-answer vector" |
| Tests | Tests/Cases/CoreLib/CoreMemRegionsHandOutBlocksAndAWindowChecksItsIndex.fixture/main.jet | 16:5 | "the caller's block holds the bytes and stays unused while the adopted region and its blocks live" |
| Tests | Tests/Cases/CoreLib/CoreMemRegionsHandOutBlocksAndAWindowChecksItsIndex.fixture/main.jet | 43:5 | "the shelf's blocks were claimed from regions that are closed, and nothing reads them again" |
| Tests | Tests/Cases/CoreLib/CoreMemRegionsHandOutBlocksAndAWindowChecksItsIndex.fixture/window.jet | 6:5 | "the four Ints are fresh heap storage, every access stays inside them, and they are released once" |
| Tests | Tests/Cases/CoreLibNet/SharedMemoryWordsOrderAcrossProcesses.fixture/main.jet | 25:5 | "fork a peer that shares the mapping, and reap it" |
| Tests | Tests/Cases/Cross/WebBuildRefusesInlineCBeforeCodegen.fixture/main.jet | 1:1 | "the scalar ABI contract matches the C definition" |
| Tests | Tests/Cases/Cross/WebBuildRefusesInlineCBeforeCodegen.fixture/main.jet | 7:5 | "call the audited inline C contract" |
| Tests | Tests/Cases/Custom/Fixtures/FromAddrSentryIsTierParity/fixture.jet | 5:5 | "value is live on this stack frame and the pointer never escapes" |
| Tests | Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_alignment/unsafe_sentries_alignment.jet | 5:5 | "the tuple storage is live but this raw Int is misaligned" |
| Tests | Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_expired_recursive_stack/unsafe_sentries_expired_recursive_stack.jet | 11:5 | "the recursive address outlives its owning Jet frame" |
| Tests | Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_expired_stack/unsafe_sentries_expired_stack.jet | 10:5 | "the returned address outlives its owning Jet frame" |
| Tests | Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_misaligned_list/unsafe_sentries_misaligned_list.jet | 5:5 | "the list storage is live but this raw Int is misaligned" |
| Tests | Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_provenance/unsafe_sentries_provenance.jet | 6:5 | "external address must be a live allocation" |
| Tests | Tests/Cases/Custom/Fixtures/SentryFaultsAreTierParity/unsafe_sentries_quarantine/unsafe_sentries_quarantine.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| Tests | Tests/Cases/Custom/Fixtures/SentrySourcePolicyOffIsTierParity/fixture.jet | 7:5 | "the local cell is live for this raw read" |
| Tests | Tests/Cases/Debug/AFaultNamesItsRoutineAndTask.fixture/main.jet | 6:5 | "the program copies from an address it never mapped, to fault" |
| Tests | Tests/Cases/DevPartsRoutine/JIT1216AdversarialRegressions.fixture/raw_alias/run.jet | 5:5 | "the pointer stays inside this stack frame" |
| Tests | Tests/Cases/DevPartsRoutine/UnsafeBlocksAreEvaluatedWithLiveSentries.fixture/fault.jet | 4:5 | "mapped address is valid and aligned" |
| Tests | Tests/Cases/DiagnosticsCoverage/SentryMetadataUsesTheReturnedFaultExitCode.fixture/app/main.jet | 4:5 | "calls the audited dependency" |
| Tests | Tests/Cases/DiagnosticsCoverage/SentryMetadataUsesTheReturnedFaultExitCode.fixture/dep/dep.jet | 3:1 | "intentionally invalid address" |
| Tests | Tests/Cases/Effects/ProcessExitNeedsOnlyTheExitLeaf.fixture/exit.jet | 7:1 | "the case checks the leaf of the unmapped exit" |
| Tests | Tests/Cases/Effects/ProcessExitNeedsOnlyTheExitLeaf.fixture/exit.jet | 13:9 | "the case checks the leaf of the unmapped exit" |
| Tests | Tests/Cases/FixturesCryptoBridgeEntropy/Argon2IDRfcAndExpertKnownAnswersKillArgon2IMutation.fixture/run.jet | 44:5 | "original public expert Argon2id KAT" |
| Tests | Tests/Cases/FixturesCryptoBridgeEntropy/ExpertAES256GcmMatchesNistCavsVector.fixture/run.jet | 41:5 | "original published known-answer vector" |
| Tests | Tests/Cases/FixturesCryptoBridgeEntropy/ExpertSurfaceMatchesReferenceVectorsAndBounds.fixture/run.jet | 48:5 | "original expert interoperability vectors" |
| Tests | Tests/Cases/FixturesCryptoBridgeEntropy/ExpertXchacha20Poly1305MatchesCfrgVector.fixture/run.jet | 41:5 | "original published known-answer vector" |
| Tests | Tests/Cases/FixturesCryptoBridgeEntropy/JetcV1ExpertOpenAcceptsOnlyThePinnedGrammar.fixture/run.jet | 47:5 | "original pinned JETC v1 migration grammar" |
| Tests | Tests/Cases/FixturesCryptoBridgeEntropy/JetcV1MigrationResealsToVerifiedV2WithoutTouchingSource.fixture/run.jet | 24:5 | "crypto expert: the fixture seals a pinned JETC v1 envelope and migrates it" |
| Tests | Tests/Cases/FixturesCryptoBridgeEntropy/KeygenZeroizesProviderCopyAndStackSeed.fixture/run.jet | 46:5 | "crypto expert: the test reads the generated seed to prove it is live and 32 bytes" |
| Tests | Tests/Cases/FixturesCryptoBridgeEntropy/PriorCryptoSuiteStillUsesLiveEntropy.fixture/run.jet | 49:5 | "crypto expert: the test reads the generated seed length" |
| Tests | Tests/Cases/FixturesCryptoBridgeEntropy/TypedSecretWrapSignKDFAndPasswordPathsPreserveRoles.fixture/run.jet | 55:5 | "crypto expert: the test derives from the secret's raw bytes" |
| Tests | Tests/Cases/Fmt/FmtPreservesCheckedTextType/Input/run.jet | 19:5 | "formatter fixture" |
| Tests | Tests/Cases/Fmt/FmtPreservesUnsafeSiteModesAndPostfixObligations/Input/run.jet | 3:1 | "caller keeps address live" |
| Tests | Tests/Cases/Fmt/FmtPreservesUnsafeSiteModesAndPostfixObligations/Input/run.jet | 13:5 | "calling audited reader" |
| Tests | Tests/Cases/Fmt/FmtUnsafeReasonsEscapeStrings/Input/run.jet | 3:1 | "caller says \"ok\"" |
| Tests | Tests/Cases/Fmt/FmtUnsafeReasonsEscapeStrings/Input/run.jet | 8:5 | "path C:\\tmp" |
| Tests | Tests/Cases/FmtProject/RawCImportsAndModuleBindingsRemainCleanThroughCLI.fixture/runtime.jet | 7:1 | "audited C boundary" |
| Tests | Tests/Cases/FmtProject/RawCImportsAndModuleBindingsRemainCleanThroughCLI.fixture/runtime.jet | 10:1 | "audited C boundary" |
| Tests | Tests/Cases/FmtRoundTripRegressions/RawCImportsAndModuleBindingMutabilityAreByteExactAndIdempotent/Input/run.jet | 7:1 | "audited C boundary" |
| Tests | Tests/Cases/FmtRoundTripRegressions/RawCImportsAndModuleBindingMutabilityAreByteExactAndIdempotent/Input/run.jet | 10:1 | "audited C boundary" |
| Tests | Tests/Cases/GateLedger/Fixtures/Gates/gates.jet | 2:5 | "test unsafe reason" |
| Tests | Tests/Cases/JetTest/JetTestReportsEachFailingTestAndRunsTheRest.fixture/parked.jet | 35:5 | "the exit callback has no captured state" |
| Tests | Tests/Cases/LanguageFeatures/CoreGroupImportNamesModuleTraitsAndTypes.fixture/main.jet | 46:5 | "the stack's kept block was claimed from its region, which unmaps it" |
| Tests | Tests/Cases/LanguageFeatures/ImplMethodInheritsTraitEffectRowOnEveryTier.fixture/main.jet | 38:5 | "the case prints the checked text" |
| Tests | Tests/Cases/LivenessLints/FallbackErrIsNeverAnUnusedLocal.fixture/main.jet | 3:1 | "probe" |
| Tests | Tests/Cases/LivenessLints/FallbackErrIsNeverAnUnusedLocal.fixture/main.jet | 9:5 | "probe" |
| Tests | Tests/Cases/Memory/SentryPolicyOffSkipsR0801.fixture/checked.jet | 11:5 | "target's buffer holds eight Ints: its three items and spare capacity past them" |
| Tests | Tests/Cases/Memory/SentryPolicyOffSkipsR0801.fixture/off.jet | 11:5 | "target's buffer holds eight Ints: its three items and spare capacity past them" |
| Tests | Tests/Cases/MemoryKernel/AMappingIsLiveStorageUntilItIsUnmapped.fixture/after_close.jet | 19:5 | "the copy writes into pages the region unmapped, so the sentry stops it before any byte moves" |
| Tests | Tests/Cases/MemoryKernel/AMappingIsLiveStorageUntilItIsUnmapped.fixture/after_close.jet | 25:5 | "the shelf's block was claimed from the closed region, and nothing reads it again" |
| Tests | Tests/Cases/MemoryKernel/AMappingIsLiveStorageUntilItIsUnmapped.fixture/keeper.jet | 6:5 | "the program claims pages it never mapped" |
| Tests | Tests/Cases/MemoryKernel/AMappingIsLiveStorageUntilItIsUnmapped.fixture/main.jet | 15:5 | "the block holds three Ints of the open region's fresh pages" |
| Tests | Tests/Cases/MemoryKernel/AMappingIsLiveStorageUntilItIsUnmapped.fixture/main.jet | 23:5 | "the shelf's block was claimed from the closed region, and nothing reads it again" |
| Tests | Tests/Cases/MemoryKernel/ARawBlockCopyAndFillMoveWholeRanges.fixture/copy_from_past.jet | 4:5 | "the copy reads one Int past the source block" |
| Tests | Tests/Cases/MemoryKernel/ARawBlockCopyAndFillMoveWholeRanges.fixture/copy_to_past.jet | 4:5 | "the copy writes one Int past the target block" |
| Tests | Tests/Cases/MemoryKernel/ARawBlockCopyAndFillMoveWholeRanges.fixture/main.jet | 5:5 | "every range stays inside the `values` tuple, which outlives each access" |
| Tests | Tests/Cases/MemoryKernel/ARawBlockCopyAndFillMoveWholeRanges.fixture/main.jet | 16:5 | "both Strings stay inside `names`; the static texts they replace need no drop" |
| Tests | Tests/Cases/MemoryKernel/ARawBlockCopyAndFillMoveWholeRanges.fixture/past.jet | 5:5 | "the fill runs one Int past the tuple, so the sentry stops it before any write" |
| Tests | Tests/Cases/MemoryKernel/ARawByteFillWritesExactlyItsRange.fixture/main.jet | 16:5 | "every fill and read stays inside the block's `room` bytes, and the block is freed once after them" |
| Tests | Tests/Cases/MemoryKernel/ARawByteMoveMatchesABufferedMoveAtEverySizeAndOverlap.fixture/main.jet | 17:5 | "every move and every byte written or read stays inside the block, which is freed once after the moves" |
| Tests | Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/main.jet | 6:5 | "every raw range stays inside the `values` tuple, which outlives each access" |
| Tests | Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/main.jet | 17:5 | "each append reads the Ints `many` holds, which move before their old buffer goes" |
| Tests | Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/main.jet | 27:5 | "both ranges stay inside `names`; every String is a static text, which needs no single owner" |
| Tests | Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/negative.jet | 6:5 | "the two Ints of `cell` take the copy" |
| Tests | Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/outside.jet | 6:5 | "the two Ints of `cell` take the copy" |
| Tests | Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/past_append.jet | 6:5 | "the append reads one Int past the tuple, so the sentry stops it before the List changes" |
| Tests | Tests/Cases/MemoryKernel/ARawListAppendAndCopyMoveWholeRanges.fixture/past_copy.jet | 6:5 | "the copy writes one Int past the tuple, so the sentry stops it before any byte moves" |
| Tests | Tests/Cases/MemoryKernel/ARawPointerCastReadsTheSameAddress.fixture/main.jet | 6:5 | "each cast names the live local cell, at an alignment its type allows" |
| Tests | Tests/Cases/MemoryKernel/ARawPointerCastReadsTheSameAddress.fixture/misaligned.jet | 6:5 | "the cast Int starts one byte into the cell, so the sentry stops the read" |
| Tests | Tests/Cases/MemoryKernel/ARawPointerStepsByElementsOrBytes.fixture/main.jet | 5:5 | "every step stays inside the `values` tuple, which outlives each read" |
| Tests | Tests/Cases/MemoryKernel/ARawPointerStepsByElementsOrBytes.fixture/past.jet | 6:5 | "the step runs one Int past the local cell, so the sentry stops the read" |
| Tests | Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/fromaddrfreed.jet | 4:5 | "the address names an Int of a block already released" |
| Tests | Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/main.jet | 6:5 | "every address names a live, aligned value of the block, which outlives each access" |
| Tests | Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/misaligned.jet | 4:5 | "the read starts one byte into the block, so its Int is not aligned" |
| Tests | Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/readfreed.jet | 4:5 | "the read takes an Int of a block already released" |
| Tests | Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/readpast.jet | 4:5 | "the read takes the Int just past the block" |
| Tests | Tests/Cases/MemoryKernel/ARawReadOrWriteNeedsALiveAlignedValue.fixture/writepast.jet | 4:5 | "the write stores the Int just past the block" |
| Tests | Tests/Cases/MemoryKernel/ARawWindowChecksEachIndex.fixture/main.jet | 5:5 | "the window covers exactly the tuple's four Ints, which outlive it" |
| Tests | Tests/Cases/MemoryKernel/ARawWindowChecksEachIndex.fixture/past.jet | 5:5 | "the window covers exactly the tuple's four Ints, which outlive it" |
| Tests | Tests/Cases/MemoryKernel/ARawWindowChecksEachIndex.fixture/released.jet | 10:9 | "`base` names `count` live I64s while the capability lives" |
| Tests | Tests/Cases/MemoryKernel/ARawWindowChecksEachIndex.fixture/released.jet | 17:5 | "the block is used only through this capability and released once by `release`" |
| Tests | Tests/Cases/MemoryKernel/ARawWindowChecksEachIndex.fixture/released.jet | 27:5 | "`base` came from `alloc_raw` in `acquire`, and the capability is spent here, once" |
| Tests | Tests/Cases/MemoryKernel/AllocResizeFreeFuzz.fixture/main.jet | 26:5 | "`address` starts a live block of `size` bytes" |
| Tests | Tests/Cases/MemoryKernel/AllocResizeFreeFuzz.fixture/main.jet | 38:5 | "the block is written only inside its `size` bytes, and released once by `release`" |
| Tests | Tests/Cases/MemoryKernel/AllocResizeFreeFuzz.fixture/main.jet | 47:5 | "`address` came unchanged from `alloc_raw` in `claim`, and its block is released here once" |
| Tests | Tests/Cases/MemoryKernel/AllocResizeFreeFuzz.fixture/main.jet | 91:13 | "both blocks are live, `old` holds at least `shared` bytes and `fresh` holds `size`" |
| Tests | Tests/Cases/MemoryKernel/RawHeapStorageIsTakenAndReleased.fixture/aliased.jet | 4:5 | "a second release through a copy of the pointer, which the ownership check cannot see, so the runtime stops it" |
| Tests | Tests/Cases/MemoryKernel/RawHeapStorageIsTakenAndReleased.fixture/kept.jet | 4:5 | "a copy of the pointer is read after its block is released, so the sentry stops the read" |
| Tests | Tests/Cases/MemoryKernel/RawHeapStorageIsTakenAndReleased.fixture/main.jet | 4:5 | "each block is used only through pointers derived from it, and released once" |
| Tests | Tests/Cases/MemoryKernel/RawHeapStorageIsTakenAndReleased.fixture/twice.jet | 4:5 | "the second release is refused before the program runs" |
| Tests | Tests/Cases/MemorySemantics/ClosureBoxesFreeAtTheirLastOwner.fixture/main.jet | 313:5 | "the exit callback only reads its captured List" |
| Tests | Tests/Cases/MemorySemantics/UnsafeAddressKeepsStringAliveToBlockEnd.fixture/main.jet | 9:5 | "label is live on this frame until this block ends" |
| Tests | Tests/Cases/ModuleStorage/ModuleGlobalsStartLaidOutUnlessTheyHoldAHandle.fixture/main.jet | 114:5 | "one task writes the globals" |
| Tests | Tests/Cases/OSNative/CredentialsChangeOnEveryThread.fixture/main.jet | 78:5 | "the process is root of a disposable user namespace; it drops to a mapped user" |
| Tests | Tests/Cases/OSNative/ForkUnderLoadChildCompletes.fixture/main.jet | 47:9 | "the child only allocates, resolves and exits; the parent reaps it" |
| Tests | Tests/Cases/Observe/PanicContextUsesOnlyLexicallyLiveLocals.fixture/unsafe/panic_lexical_unsafe.jet | 4:5 | "the region only binds and prints a local; it reads and writes no raw memory" |
| Tests | Tests/Cases/OutputCallable/RunnableContractsAndSelectionFailInSema.fixture/probe3/main.jet | 3:1 | "raw boundary" |
| Tests | Tests/Cases/PerfRun/NoCounters.fixture/main.jet | 13:1 | "`program` is the address of a live struct sock_fprog whose filter block outlives the call; the kernel copies the filter before it returns" |
| Tests | Tests/Cases/PerfRun/NoCounters.fixture/main.jet | 90:5 | "the block holds the filter while seccomp copies it into the kernel, and is released once, after the call" |
| Tests | Tests/Cases/Pin/Fixtures/LibraryWiresSelfRefUnderUnsafeAndExposesSafePinAPI/fixture.jet | 9:5 | "node storage is fixed for the returned pin; self_addr names this place" |
| Tests | Tests/Cases/PlaceWindowLifetimes/BorrowedWindowStaysCallerOwned.fixture/main.jet | 5:5 | "the borrowed value is live in the caller" |
| Tests | Tests/Cases/PlaceWindowLifetimes/HeapElementWindowStaysAllocatorOwned.fixture/main.jet | 6:5 | "the list element remains heap-owned" |
| Tests | Tests/Cases/PlaceWindowLifetimes/StackWindowUsesExpiringSentryAddress.fixture/main.jet | 6:5 | "cell is live on this stack frame" |
| Tests | Tests/Cases/PolicyScope/UnsafeObligationsAreCheckedBeforeCodegen.fixture/run.jet | 5:5 | "local pointer" |
| Tests | Tests/Cases/ProcessSessions/Subjects/native/session_fixture.jet | 54:5 | "the fixture's descendant rejoins the process group of the tree that spawned it" |
| Tests | Tests/Cases/ProcessSessions/Subjects/native/session_fixture.jet | 96:9 | "kill -0 only asks whether the killed descendant still exists" |
| Tests | Tests/Cases/ProcessSignalDrain/AHandlerRunsWhenItsTaskIsAtItsGuard.fixture/main.jet | 29:9 | "the program signals itself: 0 only checks that it may, SIGINT is the subscribed signal" |
| Tests | Tests/Cases/ResourceClose/ReplacedListElementMapValueAndModuleStorageCloseBeforeTheyDrop.fixture/main.jet | 19:5 | "the test runs one task" |
| Tests | Tests/Cases/RunEntry/ModuleWriteFromFnIsE0111.fixture/ok.jet | 11:5 | "this program starts no task, so nothing else writes `raw`" |
| Tests | Tests/Cases/SingleUse/DropErasesNoUnsafeInCodegen.fixture/source-1.jet | 11:5 | "gone" |
| Tests | Tests/Cases/SingleUse/DropInsideUnsafeBlockSatisfiesSingleUse.fixture/source-1.jet | 11:5 | "the resource is already gone; nothing to release" |
| Tests | Tests/Cases/SingleUse/DropInsideUnsafeFnSatisfiesSingleUse.fixture/source-1.jet | 9:1 | "voids a freshly-acquired lock whose resource is already gone" |
| Tests | Tests/Cases/SingleUse/DropInsideUnsafeFnSatisfiesSingleUse.fixture/source-1.jet | 16:5 | "calling the audited voider" |
| Tests | Tests/Cases/SingleUse/HolderConsumedCompiles.fixture/main.jet | 28:5 | "the lease's job is done by hand here" |
| Tests | Tests/Cases/SingleUse/HolderConsumedCompiles.fixture/main.jet | 35:5 | "the slot's lease is done by hand here" |
| Tests | Tests/Cases/SingleUse/HolderDroppedIsE0140.fixture/main.jet | 33:5 | "the lease's job is done by hand here" |
| Tests | Tests/Cases/SingleUse/ReuseAfterDropIsE0121.fixture/source-1.jet | 11:5 | "done with it" |
| Tests | Tests/Cases/TargetMachines/Fixtures/SelectedTargetMachineRejectsDirectMMIOOutsideRegion/main.jet | 4:5 | "this address is intentionally outside the board MMIO region" |
| Tests | Tests/Cases/TargetMachines/Fixtures/SelectedTargetMachineValidatesDirectMMIOAccesses/main.jet | 4:5 | "timer register is mapped by board.sensor_v1" |
| Tests | Tests/Cases/UnsafeAndRuntime/ASentryFaultNamesTheAccessAndTheRange.fixture/steps.jet | 5:5 | "the second read steps one Int past the tuple, so the sentry stops it there" |
| Tests | Tests/Cases/UnsafeAndRuntime/AnOptionalRawPointerIsNoneWhenNull.fixture/main.jet | 12:5 | "each pointer names the live local cell, or is null and never read" |
| Tests | Tests/Cases/UnsafeAndRuntime/DerefPlaceLendsThePointeeToAWriteParameter.fixture/main.jet | 13:5 | "c lives in place until `run` returns" |
| Tests | Tests/Cases/UnsafeAndRuntime/DerefPlaceReceivesATraitSelfMethodThroughAPointer.fixture/main.jet | 21:5 | "address is the address of a live A that outlives this call" |
| Tests | Tests/Cases/UnsafeAndRuntime/DerefPlaceReceivesATraitSelfMethodThroughAPointer.fixture/main.jet | 30:5 | "c lives in place until `run` returns" |
| Tests | Tests/Cases/UnsafeAndRuntime/EveryUnsafeRegionKindEntersItsOwnSentryScope.fixture/regions.jet | 1:1 | "the cell is live on this frame" |
| Tests | Tests/Cases/UnsafeAndRuntime/EveryUnsafeRegionKindEntersItsOwnSentryScope.fixture/regions.jet | 9:5 | "call the gated reader" |
| Tests | Tests/Cases/UnsafeAndRuntime/EveryUnsafeRegionKindEntersItsOwnSentryScope.fixture/regions.jet | 13:5 | "test region calls the gated reader" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/FFICapabilityEdgeUsesSharedSentryPrelude/main.jet | 8:5 | "the foreign call may write through the borrowed value" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/MemAddressOfNeverFoldsPlainOrParenthesized/paren/main.jet | 3:1 | "reads through a raw pointer; addr must be a live, valid Int" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/MemAddressOfNeverFoldsPlainOrParenthesized/paren/main.jet | 12:5 | "addr is the address of `cell`, a live Int on this stack frame" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/MemAddressOfNeverFoldsPlainOrParenthesized/plain/main.jet | 3:1 | "reads through a raw pointer; addr must be a live, valid Int" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/MemAddressOfNeverFoldsPlainOrParenthesized/plain/main.jet | 12:5 | "addr is the address of `cell`, a live Int on this stack frame" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/SentryKernelIsSharedByAOTAndRuntimeAdapters/main.jet | 7:5 | "the local cell is live for this raw read" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/UnsafeFnBlockAndPtrOps/main.jet | 3:1 | "reads through a raw pointer; addr must be a live, valid Int" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/UnsafeFnBlockAndPtrOps/main.jet | 12:5 | "addr is the address of `cell`, a live Int on this stack frame" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/UnsafeTierEmitIsByteExact/main.jet | 3:1 | "reads through a raw pointer; addr must be valid" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/UnsafeTierEmitIsByteExact/main.jet | 12:5 | "safe: cell is live" |
| Tests | Tests/Cases/UnsafeAndRuntime/Fixtures/VolatileWriteEmitIsByteExact/main.jet | 3:1 | "UART TX register is mapped by the target machine" |
| Tests | Tests/Cases/UnsafeAndRuntime/HardenedProfileStopsAnOutOfBoundsRawAccessAtEveryLevel.fixture/read/read.jet | 6:5 | "reads the one Int the arena allocated" |
| Tests | Tests/Cases/UnsafeAndRuntime/HardenedProfileStopsAnOutOfBoundsRawAccessAtEveryLevel.fixture/write/write.jet | 6:5 | "writes the one Int the arena allocated" |
| Tests | Tests/Cases/UnsafeAndRuntime/RawPointerToolsKeepTheirChecksInHardenedBuilds.fixture/freed.jet | 4:5 | "no copy of the pointer is read after the block is released" |
| Tests | Tests/Cases/UnsafeAndRuntime/RawPointerToolsKeepTheirChecksInHardenedBuilds.fixture/misaligned.jet | 5:5 | "every Int read starts on an Int boundary" |
| Tests | Tests/Cases/UnsafeAndRuntime/RawPointerToolsKeepTheirChecksInHardenedBuilds.fixture/range.jet | 5:5 | "the steps stay inside the tuple's four Ints" |
| Tests | Tests/Cases/WebBuild/ModuleLocalDevIsValidatedAsWebRuntime.fixture/main.jet | 5:9 | "exercise the web gate" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/AES/nist_gcm.jet | 13:5 | "published KAT or frozen seed AES-GCM fixture" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/AES/seed_differential.jet | 13:5 | "published KAT or frozen seed AES-GCM fixture" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/aes256gcm_open.jet | 8:5 | "fixed NIST AES-256-GCM vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/aes256gcm_seal.jet | 7:5 | "fixed NIST AES-256-GCM vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/argon2id.jet | 10:5 | "fixed Argon2id known-answer vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/ed25519_sign.jet | 6:5 | "fixed RFC 8032 Ed25519 vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/ed25519_verify_strict.jet | 7:5 | "fixed RFC 8032 Ed25519 vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/hkdf_sha256_raw.jet | 8:5 | "fixed RFC 5869 HKDF vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/open_v1.jet | 7:5 | "fixed JETC v1 AES vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/secret_bytes.jet | 9:5 | "inspect fixed secret bytes" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/shared_secret_bytes.jet | 11:5 | "inspect shared-secret length" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/signing_key_bytes.jet | 9:5 | "inspect generated signing-key bytes" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/x25519_raw.jet | 7:5 | "fixed RFC 7748 X25519 vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/x25519_secret_bytes.jet | 9:5 | "inspect generated X25519 secret bytes" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/xchacha20poly1305_open.jet | 9:5 | "fixed CFRG XChaCha20-Poly1305 vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Expert/xchacha20poly1305_seal.jet | 9:5 | "fixed CFRG XChaCha20-Poly1305 vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Vault/commit_import_signing.jet | 5:5 | "restore a fixed signing key into the vault" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Vault/commit_import_x25519.jet | 5:5 | "restore a fixed X25519 secret into the vault" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Vault/guard_errors.jet | 33:5 | "vault key import: fixed guard vectors that never hold key material" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Vault/prepare_import_signing.jet | 5:5 | "restore a fixed signing-key vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/Vault/prepare_import_x25519.jet | 5:5 | "restore a fixed X25519 secret vector" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 15:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 19:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 32:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 36:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 49:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 53:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 66:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 70:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 83:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 87:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 98:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 109:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 120:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 131:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 142:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 153:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 164:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 175:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 186:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 197:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 208:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 219:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 230:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 241:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 252:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 263:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 274:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 285:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 296:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 307:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 323:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 334:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 350:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 361:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 377:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 388:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 404:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 415:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 431:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 442:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 458:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 469:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 485:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 496:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 512:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 523:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 539:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 550:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 566:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 577:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 593:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 604:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 620:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 631:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 647:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 658:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 674:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 685:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 701:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 712:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 728:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 739:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 755:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 766:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 782:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 793:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 809:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 820:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 836:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 847:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 863:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 874:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 890:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 901:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 917:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 928:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 944:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 955:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 971:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 982:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 998:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1009:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1025:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1036:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1052:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1063:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1079:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1090:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1106:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1117:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1133:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1144:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1160:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1171:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1187:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1198:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1214:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1225:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1241:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1252:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1268:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1279:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1295:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1306:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1322:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1333:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1344:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1355:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1366:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1377:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1388:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1399:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1410:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1426:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1442:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1458:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1476:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1480:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1493:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1497:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1510:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1514:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1527:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1531:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1544:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1548:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1561:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1565:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1578:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1582:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1595:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1599:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1612:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1616:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1629:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1633:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1646:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1650:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1663:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1667:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1680:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1684:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1697:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1701:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1714:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1718:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1731:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1735:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1748:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1752:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1765:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1769:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1782:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1786:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1799:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1803:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1816:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1820:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1833:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1837:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1850:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1854:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1867:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1871:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1884:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1888:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1901:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1905:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1918:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1922:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1935:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1939:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1952:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1956:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1969:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1973:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1986:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 1990:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2003:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2007:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2020:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2024:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2037:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2041:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2054:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2058:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2071:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2075:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2088:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2092:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2105:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2109:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2122:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2126:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2139:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2143:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2156:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2160:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2173:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2177:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2190:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2194:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2207:5 | "frozen Ed25519 sign witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2211:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2222:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_ed25519.jet | 2238:5 | "public Ed25519 acceptance witness" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/ct_x25519.jet | 5:5 | "published/frozen X25519 test inputs" |
| Tests | Tests/Conformance/Corpus/Core/Crypto/x25519.jet | 11:5 | "inspect shared-secret length" |
| Tests | Tests/Conformance/Corpus/Core/Files/Sealed/migrate_v1.jet | 12:5 | "fixed JETC v1 source failure" |
| Tests | Tests/Conformance/Corpus/Core/Mem/volatile_read.jet | 6:5 | "value is live on this stack frame and the pointer never escapes" |
| Tests | Tests/Conformance/Corpus/Core/Mem/volatile_write.jet | 6:5 | "value is live on this stack frame and the pointer never escapes" |
| Tests | Tests/Conformance/Corpus/Core/Sys/atexit.jet | 5:5 | "the exit callback has no captured state" |
| Tests | Tests/Conformance/Corpus/Core/Sys/close_fd.jet | 7:13 | "close an invalid descriptor without changing process state" |
| Tests | Tests/Conformance/Corpus/Core/Sys/fork.jet | 5:5 | "fork and let the child return immediately" |
| Tests | Tests/Conformance/Corpus/Core/Sys/getpriority.jet | 5:5 | "read the current process priority" |
| Tests | Tests/Conformance/Corpus/Core/Sys/initgroups.jet | 5:5 | "probe initgroups with a stable missing user" |
| Tests | Tests/Conformance/Corpus/Core/Sys/kill.jet | 5:5 | "check the process group without sending a signal" |
| Tests | Tests/Conformance/Corpus/Core/Sys/mkfifo.jet | 5:5 | "probe FIFO creation with an empty path" |
| Tests | Tests/Conformance/Corpus/Core/Sys/mknod.jet | 5:5 | "probe device-file creation with a major number out of range" |
| Tests | Tests/Conformance/Corpus/Core/Sys/pipe.jet | 5:5 | "open a pipe and observe its descriptor pair" |
| Tests | Tests/Conformance/Corpus/Core/Sys/reset_signal.jet | 5:5 | "restore the default disposition of SIGUSR1, which this program never handles, then refuse signal 0" |
| Tests | Tests/Conformance/Corpus/Core/Sys/set_group_ids.jet | 5:5 | "set the effective group ID to the one it already is, which needs no privilege, then refuse an ID out of range" |
| Tests | Tests/Conformance/Corpus/Core/Sys/set_scheduling.jet | 5:5 | "move this short-lived witness to batch time sharing and back; neither needs privilege" |
| Tests | Tests/Conformance/Corpus/Core/Sys/set_user_ids.jet | 5:5 | "set the effective user ID to the one it already is, which needs no privilege, then refuse an ID out of range" |
| Tests | Tests/Conformance/Corpus/Core/Sys/set_wall_clock.jet | 8:5 | "a time before 1970 is refused before it reaches the kernel, so the clock never changes" |
| Tests | Tests/Conformance/Corpus/Core/Sys/setgid.jet | 5:5 | "probe setgid with an invalid identifier" |
| Tests | Tests/Conformance/Corpus/Core/Sys/setpgid.jet | 5:5 | "set the current process into its existing process group" |
| Tests | Tests/Conformance/Corpus/Core/Sys/setpgrp.jet | 5:5 | "set the current process group" |
| Tests | Tests/Conformance/Corpus/Core/Sys/setpriority.jet | 5:5 | "lower the current process priority for this short-lived witness" |
| Tests | Tests/Conformance/Corpus/Core/Sys/setsid.jet | 5:5 | "create a session when the host permits it" |
| Tests | Tests/Conformance/Corpus/Core/Sys/setuid.jet | 5:5 | "probe setuid with an invalid identifier" |
| Tests | Tests/Conformance/Corpus/Core/Sys/stop.jet | 7:13 | "stop this dedicated witness successfully" |
| Tests | Tests/Conformance/Corpus/Core/Sys/umask.jet | 5:5 | "set a deterministic mask for this short-lived witness" |
| Tests | Tests/Conformance/Corpus/Core/Sys/utime.jet | 5:5 | "probe timestamp update with an empty path" |
| Tests | Tests/Conformance/Corpus/Core/Sys/wait.jet | 5:5 | "wait for a child that this witness does not have" |
| Tests | Tests/Conformance/Corpus/Core/Sys/waitpid.jet | 5:5 | "poll for a child that this witness does not have" |
| Tests | Tests/Conformance/Corpus/Core/Tasks/queued_handle.jet | 17:5 | "the handles stay in this program and never reach the runtime's queue" |
| Tests | Tests/Conformance/Corpus/Core/Tasks/queued_task.jet | 5:5 | "the handle stays in this program and never reaches the runtime's queue" |
| Tests | Tests/Fixtures/native_inline_c_shared_state.jet | 4:1 | "updates this program's private counter" |
| Tests | Tests/Fixtures/native_inline_c_shared_state.jet | 13:1 | "reads the counter defined by fixture_bump" |
| Tests | Tests/Fixtures/native_inline_c_shared_state.jet | 25:5 | "the counter is private to these two bodies" |
| Tests | Tests/Fuzz/Sema/Differential/ex_crypto_crypto_migration.jet | 29:5 | "AES-256-GCM protocol interoperability" |
| Tests | Tests/Fuzz/Sema/Valid/ex_crypto_crypto_suite.jet | 40:5 | "known-answer check reads the RFC 7748 shared secret" |
| Tests | Tests/Fuzz/Sema/Valid/ex_lowlevel_lowlevel.jet | 8:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| Tests | Tests/Fuzz/Sema/Valid/ex_lowlevel_lowlevel.jet | 22:5 | "`cell` is a live Int on this stack frame; the pointer never escapes" |
| Tests | Tests/Fuzz/Sema/Valid/ex_lowlevel_pointer_cast_deref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
| Tests | Tests/Run/Comptime/ForeignBranchNotTaken.jet | 12:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Tests | Tests/Run/Comptime/ForeignBranchNotTaken.jet | 24:9 | "call the audited register-only assembly contract" |
| Tests | Tests/Run/Comptime/MutableGlobalStartsFromAnyStorableValue.jet | 56:5 | "one task writes the globals" |
| Tests | Tests/Run/Comptime/MutableGlobalStartsFromComputedValue.jet | 32:5 | "one task writes the frame" |
| Tests | Tests/Run/Comptime/MutableGlobalStartsFromComputedValue.jet | 43:5 | "one task writes the globals" |
| Tests | Tests/Run/CoreCryptoFoundations/CryptoAeadTamperingIsRejectedAcrossTiers.jet | 64:5 | "fixed AEAD tamper vector" |
| Tests | Tests/Run/CoreCryptoFoundations/PrimitiveMatrix.jet | 101:5 | "known-answer check reads the RFC 7748 shared secret" |
| Tests | Tests/Run/CoreCryptoFoundations/PrimitiveMatrix.jet | 163:5 | "expert known-answer vectors need raw key bytes" |
| Tests | Tests/Run/CoreIO/CleanupMatrix.jet | 35:5 | "the stop runs the shared cleanup boundary" |
| Tests | Tests/Run/CoreIO/CleanupMatrix.jet | 83:5 | "the exit callbacks have no captured state" |
| Tests | Tests/Run/Dyn/NumberInUnsafe.jet | 7:5 | "the literal below is a valid JSON number lexeme" |
| Tests | Tests/Run/FailureFoundation/TransactOSStop.jet | 34:5 | "the stop runs the shared cleanup boundary" |
| Tests | Tests/Run/PatternsAndFields/FixedSizeListWidensAtCoreCall.jet | 5:5 | "fixed signature vector" |
| Tests | Tests/Run/UnsafeAndRuntime/AsmSevenInputsPushToTheStack.jet | 7:1 | "the syscall may read at most four initialized stack words and retains no pointer; all pushes are balanced before return" |
| Tests | Tests/Run/UnsafeAndRuntime/AsmSevenInputsPushToTheStack.jet | 54:5 | "Linux getpid syscall 39 ignores all argument registers and retains no stack pointer" |
| Tests | Tests/Run/UnsafeAndRuntime/MemFromAddrDirect.jet | 11:5 | "the borrowed value is live in the caller and the pointer never escapes" |
| Tests | Tests/Run/UnsafeAndRuntime/NativeGlobalWord.jet | 6:5 | "run, the program's one thread, is the only writer" |
| Tests | Tests/UI/AuditedGateLadderForbidden/run.jet | 3:5 | "the organization policy refuses this audited escape" |
| Tests | Tests/UI/E3113PackageWithoutKernelFiles/run.jet | 6:1 | "the caller releases the storage with `free_raw`, exactly once" |
| Tests | Tests/UI/E3113RawMemoryOutsideKernel/kernel.jet | 4:1 | "the caller releases the storage with `free_raw`, exactly once" |
| Tests | Tests/UI/E3113RawMemoryOutsideKernel/run.jet | 5:1 | "the caller releases the storage with `free_raw`, exactly once" |
| Tests | Tests/UI/E3113RuntimeUnitSourceOutsideKernel/run.jet | 6:1 | "each caller vouches for the address" |
| Tests | Tests/UI/E3113RuntimeUnitSourceOutsideKernel/run.jet | 10:5 | "the kernel's own leaf reads a word its caller lends" |
| Tests | Tests/UI/E3113RuntimeUnitSourceOutsideKernel/run.jet | 16:5 | "the host holds the word at this address" |
| Tests | Tests/UI/E3114RawAddressIntoKernel/run.jet | 8:5 | "the caller hands the address of a live Int" |
| Tests | Tests/UI/E3114RawAddressIntoKernel/run.jet | 18:5 | "kernel code hands the address of a live Int" |
| Tests | Tests/UI/GatesLoadPackageCeiling/run.jet | 2:5 | "audited load-only gate" |
| Tests | Tests/UI/Ownership/raw_window_read_after_its_owner_is_released.jet | 11:5 | "the cells hold no resource, so spending them frees nothing" |
| Tests | Tests/UI/Ownership/raw_window_read_after_its_owner_is_released.jet | 16:5 | "the window covers exactly the two Int fields, which `cells` keeps" |
| Tests | Tests/UI/Ownership/single_use_holder_dropped_owes_consume.jet | 33:5 | "the lease's job is done by hand here" |
| Tests | Tests/UI/SecretsExpiry/expiring_secret_loan_rejects_move_variadic_and_drop_paths_3.jet | 13:9 | "attempt to discard a loan" |
| Tests | Tests/UI/UnsafeAndRuntime/VolatileWriteEmitIsByteExact/run.jet | 3:1 | "UART TX register is mapped by the target machine" |
| Tests | Tests/UI/UnsafeEffectBudget/unsafedep/unsafedep.jet | 3:5 | "audited peek" |
| Tests | Tests/UI/UnsafeForbidden/run.jet | 2:5 | "not allowed" |
| Tests | Tests/UI/UnsafeForbiddenByOrganizationPolicy/run.jet | 5:5 | "the organization policy forbids this escape" |
| Tests | Tests/UI/UnsafePerSite/run.jet | 2:5 | "selection required" |
| Tests | Tests/UI/cffi_out_pointer_requires_unsafe.jet | 14:1 | "the local slot remains live for the complete C call" |
| Tests | Tests/UI/changeset_a_failed_handler_publishes_none.jet | 28:25 | "generated" |
| Tests | Tests/UI/checked_text_raw_requires_unsafe.jet | 22:5 | "audited literal" |
| Tests | Tests/UI/crypto_argon2id_literal_policy.jet | 9:5 | "fixed password-hash vectors" |
| Tests | Tests/UI/crypto_argon2id_literal_policy_type_precedence.jet | 9:5 | "fixed password-hash vector" |
| Tests | Tests/UI/crypto_argon2id_literal_policy_valid.jet | 9:5 | "protocol-selected password policy" |
| Tests | Tests/UI/crypto_argon2id_literal_policy_valid.jet | 17:5 | "fixed password-hash vectors" |
| Tests | Tests/UI/crypto_expert_material_length.jet | 12:5 | "fixed interop vectors" |
| Tests | Tests/UI/crypto_expert_material_length_type_precedence.jet | 5:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_expert_material_length_valid.jet | 5:5 | "protocol-selected interop material" |
| Tests | Tests/UI/crypto_expert_material_length_valid.jet | 17:5 | "fixed interop vectors" |
| Tests | Tests/UI/crypto_expert_nonce_callback_bound_precedence.jet | 13:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_expert_nonce_correct.jet | 5:5 | "fixed interop vectors" |
| Tests | Tests/UI/crypto_expert_nonce_effect_bound_precedence.jet | 10:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_expert_nonce_effect_precedence.jet | 10:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_expert_nonce_effect_prohibition_precedence.jet | 10:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_expert_nonce_effect_unknown_precedence.jet | 10:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_expert_nonce_length.jet | 5:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_expert_nonce_region_authority_precedence.jet | 9:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_expert_nonce_trait_dispatch_precedence.jet | 9:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_expert_nonce_type_precedence.jet | 5:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_hkdf_output_length.jet | 9:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_hkdf_output_type_precedence.jet | 5:5 | "fixed interop vector" |
| Tests | Tests/UI/crypto_hkdf_output_valid.jet | 8:5 | "protocol-selected output length" |
| Tests | Tests/UI/crypto_hkdf_output_valid.jet | 16:5 | "fixed interop vectors" |
| Tests | Tests/UI/discarded_pure_quiet.jet | 47:5 | "the test deliberately keeps this stderr write behind its audit boundary" |
| Tests | Tests/UI/e3106_unsafe_skip_without_per_site.jet | 3:5 | "reads a checked constant" |
| Tests | Tests/UI/ffi_asm_duplicate_clobber.jet | 3:1 | "each discarded register is named once" |
| Tests | Tests/UI/ffi_asm_float_signature.jet | 3:1 | "register-only increment" |
| Tests | Tests/UI/ffi_asm_missing_return_anchor.jet | 3:1 | "cycle counter" |
| Tests | Tests/UI/ffi_asm_no_mem.jet | 1:1 | "register-only increment" |
| Tests | Tests/UI/ffi_asm_return_clobber.jet | 3:1 | "the result register is overwritten by the body" |
| Tests | Tests/UI/ffi_asm_unencoded_mnemonic.jet | 3:1 | "the body writes only its result register" |
| Tests | Tests/UI/ffi_asm_unknown_clobber.jet | 3:1 | "the body declares only audited registers" |
| Tests | Tests/UI/ffi_body_not_string.jet | 1:1 | "demo" |
| Tests | Tests/UI/ffi_inline_asm_call_needs_unsafe_region.jet | 3:1 | "the operands are scalar registers and r10 is a declared clobber" |
| Tests | Tests/UI/ffi_inline_asm_call_needs_unsafe_region.jet | 14:5 | "call the audited register-only assembly contract" |
| Tests | Tests/UI/ffi_inline_c_call_needs_unsafe_region.jet | 1:1 | "the scalar ABI contract matches the C definition" |
| Tests | Tests/UI/ffi_inline_c_call_needs_unsafe_region.jet | 7:5 | "call the audited inline C contract" |
| Tests | Tests/UI/ffi_inline_c_mismatch.jet | 1:1 | "deliberate mismatch fixture" |
| Tests | Tests/UI/ffi_inline_cpp_call_needs_unsafe_region.jet | 1:1 | "the scalar ABI contract matches the C++ definition" |
| Tests | Tests/UI/ffi_inline_cpp_call_needs_unsafe_region.jet | 7:5 | "call the audited inline C++ contract" |
| Tests | Tests/UI/ffi_unknown_argument.jet | 1:1 | "demo" |
| Tests | Tests/UI/ffi_unknown_language.jet | 1:1 | "demo" |
| Tests | Tests/UI/inline_always_foreign.jet | 5:1 | "register-only increment" |
| Tests | Tests/UI/inline_always_foreign.jet | 10:1 | "the scalar ABI contract matches the C definition" |
| Tests | Tests/UI/inline_always_foreign.jet | 15:1 | "this program owns the exit status it passes" |
| Tests | Tests/UI/inline_always_foreign.jet | 19:5 | "call the audited foreign contracts" |
| Tests | Tests/UI/lowlevel_e3102.jet | 2:5 | "addr is valid" |
| Tests | Tests/UI/lowlevel_e3103.jet | 3:1 | "reads through caller-provided raw pointer" |
| Tests | Tests/UI/lowlevel_volatile_write_without_mem.jet | 2:5 | "addr is valid" |
| Tests | Tests/UI/lowlevel_volatile_write_wrong_ptr.jet | 4:5 | "plain ints are not raw pointers" |
| Tests | Tests/UI/lowlevel_volatile_write_wrong_value.jet | 6:5 | "slot is live and pointer never escapes" |
| Tests | Tests/UI/meta_ladder_add_never_authorizes_unsafe.jet | 7:13 | "generated" |
| Tests | Tests/UI/module_write_from_fn_is_e0111.jet | 13:5 | "this program starts no task, so nothing else writes `raw`" |
| Tests | Tests/UI/runtime_r0801.jet | 8:5 | "external address must be a live allocation" |
| Tests | Tests/UI/runtime_r0801_misaligned_without_storage.jet | 7:5 | "a fixed odd address stands for a pointer with no storage" |
| Tests | Tests/UI/runtime_r0802.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| Tests | Tests/UI/runtime_r0803.jet | 7:5 | "the read starts one byte into a live Int" |
| Tests | Tests/UI/secret_read_requires_unsafe.jet | 8:5 | "the length leaves this function, never the key bytes" |
| Tests | Tests/UI/sink_needs_checked_text.jet | 26:5 | "audited: the text is a fixed literal" |
| Tests | Tests/UI/unsafe_block_after_statement_parse_error.jet | 8:5 | "target outlives the read in this frame" |
| Tests | Tests/UI/unsafe_effect_module_cap.jet | 4:5 | "audited peek" |
| Tests | Tests/UI/unsafe_effect_prohibition_transitive.jet | 4:5 | "audited peek" |
| Tests | Tests/UI/unsafe_obligation_missing.jet | 5:5 | "local pointer" |
| Tests | Tests/UI/unsafe_obligation_no_bleed.jet | 6:5 | "two independent pointer operations" |
| Tests | Tests/UILint/inline_asm_reads_its_core_mem_gate.jet | 8:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Tests | Tests/UILint/inline_asm_reads_its_core_mem_gate.jet | 19:5 | "call the audited register-only assembly contract" |
| Tests | Tests/UILint/raw_pointer_reads_its_core_mem_gate.jet | 10:5 | "the local cell is live for this raw read" |
| Tools | Tools/Agent/Cache/Lanes.jet | 325:21 | "the guard signals only compiler, build and test jobs of this user, chosen by name from /proc" |
| Tools | Tools/Agent/Orchestrator/LaneCommit.jet | 224:5 | "signal 0 delivers nothing: it only asks whether a queued job's caller still runs" |
| Tools | Tools/CI/CompiledWorkloadPeerLauncher.jet | 655:9 | "posix kill: every process in the sandbox's private PID namespace except its init and this supervisor belongs to the supervised command" |
| Tools | Tools/CI/CompiledWorkloadPeerLauncher.jet | 669:5 | "posix signal reset + kill: restore SIG_DFL for one signal number, then re-raise it on this process so the caller observes the supervised command's signal" |
| Tools | Tools/CI/CompiledWorkloadPeerProbe.jet | 78:5 | "posix kill: the probe ends itself with SIGTERM so the check observes the launcher propagating a signal death" |
| Tools | Tools/Packaging/NativeCCompiler.jet | 77:9 | "re-raise the compiler's terminating signal on this process, as the parent shell expects" |
| Tools | Tools/Perf/AsmParity/Allocfree/run.jet | 48:5 | "each slot holds the address of one block alloc_raw gave and nothing else does; the block is released once, before its slot takes the next one or after the passes" |
| Tools | Tools/Perf/AsmParity/Memcopy/run.jet | 64:5 | "the block holds the source, the target and the moved bytes, LARGEST bytes and their room each, every offset stays inside its room, and the block is freed once after the passes" |
| Tools | Tools/Perf/AsmParity/Memfill/run.jet | 52:5 | "the block holds LARGEST bytes and their room, every fill stays inside it, and the block is freed once after the passes" |
| Tools | Tools/Perf/AsmParity/Rawword/run.jet | 44:5 | "the block holds WORDS words, every index is below WORDS, and the block is freed once after the passes" |
| Tools | Tools/Perf/AsmParity/Resize/run.jet | 54:5 | "every append reads the first `op.size` of the 64 bytes `source` holds, and `source` neither grows nor drops while the appends run" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Direct/main.jet | 25:9 | "each index is checked against the block's `count` words, and the block is released once, at the end of the turn" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Direct/main.jet | 47:5 | "each index is checked against the block's `count` words, and each block is released once" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Direct/main.jet | 76:5 | "both blocks hold WORDS words, each index is below WORDS, and each block is released once, at the end" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Direct/main.jet | 133:9 | "each index is checked against the mapping's `count` words, and the region stays mapped until its close" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Direct/main.jet | 158:5 | "the block holds `count` words, each index is below `count`, and the block is released once, at the end" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 14:9 | "the block is used only through this owner, which releases it once, at close" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 24:9 | "`at` is one of the owner's live words" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 29:9 | "`at` is one of the owner's live words" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 36:9 | "the owner keeps its `count` words alive while the View lives" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 43:9 | "the owner's words are live I64s, which own nothing" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 51:9 | "both owners hold at least `count` live I64s" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 59:9 | "both owners hold `count` live I64s" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 72:9 | "the fresh block takes the owner's words, and the old block is released once, here" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 86:13 | "`base` came from `alloc_raw` at `claim` or `grow`, and the owner is spent here" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 122:9 | "the region's pages stay mapped, private and writable until the owner closes it" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 132:9 | "`at` is one of the mapping's words" |
| Tools | Tools/Perf/CodegenCeiling/ZeroCost/Kernel/kernel.jet | 139:9 | "`at` is one of the mapping's words" |
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
  {"file": "Core/Crypto/Jetc.jet", "function": "writer", "proof": "The gate (:271-273) copies the ephemeral Secret through expert.secret_bytes straight into X25519SecretKey.from_bytes, which takes the copy and accepts exactly 32 bytes (crypto.jet:244-247); :269 already checked that width, so the refusal path is unreachable. The copy then lives only in the zeroizing X25519SecretKey (Close wipes, crypto.jet:254-258) and the moved ephemeral Secret keeps its own Close. Only the derived public key (:274) and per-recipient wraps leave; the coordinator draws the four inputs fresh for each container (Core/Files/Sealed.jet:372-375)."},
  {"file": "Core/Files/Sealed.jet", "function": "migrate_v1", "proof": "The checker refuses a migrate_v1 call outside #Unsafe with E0510 (JetSema Sema/Calls/Core.jet:209-214), so every caller audits the raw key. The legacy buffer is bounded (≤1 GiB + 34, :144-150); expert.open_v1 authenticates before any plaintext exists (:155-157), and that plaintext sits in a JetcBytes buffer wiped at close; the staged v2 output is re-opened and verified (:402, :429-451) before publication."},
  {"file": "Core/Crypto/Crypto.jet", "function": "__vault_wrapped_from_bytes", "proof": "The gate (:314-317) forwards the moved bytes to vault.__wrapped_from_bytes (vault.jet:182-186), which returns a WrappedVaultKey only after wrapped_metadata (vault.jet:487-) bounds the blob to 16..8192 bytes and checks the JVKW magic and header fields; failures become KeyWrapError. The carrier holds the wrapped (encrypted) container, never plaintext key material. The `__` name is reserved (E0067 outside Core), so users reach it only as the typed constructor WrappedVaultKey.from_bytes (BuiltinStatics.jet:145, 148)."},
  {"file": "Core/Crypto/Crypto.jet", "function": "__vault_wrapped_bytes", "proof": "vault.__wrapped_bytes (vault.jet:187) returns an owned copy of the carrier's `encoded` field: the same authenticated wrapped container that WrappedVaultKey.from_bytes validated or the vault export produced. No unwrapped key or Secret is read, and the carrier stays intact. The `__` name is reserved (E0067 outside Core), so only Core and compiler lowering call it."},
  {"file": "Core/Crypto/Crypto.jet", "function": "__vault_unlock_recipient", "proof": "vault.__key_unlock_recipient (vault.jet:203-205) builds KeyUnlock{kind: 1, secret: identity.bytes[0..<len]}: a read View of the caller's X25519SecretKey bytes, not a copy, so no second copy of the identity exists and the owner keeps its zeroizing Close. The gate performs no raw memory operation; the view's lifetime is ordinary type checking of the view-carrying KeyUnlock (vault.jet:198-201; emitted as the borrowing JetVaultKeyUnlock<'_>, Emit/Types.jet:3201). Its consumer is vault.prepare_import_wrapped (vault.jet:275). Users reach it only as KeyUnlock.Recipient(identity) (BuiltinStatics.jet:146, 149)."},
  {"file": "Core/Crypto/Crypto.jet", "function": "__vault_unlock_passphrase", "proof": "vault.__key_unlock_passphrase (vault.jet:206-208) builds KeyUnlock{kind: 2, secret: passphrase.bytes[0..<len]}: a read View of the caller's Secret, not a copy, so the passphrase owner keeps its zeroizing Close. The gate performs no raw memory operation; the view's lifetime is ordinary type checking of the view-carrying KeyUnlock (vault.jet:198-201; emitted as the borrowing JetVaultKeyUnlock<'_>, Emit/Types.jet:3201). Its consumer is vault.prepare_import_wrapped (vault.jet:275). Users reach it only as KeyUnlock.Passphrase(secret) (BuiltinStatics.jet:147, 150)."},
  {"file": "Core/Crypto/Crypto.jet", "function": "poly1305", "proof": "Not a core.crypto export (Core.rows:44), so its callers are crypto.jet's chacha_poly_seal (:2209) and chacha_poly_open (:2228). Both pass otk = chacha20_block(key, nonce, 0) (:2200, :2220), the RFC 8439 §2.6 one-time key, over the AEAD framing aad ∥ pad16 ∥ ct ∥ pad16 ∥ le64(aad) ∥ le64(ct); the 64-byte block covers the 32 key bytes fixedcrypto.poly1305 reads (expert_ct.jet:628). The key is therefore one-time per (key, nonce): xchacha_seal's callers use a random 24-byte nonce for the payload (:466, :506) and HKDF nonces under fresh ephemeral keys for wraps (:486-496, :1161-1174, :1190). open compares tags in constant time before decrypting (:2229)."},
  {"file": "Core/Crypto/Crypto.jet", "function": "x25519_scalarmult", "proof": "Not a core.crypto export (Core.rows:44). Its callers are x25519 (:1012), which first requires 32-byte secret and public keys (:1009-1010) and rejects an all-zero shared secret (:1013), x25519_public (:1019) after a 32-byte check (:1018), and X25519SecretKey.public_key (:251) with the fixed 32-byte base point. fixedcrypto.x25519 panics on any other width rather than reading past its inputs (expert_ct.jet:150), wipes its workspace (:201), and returns a fresh 32-byte list."},
  {"file": "Core/Crypto/Crypto.jet", "function": "ed25519_sign_raw", "proof": "Not a core.crypto export (Core.rows:44). Its only caller sign (:998) requires a 32-byte SigningKey seed first (:997). fixedcrypto.ed_sign hashes the seed into a private workspace, wipes it (expert_ct.jet:355-385), and returns the 64-byte signature; the seed is never copied out."},
  {"file": "Core/Crypto/Crypto.jet", "function": "ed25519_verify_raw", "proof": "Not a core.crypto export (Core.rows:44). Its only caller verify (:1005) requires a 32-byte public key and a 64-byte signature (:1002-1004); fixedcrypto.ed_verify rejects other widths itself (expert_ct.jet:512). Only public inputs are read and only a Bool returns."},
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
  {"file":"Core/Files/Data.jet","function":"bridge_tool_on_path","proof":"__host_path_is_file invokes OsStr::from_encoded_bytes_unchecked (Top/DataStats.rs:11-13), so its input must retain native encoded-byte provenance. data.jet:1090 obtains PATH from the current host's encoded environment; :1092-1108 split only at ASCII colon/semicolon and remove Windows ASCII quotes. :1111-1117 append only ASCII separators and valid UTF-8 tool text, preserving that host encoding. The metadata leaf borrows the owned list and returns Bool/error, not a pointer; the caller cannot supply an unrelated byte allocation to this helper or retain a host borrow."},
  {"file":"Core/Data/Data.jet","function":"data_loader_render","proof":"The gate at data.jet:1729 only matches the compiler-private Dyn Number, TypedText and Bytes variants. Each payload is accessed after its matching discriminant at :1730, :1734 or :1738, then read through ordinary String/List operations into the caller's exclusively borrowed output; no unchecked memory, forged carrier or raw pointer is created. Other variants leave the gate for normal recursive rendering. Any typed Dyn input remains a valid enum; callers cannot select a mismatched native payload."},
  {"file":"Core/Data/Data.jet","function":"data_schema_value_type","proof":"Not a core.data export (Compiler/Prelude/Core.rows:69). Its #Unsafe block (Data.jet:3674-3677) only pattern-matches the compiler-private Dyn carriers .Number and .Bytes to name their schema type; it reads no memory, builds or mutates no carrier and returns a static String, so no caller can break those carriers' invariants through it. Every other Dyn shape is matched through the public variants (:3678-3688)."},
  {"file":"Core/Process/Process.jet","function":"cmd","proof":"The __spec_fields leaf is layout transfer into ProcessSpec, not process execution (Top/ProcessSpec.rs:6-37). process.jet:92 takes all fields from default_fields(:419-428): paired environment columns are both empty; optional cwd, stdin, timeout, resource limits, terminal and policy are accompanied by their actual presence flags at :95-104, with valid typed placeholders. The sole present numeric limit is 67108864. argv is a typed owned String list; even empty argv is deferred to checked spawn, not indexed here. No descriptor or memory address is supplied or returned, and callers cannot alter the hidden initialized field set."},
{"file":"Core/Net/Net.jet","function":"ip_addr","proof":"__ip_from_bytes expects exactly four or sixteen octets. net.jet:136 accepts only parse_ip_bytes success: parse_ipv4_bytes(:960-979) checks four decimal octets <=255, and the IPv6 parser(:982-1048) checks eight 16-bit words including compression before expanding to sixteen bytes. The exact owned list moves at :138; callers cannot substitute a differently sized list after validation."},
{"file":"Core/Net/Net.jet","function":"ip_to_string_impl","proof":"net.jet:143-144 call __ip_bytes on a borrowed typed IPAddr; Top/NetHTTP.rs:1944-1948 discriminant-matches its native IPv4/IPv6 enum and copies exactly its initialized octets into an owned list. Formatting only reads that list. No caller-selected index, raw pointer or borrowed native storage escapes."},
{"file":"Core/Net/Net.jet","function":"ip_is_ipv4","proof":"__ip_is_v4 at net.jet:149 only matches the valid native IPAddr discriminant (Top/NetHTTP.rs:1951-1952). Any typed address is admitted; no payload reinterpretation, mutation or memory borrow is returned, only Bool."},
{"file":"Core/Net/Net.jet","function":"socket_addr_parse","proof":"At net.jet:182-189 parsing produces exactly sixteen IPv6 bytes or four IPv4 bytes, with the IPv6 length checked at :183. parse_port(:846-856) bounds decimal port to 0..65535 and parse_scope(:1144-1154) bounds scope to 0..4294967295 before :193-194 consume the octets and IP into __ip_from_bytes/__socket_from_parts. Flow is the constant zero. Callers cannot select uninitialized representation fields or overflow the U16/U32 transfer."},
{"file":"Core/Net/Net.jet","function":"socket_host","proof":"__socket_ip at net.jet:199 borrows a valid SocketAddr and copies its native IP enum (Top/NetHTTP.rs:2038-2039). ip_to_string_impl then reads copied octets for source formatting; the caller receives owned text, not a reference into the socket carrier. No raw address is accepted."},
{"file":"Core/Net/Net.jet","function":"socket_port","proof":"__socket_port at net.jet:202 borrows a typed SocketAddr and widens its initialized native u16 port to Int (Top/NetHTTP.rs:2042-2043). This scalar projection accepts every valid carrier and neither exposes a borrow nor changes socket ownership."},
{"file":"Core/Net/Net.jet","function":"socket_scope","proof":"__socket_scope at net.jet:215 matches the valid socket enum, returning None for IPv4 or widening the IPv6 u32 scope (Top/NetHTTP.rs:2046-2050). Source substitutes zero for None. No reinterpretation or caller-controlled pointer is involved and only an owned scalar escapes."},
{"file":"Core/Net/Net.jet","function":"udp_packet_addr","proof":"__udp_packet_addr at net.jet:413 borrows a typed UDPPacket and clones its stored SocketAddr (Top/NetHTTP.rs:2851-2852); the packet remains live during the projection. The return carrier owns its address rather than pointing into packet storage, so releasing the packet cannot invalidate the result."},
{"file":"Core/HTTP/Client.jet","function":"__build_socket","proof":"Reserved __ name; it copies the resolver octets into a fresh owned list (Client.jet:653-654) and moves it into core.net.__ip_from_bytes, whose runtime refuses any length but 4 or 16 before allocating (JetRuntime HostNetAddress.jet:31-32), so a malformed address cannot overrun the native carrier. U16{port} stops with E3010 when the port does not fit (core-library.md:4172-4173), and jet_http_build_get already rejects ports outside 0..65535 (:590). It returns an owned SocketAddr; no pointer or borrow escapes."},
{"file":"Core/Jobs/Jobs.jet","function":"queue","proof":"The gates at jobs.jet:130-136 call __path_kind and __create_dir_all_raw with owned String paths computed by queue_default_path/files.dirname, after endpoint authority validation at :124-126. Metadata is a scalar kind query and recursive mkdir returns owned IOError; neither leaf accepts or returns memory addresses. The kind check at :131-132 rejects observed symlinks before the checked SQLite open. Path races do not create a borrowed descriptor or dangling pointer because these are independent pathname operations, not identity-handle transfers."},
{"file":"Core/Jobs/Jobs.jet","function":"wait","proof":"The unsafe __sleep_ms leaf casts signed milliseconds to unsigned host Duration (Top/ServiceAuthority.rs:3792-3793). jobs.jet:386-387 reject negative/oversized timeout and :393 clamps the exact delay passed at :395 to 1..25, independently of wall-clock movement. No caller can reach the cast with a negative or oversized delay; the leaf only sleeps and exports no memory."},
{"file":"Core/Jobs/Jobs.jet","function":"queue_publish_transition","proof":"jobs.jet:486 constructs owned JSON fields and :487 clamps negative timestamps before __publish_parts(:489). The provider Top/ServiceAuthority.rs:3772-3787 validates JetDevtoolsEvent::from_parts before publication, owns entity/fields and borrows source/kind only during the call. No native pointer, callback or mutable queue storage is transferred to a sink; caller-chosen observation text cannot escape that validation boundary."},
{"file":"Core/Jobs/Jobs.jet","function":"queue_publish_status","proof":"jobs.jet:499-502 assemble owned status fields/entity and nonnegative timestamp, then __publish_parts(:504) invokes the existing validating JetDevtoolsEvent::from_parts gate (Top/ServiceAuthority.rs:3779-3786). The sink gets an owned validated event rather than references into JobQueueStatus, so callers cannot make event storage outlive a borrowed source value."},
{"file":"Core/Testing/Testing.jet","function":"testing_owned_text","proof":"__bytes_into_text at testing.jet:376 requires valid UTF-8. The only callers are testing_unified_diff(:503) and testing_ascii_name(:517). Diff output appends complete String bytes or scalar-aligned prefixes chosen by testing_boundary(:362-366,423-424,450-451); marker fragments are ASCII. Name output(:508-515) appends only ASCII alphanumerics, hyphen or underscore. Each exact owned list is copied into a String with no mutation between construction and transfer; arbitrary caller bytes cannot reach this local helper."},
{"file":"Core/TUI/TUI.jet","function":"capability_env_bytes","proof":"__cap_env_bytes at tui.jet:101 borrows a String name and returns an owned optional byte list. Its provider Ui.rs:514-515 obtains var_os and consumes OsString into encoded bytes; no borrowed environment storage is returned and source does not assume UTF-8 (capability_text uses checked String.from_bytes at :112). Arbitrary names cannot manufacture a native pointer or invalidate returned storage."},
  {"file":"Core/Encoding/CBOR.jet","function":"validate","proof":"The cbor.jet:95 gate only matches Dyn's private Number/TypedText variants to reject them(:97-98), and recursively validates initialized Array/Object payloads(:99-104). Discriminant matching precedes payload access; ordered Object storage is borrowed through object_entries while value remains live. No raw address, variant transmute or payload reference escapes in the error/unit result."},
  {"file":"Core/Encoding/CBOR.jet","function":"encode_value","proof":"The cbor.jet:157 gate grants access to private Dyn.Bytes only after its matching branch(:171); all byte reads iterate the owned list length(:173). Object entry views are taken only in the Object branch(:179-180) and retain the value borrow throughout bounded index ordering. encode(:111) validates the tree first, rejecting internal Number/TypedText. The caller's exclusively borrowed output receives copied bytes, never native payload references."},
  {"file":"Core/Encoding/CBORStream.jet","function":"retain_key","proof":"__bytes_into_text at cbor_stream.jet:208 copies stored, which :205 fills with exactly text.count_bytes() bytes obtained through bounds-checked stream.byte_at from a valid String. It neither truncates a scalar nor edits its encoding. Only that exact copy becomes the retained key; arbitrary wire bytes cannot reach this transfer."},
  {"file":"Core/Encoding/CBORStream.jet","function":"next_inner","proof":"For a CBOR text item cbor_stream.jet:303 obtains the owned payload, :306 strictly validates that exact list's UTF-8 and returns Syntax on failure, then :308 immediately copies it through __bytes_into_text. No mutation intervenes, and binary items(:304) never use the text transfer. Host/wire callers cannot bypass the validator with malformed payload bytes."},
  {"file":"Core/Encoding/Eetf.jet","function":"term","proof":"A method of the private TermWriter (Eetf.jet:521), which is not among core.encoding.eetf's exports (Compiler/Prelude/Core.rows:103), and depth-bounded (:548). Its #Unsafe region (:549-603) only pattern-matches the compiler-private Dyn carriers .Number and .Bytes to encode them; every byte is written through the bounds-checked put, put_u32 and payload helpers after item() length limits, and object entries come from stream.object_entries, itself a recorded safe abstraction. No carrier is constructed or mutated and no memory is addressed."},
  {"file":"Core/DB/Query.jet","function":"query_text","proof":"csv.jet:315-316 enforce the bounded read's size/list limits and :317 validates sample.bytes with the strict shared UTF-8 validator. The gate at :318-319 immediately copies that same owned list through __bytes_into_text; failures return FieldError before conversion. A caller's arbitrary file contents cannot produce an invalid String or retain a native read-buffer borrow."},
  {"file":"Core/Encoding/CSV.jet","function":"finish_field","proof":"CSVReader.character(:725-744) validates lead width, all continuation bytes, overlong/surrogate/out-of-range cases before next_inner(:880-882) calls push. push(:798-874) appends each entire validated scalar or only ASCII quote/CRLF escapes. Consequently finish_field's __bytes_into_text(:769) copies only complete UTF-8 storage, then resets field(:771). Partial/invalid wire scalars fail without reaching this gate."},
  {"file":"Core/Encoding/CSV.jet","function":"__take_plain","proof":"The gate at Csv.jet:1040-1042 copies buffer[start..<end] into a String. The scan at :1031 advances end only while stops[byte] == 0, and stops (a priv field, built once by reader at :1141 and never written again) is 1 for every byte from 128, so each byte of the slice is ASCII and the slice is whole UTF-8. start..<end lies inside buffer: end < start + room <= buffer.len() (:1033)."},
  {"file":"Core/Encoding/JSON.jet","function":"append_events","proof":"json.jet:707-709 match each private Dyn.Number/TypedText/Bytes discriminant before projecting its initialized String/list into the corresponding typed DataEvent. Only owned enum payloads are returned; no mismatched variant access, pointer formation or borrowed backing view escapes. Public variants are recursively handled outside the gate(:712-735)."},
  {"file":"Core/Encoding/Stream.jet","function":"typed_number","proof":"The gate at stream.jet:33-34 constructs the compiler-private Number enum from a consuming typed String; it does not reinterpret memory. Its source caller json.jet:562 is in Parser.number after numeric grammar validation and checked String construction(:561). The owned lexeme is retained for typed decoding, never used as an address or unchecked index; consumers use normal checked numeric parsing. Callers cannot corrupt enum layout or retain the moved String's storage."},
  {"file":"Core/Encoding/Stream.jet","function":"read_string","proof":"stream.jet:294 validates the exact decoded byte list, returning Syntax and releasing tracked capacity on failure(:295-296). The __bytes_into_text gate(:299-300) immediately copies that same list without intervening mutation. Escaped or raw wire bytes therefore cannot bypass UTF-8 validation or expose a dangling parser buffer."},
  {"file":"Core/Encoding/Stream.jet","function":"number","proof":"Both gates discharge separate obligations: stream.jet:360 rejects invalid JSON numeric grammar and :365 rejects invalid UTF-8 before :371 copies that exact byte list into a String. Only after those checks does :375-376 construct DataEvent.Number when typed_numbers is enabled. The valid initialized lexeme moves into its matching carrier; no unchecked numeric payload reinterpretation or raw buffer reference reaches callers."},
  {"file":"Core/Encoding/Stream.jet","function":"fold_text","proof":"stream.jet:555 consumes a valid String into __text_into_bytes solely to measure allocation capacity. That byte list is not modified; :557 can fail before reconstruction, otherwise :559 immediately copies the unchanged UTF-8 list through __bytes_into_text. Budget checks affect retention, not validity; no caller receives the intermediate storage or can mutate it between conversions."},
  {"file":"Core/Encoding/Stream.jet","function":"fold","proof":"stream.jet:566 checks the private DataEvent.Number discriminant before moving its initialized String through fold_text(:567) into matching Dyn.Number(:568). fold_text preserves UTF-8 while accounting allocation ownership. No other event variant reaches Dyn.Number; recursion and ordinary event handling occur outside the gate and no native payload borrow escapes."},
  {"file":"Core/Encoding/Stream.jet","function":"ascii_text","proof":"Despite its formatter-oriented name, ascii_text admits no unchecked caller assumption: stream.jet:1402 runs strict __utf8_validate on the exact owned list and panics on invalid UTF-8. Only the succeeding path at :1404-1405 copies it through __bytes_into_text, with no intervening write. Malformed formatter output cannot create an invalid String."},
  {"file":"Core/Encoding/Stream.jet","function":"jsonl_tree_size","proof":"The gate at stream.jet:1648-1653 runs only in the fallback branch, after every public variant was sized (:1620-1644). It tests the private Dyn.Bytes discriminant without binding its payload (`.Bytes(_)`) and only chooses the error reason and kind; the branch always returns Err (:1654-1656). No payload, pointer or backing view is read or escapes."},
  {"file":"Core/Encoding/Stream.jet","function":"write_tree","proof":"Stream.jet is the Core-internal __core_encoding_stream package, whose reserved name keeps it out of user imports. The two #Unsafe regions (Stream.jet:1722-1724, :1727-1729) build the private DataEvent.Number and DataEvent.Bytes carriers from the payload of the Dyn .Number or .Bytes just matched, which was validated when that Dyn was built, and pass it on unchanged; nothing is read from or written to raw memory."},
  {"file":"Core/Encoding/Stream.jet","function":"plain_quoted","proof":"The gate at Stream.jet:1675-1677 copies bytes[open + 1..<at] into a String. The loop at :1672-1681 reaches it only at the first quote after open, and every earlier byte passed :1679 (no backslash, 32..126), so the slice is printable ASCII and whole UTF-8; open is a checked index (:1670) and at < bytes.len()."},
  {"file":"Core/Encoding/XMLEngine.jet","function":"byte_text","proof":"__bytes_into_text at xml_engine.jet:174 receives a copy of source[start..<end]. Every local caller slices bytes of an existing String (DOCTYPE/entity/declaration helpers) or Scanner.source, whose sole construction path parse_document_with(:667-669) takes String.bytes(). Boundaries are zero/end or offsets adjacent to ASCII XML delimiters, quotes and whitespace, which cannot lie inside a UTF-8 continuation sequence. Checked list accesses(:172) prevent out-of-storage copies; the exact copied scalar-aligned allocation becomes the String with no intervening mutation or escaping borrow."},
  {"file":"Core/Encoding/XMLEngine.jet","function":"normalize_attribute_text","proof":"xml_engine.jet:2204 starts with bytes of a valid String. The in-place loop(:2207-2215) replaces only ASCII CR/LF/tab with ASCII space and removes the ASCII LF following CR; all non-ASCII bytes keep their order and membership. write never exceeds read, and :2217 truncates only the finished compacted sequence. Thus __bytes_into_text(:2219) copies valid complete UTF-8 for every caller text, not an assumed-valid wire allocation."},
  {"file":"Core/Encoding/YAML.jet","function":"yaml_leaf","proof":"yaml.jet:320 grants private carrier matching. Number/TypedText explicitly panic(:326); Bytes are read only after their matching discriminant over 0..<bytes.len()(:328-333). Object payload views are obtained only in the Object branch(:344-347), retaining the owner; arrays recurse on typed values. The caller receives an owned String and cannot request a mismatched payload or retain native storage references."},
  {"file":"Core/Service/Service.jet","function":"state_path_kind","proof":"__path_kind at service.jet:1365 accepts a borrowed valid String path and returns copied metadata kind or owned IOError. It performs a final-component pathname query, not a descriptor/identity ownership transfer; no caller pointer or reference into OS metadata is returned. A path race can alter the metadata result but cannot invalidate a caller memory borrow."},
  {"file": "Core/Sys/Sys.jet", "function": "getpgid", "proof": "A read-only POSIX query: it changes no process state, touches no memory the caller owns, and returns an Int or IOError."},
  {"file": "Core/Sys/Sys.jet", "function": "getsid", "proof": "A read-only POSIX query: it changes no process state, touches no memory the caller owns, and returns an Int or IOError."},
  {"file": "Core/Mem/Mem.jet", "function": "reserve", "proof": "A size of zero or less is an AllocError before the gate (Mem.jet:105). __region_reserve maps `bytes` of fresh private pages that no other owner can see, and a refusal answers 0, which is an AllocError too, so a Region never holds address 0. The Region alone keeps the address and length, with `mapped` set, and `close` unmaps them once at that length. A Block holds an address and a size, not a borrow: reading or writing its bytes takes an #Unsafe pointer whose caller vouches for the region."},
  {"file": "Core/Mem/Mem.jet", "function": "close", "proof": "Unmaps only a region that `reserve` mapped (`mapped` is false for `adopt`), at the address and length reserve recorded. `close` takes the Region (`&&self`), so the pages are unmapped once and no later call names them. A Block left over holds an address and a size, not a borrow; its bytes are reachable only through an #Unsafe pointer whose caller vouches that the region is still open."},
  {"file": "Core/Mem/Mem.jet", "function": "join", "proof": "The gate only consumes `next`, a #SingleUse Block, after the neighbor check (Mem.jet:177) has stopped with a panic unless `next` starts where this block ends. This block's size then grows over exactly `next`'s bytes, so those bytes keep one owner and one release. No memory is read or written."},
  {"file": "Core/Mem/Mem.jet", "function": "__allocator_handler", "proof": "A reserved `__` name: only the compiler's lowering of a `#Context(allocator: a)` block calls it, never user code. It takes the address of `allocator`, a place that block borrows for its whole extent. The closure follows that address only when the runtime calls it inside the block (JetRuntime Alloc.jet alloc_user_call), while the borrow keeps the allocator live, in place and borrowed by nothing else. Op 1 rebuilds exactly the Block that op 0 answered, at its address and size."},
  {"file": "Core/Mem/Mem.jet", "function": "release", "proof": "The method of __ContextAllocator, the stateless allocator behind `context.allocator`. `block` moves in (`&&Block`), and __context_give hands its address and size back to the allocator that served it; a user allocator's block keeps the size its `allocate` was asked for, since `resize` never changes one (its room is 0). The gate then only consumes that #SingleUse Block, so the bytes are released once and no copy of the block lives on. No memory is read or written."}
]
```
<!-- unsafe-abstractions:end -->
