# Adversarial review: Lazy precompiled Core interfaces (2026-11-15)

Target: the design note `Docs/proposals/2026-11-15-lazy-precompiled-core-interfaces.md`
as added by the patch `PrecompiledCore-4-c22-lazy-core-interfaces-note` (r2),
for Tower card #6328 c22:

> Core and Prelude ship precompiled inside the jet binary: checked interfaces
> and machine code produced when jet is built, part of the toolchain identity
> and immutable (no runtime cache, nothing to invalidate). `jet run` reads,
> scans and re-checks no Core or Prelude source: strace -c of
> `jet run hello.jet` shows no statx/openat/getdents64 on the toolchain tree,
> and the cold user time for hello is spent on the user code. One meaning on
> o0/o1/o2 (I9).

This review follows AGENTS.md "Adversarial design review". Missing context is
counted as a failed claim.

## Method and evidence base

- **Read:** the patch, its `msg`, `meta` and `tests` files, and these sources
  on the dev mirror at `1f32878176` (the note's base is `5b3425231d`):
  - `Compiler/JetCli/Source/Cli/CoreSources.jet`, `Host.jet`, `CoreLibrary.jet`;
  - `Compiler/build.jet`, `Compiler/Toolchain/PreludeSources.jet`;
  - `Compiler/JetDriver/Source/Driver/CoreReach.jet`, `Loader.jet`,
    `Pipeline.jet`, `ComptimeCompiler.jet`;
  - `Compiler/JetSema/Source/Sema/CoreSeal.jet`, `CoreDemand.jet`,
    `MarkerModules.jet`, `Registration/Items.jet`, `Registration/Derives.jet`,
    `Registration/PreludeDerives.jet`, `Units/Prelude.jet`,
    `Calls/Binder.jet`, `Calls/Checking.jet`;
  - `Compiler/JetFoundation/Source/Registry/EffectVocabulary.jet`,
    `Policy/MarkerSource.jet`, `Types/Types.jet`, `Facts/Declarations.jet`;
  - `Compiler/JetIR/Source/BuildCore.jet`, `BuildStatic.jet`;
  - `Compiler/JetRuntime/Source/CompilerHost.jet`;
  - `Docs/proposals/precompiled-libraries-2026-11-05.md` and
    `Docs/audits/2026-11-13-core-record-size-ceiling.md`.
- **Probes run (cheap, read-only on the repository):**
  - P1: a census of `Core/**/*.jet` with awk (approximate pattern counts, stated
    as such).
  - P2: the artifacts of an existing worktree build of jet (lane Speed-3,
    identity `jet@a0.0.45#df069589…`; this is **not** the gate-p149 jet the
    note measured). These are the sizes of `jet`, `jet_core.o`, `core.seal`,
    `core.exports` and `core.facts`, and the row counts by reason.
  - P3: `jet run hello.jet` with that jet, from a scratch directory outside any
    checkout. Three timed runs read user, sys and peak RSS from `getrusage`.
    One `strace -f -c` run and one path-listing `strace` run followed.
  - P4: the same run with `JET_TOOLCHAIN_ROOT` set to that worktree, counting
    the syscalls that name the tree.
- **Not done:** no build, no suite, no A/B. Every figure from P2 to P4 is from
  a different jet than p149, so it bounds behavior and does not replace the
  note's numbers.

### Probe results

| Probe | Result |
|---|---|
| P1 Core size | 272 files, 18,953,362 bytes, 228,765 lines |
| P1 functions | 759 top-level generic function declarations against 10,362 non-generic, plus 52 generic member functions; 69 generic types of 1,377; 455 `impl` blocks |
| P1 defaults | at least 103 function declarations with a default parameter expression (single-line headers only); about 895 struct fields with a default expression; 27 `#caller` lines |
| P1 spellings | about 3.13 million identifier-like tokens outside comments |
| P2 `core.facts` | 16,153 Core function keys: 9,034 `sealed`, 5,783 `lowered`, 666 `generic`, 661 `function_value`, 9 `intrinsic`, 1 `open_dispatch` |
| P2 `core.seal` | 14,609 lines, 14,374 of them stand-ins (S), 1,091,477 bytes |
| P2 sizes | `jet` 265,007,180 B; `jet_core.o` 67,086,832 B; `core.exports` 1,922,845 B; `core.facts` 1,237,325 B |
| P3 hello, outside a checkout | wall 23 to 28 ms warm (115 ms on the first, cold run); user 11 to 17 ms; sys 12 to 16 ms; peak RSS 62 to 66 MB |
| P3 strace | 171 `mmap`, 113 `statx` (49 failed), 29 `openat`, 16 `getdents64`; none names a toolchain path |
| P4 hello, `JET_TOOLCHAIN_ROOT` set | 268 `openat`/`statx`/`getdents64` calls name files under the tree's `Core/`; user 16 ms, wall 28 ms, peak RSS 65.5 MB |

## Prior art, with each system's best case

These are the strongest existing answers to "ship a checked standard-library
interface so a compile does not re-read library source". The per-claim
sections refer back to them.

- **Swift: `.swiftmodule`, `.swiftinterface` and the prebuilt module cache.**
  The binary module is LLVM bitstream with an AST block of Decls and Types
  addressed by file-unique DeclIDs and TypeIDs. An index block maps IDs and
  identifiers to offsets and lists the top-level declarations. Types are
  "serialized with enough info to regenerate them at load time", and the SIL
  block carries inlinable bodies
  ([swift/docs/Serialization.md](https://github.com/swiftlang/swift/blob/main/docs/Serialization.md),
  read for this review). Toolchains ship prebuilt binary modules for the SDK,
  compiled from `.swiftinterface` when the toolchain is built.
  - **Best case:** a client imports the standard library without parsing it.
    Declarations deserialize lazily by ID through shipped lookup tables.
    Generics are compiled separately, with witness tables and type metadata,
    so a non-`@inlinable` generic function needs **no body** at the client.
    Only `@inlinable` code is specialized there
    ([Library evolution](https://www.swift.org/blog/library-evolution/);
    S. Pestov, *Compiling Swift Generics*, swift.org documentation; these
    sources are named here and were not fetched again for this review).
  - **What it does that this design does not:** it ships a lookup index; it
    can call a generic without its body; and it admits that load decodes
    types.
- **Rust: `.rlib` and `.rmeta`.** The metadata holds every item's signature,
  type, trait impls and spans, plus the source map. It holds MIR only for
  generic, `#[inline]` and const functions. Every other function ships as
  object code
  ([rustc dev guide, "Libraries and metadata"](https://rustc-dev-guide.rust-lang.org/backend/libs-and-metadata.html),
  read for this review). `rustc` never parses a dependency's source, and the
  loader refuses metadata from another compiler build.
  - **Best case:** a generic standard-library function is instantiated from
    shipped checked MIR, not from text. Debug builds can also reuse
    standard-library instantiations (share-generics).
  - **What it does that this design does not:** it ships checked generic and
    inline bodies on day one, which is this design's step B, and it puts no
    speed gate on them.
- **Go: export data, unified IR and GC-shape stenciling.** Export data holds
  exported declarations plus inlinable and generic bodies, indexed so an
  importer reads only what it uses. Generics run as one instantiation per GC
  shape, with a dictionary
  ([Go 1.18 generics implementation](https://github.com/golang/proposal/blob/master/design/generics-implementation-dictionaries-go1.18.md),
  read for this review;
  [cmd/compile README](https://go.dev/src/cmd/compile/README)).
  - **Best case:** lazy per-entry import with no source parse. Generic
    standard-library code needs no source.
  - **What it does that this design does not:** it reaches generics without
    text, and it puts an index in the format.
- **OCaml: `.cmi` and `.cmx`.** The `.cmi` holds the types of a module's
  signature, and digests catch mismatches. The `.cmx` holds cross-module
  inlining facts, and `-opaque` turns them off (OCaml manual, "Batch
  compilation"; named, not fetched again). OCaml compiles a polymorphic
  function once, over a uniform value representation.
  - **Best case:** a tiny interface, and no client ever needs a library body
    for generics.
  - **What it does that this design does not:** it compiles polymorphism once,
    with no per-type instantiation.
- **C++20 modules and BMIs.** A compiled module interface stores checked
  declarations and template definitions for instantiation. Clang loads
  declarations from it lazily through on-disk hash tables (Clang, "Standard
  C++ Modules",
  [clang.llvm.org](https://clang.llvm.org/docs/StandardCPlusPlusModules.html);
  named, not fetched again). Its known pain is that a BMI is valid only for
  one compiler, version and flag set.
  - **Best case:** templates instantiate from a checked tree, with no
    reparse.
  - **What it does that this design does not:** templates come from a binary
    tree, not text.
- **Zig: whole-program lazy analysis from source.** No standard-library
  metadata ships. Only reached declarations are analyzed, and the standard
  library compiles from source in every build (as recorded in
  `Docs/audits/2026-11-13-core-record-size-ceiling.md`). Zig gets its speed
  from laziness plus a persistent compilation cache. That cache is the very
  thing c22 forbids.
  - **Best case:** one meaning by construction, since there is one path.
    Unused code costs nothing.
  - **Lesson for this design:** any path that keeps text, as step A's bodies
    do, needs Zig's cache to be fast, and c22 denies it that cache.

Two of these (Swift and OCaml) need no library body at all for an unspecialized
generic call. Go, Rust and C++ ship the bodies in checked binary form. **None
of the six parses library text to instantiate a generic in a normal client
compile**, which is what step A does.

## Claims and verdicts

| # | Claim (note section) | Verdict |
|---|---|---|
| C0 | Steps A then B deliver c22 (whole note) | fails |
| C1 | The measured baseline against c22 (section 1) | holds only within stated limits |
| C2 | Step A: no Core or Prelude text is lexed, scanned or parsed, and hello reads none (3.3) | fails |
| C3 | The interface is flat rows in `Int` arrays, embedded "the way `JET_CLI_CORE_SOURCES` is", used in place, nothing to decode, loaded per declaration (3.2) | fails |
| C4 | The size is under 6 MB (3.2) | fails |
| C5 | The hello budget (at most 1 ms of CPU, measured by a trace span on every run) and the never-slower rule (3.4) | fails as a gate definition |
| C6 | The list of what step A leaves is complete, and c22 holds after A for sealed, non-generic reach (3.5) | fails |
| C7 | Identity and invalidation: one check writes the interface, seal and code; immutable; A1 guards drift (3.1, 7) | holds only within stated limits |
| C8 | One load path and one meaning; only a toolchain tree pays to produce its interface (3.3, A3) | fails |
| C9 | I9: one meaning on O0, O1 and O2 | holds only within stated limits (unproven) |
| C10 | Memory: the interface's resident-memory and size cost is acceptable | fails (missing context) |
| C11 | Step B's gate, "lands only if it measures faster", is well defined (4, 6) | fails |
| C12 | A reach miss gives a missing-name error, never a wrong meaning; the corpus, the cases and `jet check Core` catch misses (7) | fails |
| C13 | The rejected alternatives are the strongest ones (5) | holds only within stated limits |

---

### C0. Steps A then B deliver c22

**Claim.** Step A ships the interface, step B covers the bodies a compile
still needs, and c22 then holds.

**Prior art.** Rust and Go ship checked generic and inline bodies together
with the interface from the start. Swift and OCaml need no generic bodies at
all. None of them leaves conformance to a later, speed-gated step.

**Attack.**

- c22 has four clauses:
  1. no toolchain reads (strace);
  2. no scan;
  3. no re-check;
  4. hello's cold user time is spent on user code.

  The note addresses the first three. It sets the fourth aside: "The rest is
  fixed compiler work on hello and the compiler-owned rows. That fixed cost is
  outside this note."
  - By the note's own split, hello.jet's parse is 0.3 ms of 29 ms user time.
    After step A removes 1.2 ms, user code is still a few percent of hello's
    time. Clause 4 stays failed, and the note does not say which card or step
    closes it.
- The note itself says step A leaves c22 failing for Summarize and
  RunColdContext. Step B is "gated on faster" (C11), so the design admits a
  final state where c22 never holds for any program that reaches generic,
  lowered or unsealed Core.
  - Main's 2026-11-15 ruling makes text parsing a c22 violation. A
    performance gate cannot then license staying on text. The gate inverts
    the priority.
- The "re-checks" clause: CoreDemand checks some bodies up front, whatever the
  seal says. These are trait-impl methods, compiler-generated bodies and
  `close` methods (`CoreDemand.jet` lines 12 to 24). Under step A they come
  from text (C6).

**Verdict: fails.**

---

### C1. The measured baseline (section 1)

**Claim.** Reads already hold outside a checkout. Scans fail. Re-checks hold
for hello only. The hello CPU split shows 1.2 ms of Core text parsing in 29 ms
of user time.

**Attack and evidence.**

- **Reads, confirmed for this jet within the limit.** P3 found no syscall
  naming a toolchain path outside a checkout.
  - With a toolchain tree (P4), hello makes 268 syscalls on the tree's
    `Core/`. That is `jet_cli_core_tree_matches`, which compares the tree's
    Core with the built-in Core byte for byte, "about 20 MB"
    (`CoreSources.jet` lines 120 to 142).
  - So c22's strace clause holds only for programs outside a checkout. That
    includes no compile run from inside the repository, such as goldens or
    a contributor's `jet run`. The note states "outside any checkout" but
    never says whether c22 is meant to hold inside one.
- **The split does not add up.** The six rows sum to 15.8 ms of the stated
  29 ms of user time. 13.2 ms (45%) is unattributed: process start, mapping
  parts, thread pool, runtime image bind, and the like. The 1 ms budget is set
  as a share of a total whose largest part is unexplained.
- **The baseline is inconsistent.** hello's `jet run` median wall time is
  25 ms, yet its user time is 29 ms, and the note says hello is
  single-threaded. User time above wall time means either more than one busy
  thread or numbers taken from different runs, such as a traced run with
  tracing overhead.
  - P3, on another jet, shows user 11 to 17 ms and sys 12 to 16 ms. Run-to-run
    noise (about ±3 ms) is larger than the whole 1 ms budget.
- **An unidentified file.** The split names "`Core/Prelude/Prelude.jet`
  (11 KB) and `core.jet`". No `core.jet` exists under `Core/`, and no driver
  source names one. This is missing context.

**Verdict: holds only within stated limits.** These limits are outside a
checkout, numbers that do not reconcile, and an unidentified input.

**Gaps:** G1, G2, G3.

---

### C2. Step A parses no Core or Prelude text, and hello reads none

**Claim (3.3).** "No Core text is lexed, scanned or parsed." "The Prelude goes
through the same interface. hello then reads no Core or Prelude text at all."

**Prior art.** Rust, Go, Swift and C++ load the whole standard-library
surface from binary metadata, and no client compile parses library text.

**Attack and evidence.** Several paths parse or scan Core or Prelude text on
every compile, or on common compiles, and the note mentions none of them.

1. **Core text in the effect vocabulary, on every compile.**
   - `EFFECT_CANONICAL_SOURCE` is the concatenated text of
     `Compiler/Prelude/Effects.jet`, `Core/Effects/Effects.jet` and
     `Core/Compiler/Meta.jet` (`EffectVocabulary.jet` line 41).
   - `Pipeline.jet` line 4942 calls `jet_driver_canonical_effect_module`, which
     runs `parse_source(source.bytes())` (line 3599) on every compile. The
     compiler-query and script paths do the same (`CompilerQueries.jet` 356,
     `ScriptHelpers.jet` 258).
   - So two Core files are parsed in every compile, hello included.
2. **Compiler-owned Prelude text, parsed or scanned per compile.**
   - `MARKER_SOURCE`, the text of `Compiler/Prelude/Markers.jet`, 16 KB, is
     parsed by `parse_generated_source` in `MarkerModules.jet` (lines 482,
     718 and 731).
   - `marker_source_read` re-scans the same text on every
     `marker_registered*` query (`MarkerSource.jet` lines 371 to 455).
   - `SEMA_PRELUDE_DERIVE_SOURCE` is word-scanned in
     `sema_registration_bind_namespaces` on every registration
     (`Items.jet` lines 5434 to 5436). `Derives.jet` line 2080 parses it for
     derive providers.
   - `SEMA_PRELUDE_ERROR_SOURCE` is parsed where a module needs the error
     family (`Items.jet` line 2152).
   - `SEMA_PRELUDE_UNITS_SOURCE`, 36 KB, is parsed once a source mentions a
     unit (`Units/Prelude.jet` line 13).
   - c22 says "no Core **or Prelude** source". The note's "compiler-owned
     Prelude rows" sit inside hello's 6.2 ms register span, and the note puts
     them outside its scope.
3. **Comptime and compiler-query compiles carry Core text.**
   The comptime compiler-query context (`JetDriverComptimeCompilerContext`)
   and the requests built from it copy `core_sources`
   (`ComptimeCompiler.jet` lines 66, 138 and 272). The note's A2 names "reach
   and registration" for the main compile only. It does not say that the
   nested compiles move to the interface.
4. **Default expressions are syntax.**
   - A call that omits a defaulted argument gets the declaration's default
     expression, rewritten into the call and checked in the owner's scope
     (`Calls/Binder.jet` lines 297 to 330; `Calls/Checking.jet` lines 172 and
     1201, "register the declaration's original default expression").
   - Core has at least 103 function declarations with default parameters and
     about 895 defaulted struct fields (P1). The interface row lists
     "parameter types and access marks" and "fields". It names no form for
     default expressions, and section 2 rules out any syntax tree.
   - So a user call `verify_jwt(token, key: k, audience: a)` needs
     `issuer: String?{None}` and `clock_skew: Duration{0ms}` from somewhere.
     Today that source is the parsed text.
5. **Bodies from text.** Section 3.5 says this outright. It is the largest
   case, covered in C6.

**Verdict: fails.** Hello itself still parses two Core files and the
compiler-owned Prelude texts under step A as written.

**Gaps:** G4, G5, G6, G7.

---

### C3. Flat `Int` rows embedded like `JET_CLI_CORE_SOURCES`, used in place, nothing to decode

**Claim (3.2).** The interface is fixed-width rows in `Int` arrays plus a
string table, "embedded in jet as constant data, the way
`JET_CLI_CORE_SOURCES` is today". Loading copies fields "with nothing to
decode". A type loads when a loaded row first names it.

**Prior art.** Swift and Clang both ship an **index** (on-disk hash tables
from name to ID to offset) inside the module file, and Swift states that
types are regenerated at load. Lean `.olean` and Java CDS map an object graph
in place. The note rejects those in section 5.

**Attack and evidence.**

- **The build order rules out "the way `JET_CLI_CORE_SOURCES` is".**
  - `JET_CLI_CORE_SOURCES` is a `#run` constant compiled into **jet-bare**
    (`CoreSources.jet` line 25), and `BuildStatic.jet` lays it out as frozen
    static data.
  - The interface is written by "the step that checks Core for `jet_core.o`
    and `core.seal`". That is the `runtime` step, which **jet-bare itself
    runs** after jet-bare is built (`Compiler/build.jet` lines 72 to 90).
  - So the interface cannot be a `#run` constant of jet-bare without a second
    compile of the whole compiler. The note does not mention that cost.
  - The other route is a `core-embed` image part, like `core.seal`, read
    through `jet_cli_runtime_part`. That returns a lent `[U8]`
    (`CompilerHost.jet` lines 279 to 285, "lent where the kernel mapped them,
    never copied").
  - A byte part is not an `Int` array. Every field read then assembles a word
    from bytes, which is per-field decoding, the cost section 2 swears off.
    The note must pick one route and cost it.
- **"Nothing to decode" is false for types.**
  - Sema's `Type` is a recursive enum holding `String` names, lists, optional
    sub-records and spans (`Types.jet` lines 6 to 41). `TTypeDef` and
    `TDeclarationParam` hold `String`s, lists and `Span`s
    (`Declarations.jet` lines 200 to 250).
  - Turning an interned type index into a `Type` means allocating and building
    the tree. That is decoding, just as Swift states for its own format.
  - The 1 ms budget has to pay for it. The note neither names nor measures
    that cost.
- **No index is specified.**
  - Reach starts from user spellings and must find declarations by name.
  - The record-size ceiling the note borrows its size from explicitly
    excludes "lookup indexes (hash slots) … rebuilt at load"
    (`2026-11-13-core-record-size-ceiling.md`, table row "Lookup indexes").
  - Shipping the index costs size, which C4 leaves out. Building it at load
    costs time in proportion to all of Core (19,643 functions and methods,
    35,250 identifiers), against a 1 ms budget and against "an unreached
    declaration costs nothing".
- **Spans.** The row keeps "source path and line for diagnostics". Today's
  rows carry byte `Span`s. These serve diagnostics with columns, and
  `FunctionDefaultRef` resolves defaults by source position. Lines alone break
  the A1 round trip ("rows loaded equal rows built from source") or change
  diagnostics. Full spans enlarge every row.

**Verdict: fails.** The layout cannot be built the way the note says, the
decode cost it denies exists, and the index is unspecified.

**Gaps:** G8, G9, G10.

---

### C4. Under 6 MB

**Claim (3.2).** "Under 6 MB, which is the 14.2 MB ceiling minus its 8.01 MB of
bodies."

**Attack and evidence.**

- **The arithmetic is wrong.** 14.2 minus 8.01 is 6.19 MB, which is not under
  6 MB.
- **It is the wrong ceiling to subtract from.** The 14.2 MB figure is a
  hand-minimal **variable-length** encoding: 10 B per function, 4 B per
  parameter, identifiers front-coded in blocks of 16, delta varints. The note
  chooses fixed-width rows of `Int`, which are 8-byte words. A row of 8-byte
  words for every field can be several times the varint ceiling.
- **Parts the ceiling never held:**
  - **Reach spellings** for every declaration, bodies included. Reach is
    transitive through bodies, so the interface needs each body's words.
    Core has about 3.13 million identifier-like tokens (P1). Even at a
    fivefold reduction from deduplication per declaration, 8-byte ranges run
    to megabytes. [INFERENCE: this is an order-of-magnitude estimate, not a
    measurement.]
  - **The lookup index** (C3).
  - **Seal facts and symbol binding.** `core.seal` alone is 1.09 MB of text
    (P2).
  - **Constant values.** The note's interface stores "the value". The ceiling
    budgets 2.94 MB for constants as raw bytes, but no fixed-width row
    represents a nested list, map or struct value.
- **Binary growth.** Core text must stay embedded:
  - for step A's bodies from text;
  - for `jet_cli_core_tree_matches`;
  - for diagnostics snippets.

  So the interface adds to `jet`; it does not replace text. `jet` is already
  265 MB, with `jet_core.o` at 67 MB (P2). The note sets no limit on binary
  size.

**Verdict: fails.** A1 "records the size" but gates nothing on it.

**Gaps:** G11, G12.

---

### C5. The hello budget and the never-slower rule

**Claim (3.4).** Interface loading for hello costs at most 1 ms of CPU, under
4% of 29 ms, measured by a trace span and counters "so the budget is measured
on every run". Every other program is never slower in wall time and peak RSS.

**Attack and evidence.**

- **A span cannot hold lazy loading.**
  - Loading is "per declaration" and "a type loads when a loaded row first
    names it", so loads happen inside reach, registration and sema, wherever
    a row is first touched. One span covers this work only if every load site
    is wrapped.
  - Per-load timing on Linux reads the thread CPU clock, which is a real
    syscall and is not served by the vDSO. Over thousands of loads
    (RunColdContext loads 7,880 items), the timer becomes a measurable share
    of what it measures. [INFERENCE: syscall cost from general Linux
    behavior; not probed here.]
- **The boundary between loading and registration is undefined.** "Loading
  copies its row's fields into sema's registration rows". If that copy is
  billed to the register span, the load span can meet 1 ms while register
  grows. The budget has to cover total Core-attributable CPU, or at least
  state that no other hello span may grow.
- **The base is unstable.**
  - "Under 4% of 29 ms" uses a user-time total taken from a traced run that
    does not match the wall median (C1).
  - On another jet, P3 shows hello's user time at 11 to 17 ms. A fixed 1 ms
    there is 6 to 9%, so the "4%" framing does not travel across builds.
- **System time and page faults are excluded.** First touches of a mapped part
  are page faults. They count as sys time, or as major faults on a truly cold
  page cache. P3 shows hello's sys time (12 to 16 ms) about equal to its user
  time. The budget says "CPU" without saying user or user plus sys, and
  "cold" without saying whether the page cache is dropped. The note's
  "median of 5 cold runs" are warm-cache runs.
- **hello has no RSS budget.** Only "every other program" carries peak RSS.
- **The comparison baseline for others is a forbidden path.** "Never slower
  than the built-in-source compile it replaces" is right for a migration
  check. It cannot serve as an acceptance gate for c22, because the baseline
  violates c22.

**Verdict: fails as a gate definition.** The intent is sound. The measurement
as specified cannot certify 1 ms.

**Gaps:** G13, G14.

---

### C6. What step A leaves, and c22 after step A

**Claim (3.5).** Only these need a Core body after step A:
- generic functions instantiated by user code;
- unsealed bodies (view provenance, an unclean failure union);
- bodies `jet build` inlines at O1 or O2;
- comptime calls without shipped code.

c22 then holds for every program whose reached Core is sealed and
non-generic.

**Prior art.** Rust and Go ship exactly these categories (generic and inline
bodies) in checked binary form. Swift and OCaml need none for unspecialized
generics.

**Attack and evidence.** The list is incomplete. From code and from P2's
`core.facts` (16,153 keys), these also need a body or text:

| Missing category | Evidence | Scale (P2 build) |
|---|---|---|
| Functions of `JET_CLI_CORE_LOWERED` packages, never shipped as code, so "a program that calls one lowers it itself", from its body | `CoreLibrary.jet` lines 25 to 36; `core.facts` reason `lowered` | 5,783 keys (36%). The 35 packages include `collections`, `regex`, `sync`, `tasks`, `log`, `data`, `units` |
| Function values: a key taken as a value is checked in full, never stood in | `CoreDemand.jet` lines 32 to 35; reason `function_value` | 661 keys |
| Trait-impl methods, checked up front because generic dispatch reaches every implementation | `CoreDemand.jet` lines 12 to 15 | 455 `impl` blocks in Core (P1) |
| Compiler-generated bodies (derive and codec) and `close` methods, checked up front | `CoreDemand.jet` lines 14 to 16 | not counted |
| A shared global's initializer, error conversions, template items | `CoreDemand.jet` lines 33 to 35 | not counted |
| Functions holding trait values (`ir_core_open`): built but not exported, so a program builds them itself | `BuildCore.jet` lines 23 to 26; reason `open_dispatch` | 1 key, plus whatever reaches it |
| Lowering roots that "lowering keeps by name or by owner type", checked up front on a lowering compile, and `jet run` lowers | `CoreDemand.jet` lines 17 to 19 | not counted |
| Default parameter and field expressions | C2 item 4 | at least 103 functions, about 895 fields |
| `jet build` (and `-O1`) builds **every** Core function it calls from bodies, not only inlined ones. Only a run at `LINK_RUN_BASE` binds to Core code | `Host.jet` lines 956 to 964 ("Any other run builds every Core function it calls"), line 1635 | every reached Core function on `jet build` |

- Generic keys (666) plus lowered (5,783) plus function values (661) come to
  44% of Core's function keys that a reaching compile still needs from text
  under step A. More come from the up-front categories.
  - The note's statement "c22 holds after step A for every program whose
    reached Core is sealed and non-generic" is true as a tautology. But the
    note names only hello as such a program, and gives no census of how many
    corpus programs qualify.
  - The note's own Summarize still checks 212 Core bodies on `jet run`.
- The Prelude's `min`, `max` and `clamp` are generic
  (`Core/Prelude/Prelude.jet` lines 47 to 63). Any program that calls one
  falls outside step A's c22 set.
- `core bodies from text` is promised as a trace counter. Until it reports
  over the corpus, "the remaining gap is measured, not assumed" is a promise,
  not a fact.

**Verdict: fails.** The leftover list is incomplete, the qualifying set is
unmeasured, and on the note's own data c22 fails for every program larger than
hello.

**Gaps:** G15, G16, G17.

---

### C7. Identity and invalidation

**Claim.** The interface is written from the same check that writes
`jet_core.o` and `core.seal`, in the same build. It is immutable, with no
runtime cache. Two builds give identical bytes. The A1 round trip fails the
build's tests on drift.

**Prior art.** Rust's SVH and compiler-version check refuse foreign metadata.
OCaml checks digests. Swift requires an exact format version. All of them
identify the interface by hash or version.

**Attack and evidence.**

- **Identity holds transitively only.**
  - jet's `IDENTITY` is the identity of **jet-bare**, settled before the
    runtime parts are added (`build.jet` lines 8 to 17; `Host.jet` lines 698
    to 707).
  - The interface would be a part added after the identity is fixed. It is
    covered only because it is a deterministic function of jet-bare plus the
    Core text embedded in jet-bare.
  - That makes A1's "two builds give identical bytes" load-bearing for
    identity, not just for hygiene. The interface's digest is not recorded in
    `provenance.json` or checked by the release gate, as the 2026-11-05 note
    proposed for the pack.
- **"Nothing to invalidate" fails in a tree.** `jet_cli_core_tree_matches` is
  an invalidation check that runs per compile inside a checkout, reading
  about 20 MB (P4: 268 syscalls). The note keeps it and does not count it.
- **The A1 guard compares the writer with itself.** The interface is written
  "from the registered and checked tables". A round trip of write then read
  equals those tables by construction, so it proves the codec, not the facts.
  - Drift between the interface and what a body check would conclude is what
    A4's corpus parity checks. That covers `jet check`, not the stand-in
    semantics of `jet run` against `jet build` (C9).
  - After A3, "the source registration path for Core goes away". The A1
    comparator then survives only inside the writer, which needs to be stated.

**Verdict: holds only within stated limits.** These are: built-in Core,
outside a checkout, and a deterministic writer.

**Gaps:** G18, G19.

---

### C8. One load path and one meaning; only the tree pays

**Claim (3.3, A3).** A toolchain tree produces its interface in memory with
the same writer and loads it through the same reader. "Only the tree pays to
produce it." The tree-mode cost "is recorded; it is not gated."

**Prior art.** The 2026-11-05 note (section 5, "Working on Core itself")
chose the opposite. A changed Core stops the compile with an error naming the
rebuild step, and the compiler "never compiles Core from source as a
fallback". Swift and C++ rebuild a module once into a cache, which c22 forbids
here.

**Attack and evidence.**

- **The cost is a full Core check per compile.** The writer works "from the
  registered and checked tables" of all of Core. In a tree whose Core
  differs, every compile, hello included, would register and check every
  Core declaration and body before compiling the user file. No cache is
  allowed.
  - Today a differing tree is lazy: it "compiles every Core function a
    program calls" (`CoreSources.jet` lines 64 and 65), through reach.
  - A3 "removes the source registration path for Core". That replaces lazy
    per-reach work with whole-library work.
  - The scale: the Core check sits inside the `runtime` build step, which
    has a 30-minute timeout. RunColdContext's 2,123 checked bodies already
    take 1.3 s, against 16,153 function keys in Core.
    [INFERENCE: a full check per compile is likely several seconds and
    gigabytes.]
- **The case of a matching tree is unspecified.** The note does not say
  whether a tree whose Core matches the built-in Core loads the built-in
  interface, as the binding does today. If it does not, every goldens run
  from inside the repository pays the tree cost.
- **"Not gated" contradicts c37's never-slower rule.** Tree mode is how
  contributors and agent lanes work on Core every day.

**Verdict: fails.** One path, yes, but at a cost the note leaves unmeasured
and ungated. It also contradicts the earlier ratified-direction note without
discussing it.

**Gaps:** G20, G21.

---

### C9. I9: one meaning on O0, O1 and O2

**Claim.** This is the card's clause. The note's support is implicit: one
check writes the interface, the seal and the code, and there is "one load
path and one meaning".

**Prior art.** Rust ships `std` built at one optimization level and
instantiates generics at the client's level. Swift's resilience keeps
semantics independent of inlining. Both rely on a single checker.

**Attack and evidence.**

- **The interface itself is tier-neutral.** It holds front-end facts with no
  dependence on level. That part holds.
- **The tiers diverge in what they trust.**
  - `jet run` (O0) takes seal stand-ins and binds to `jet_core.o` code built
    **after the O2 optimizer** (`CoreLibrary.jet` lines 4 to 7).
  - `jet build` (O2) and `-O1` check reached Core bodies (from text, under
    step A) and compile them at their own level (`Host.jet` lines 956 to
    964).
  - So a seal fact that disagrees with a body check would change diagnostics
    or effect rows between `run` and `build`. That would be an I9 violation
    in diagnostics.
- **The proofs never compare tiers.** Section 6's parity is "same output and
  diagnostics … before and after" each step. A4's corpus proof compares
  `jet check` with stand-ins against `jet check` without. No proof compares
  `run` (O0, stand-ins, O2 image code) against `build -O1` and `build -O2`
  (bodies) under the interface path.
- **Comptime is unaddressed.** I9 includes compile-time code at O0. The
  nested comptime and compiler-query compiles still carry `core_sources` (C2
  item 3). The note does not say they move to the interface, so comptime
  could resolve Core declarations through a different path than the main
  compile.

**Verdict: holds only within stated limits.** The interface adds no new tier
split, but the note neither proves parity across tiers nor covers the comptime
path.

**Gaps:** G22, G23.

---

### C10. Memory (RSS) and size cost of the interface

**Claim.** This is implicit. Only per-program peak RSS is held to never-slower.

**Attack and evidence.**

- **What is resident is not stated.** A mapped part costs RSS only for touched
  pages. Reach that scans spellings, an index built at load (C3), or type
  materialization can touch or allocate far more than the "rows loaded"
  counter suggests. The note promises "bytes touched" as a counter but sets
  no limit on it.
- **Baseline.** hello's peak RSS is 62 to 66 MB on the P3 jet. The 2026-11-05
  note expected a Core-using compile under 100 MiB. No such target appears
  here.
- **Install and page cache.** The interface adds megabytes to a 265 MB binary
  that must also keep 18.9 MB of Core text (C4). The note gives neither a
  limit nor a measurement.

**Verdict: fails (missing context).**

**Gaps:** G12, G24.

---

### C11. Step B's gate, "lands only if it measures faster"

**Claim (4 and 6).** Each part of step B lands only if it is faster than
parsing and checking the same bodies from text, proven on Summarize and
RunColdContext, with `core bodies from text` falling.

**Prior art.** Rust and Go treat shipped checked bodies as part of the format,
not as a speed experiment. Their loaders were made fast because the format was
required.

**Attack.**

- **The baseline is forbidden.** Per Main's ruling, the text path violates
  c22. Gating the conforming design on beating a non-conforming one means a
  slow loader leaves c22 failed indefinitely, with no stated fallback. The
  gate should rank designs that conform, not decide whether to conform.
- **The metric is undefined.** The gate does not say:
  - wall or CPU;
  - `check`, `run`, or both;
  - whether "faster" on one program and slower on another counts;
  - what noise threshold applies, though section 1 notes a 10 ms trace clock
    on Summarize;
  - whether RSS takes part (c37 says never slower in wall **and** RSS, and
    B's text says only "faster").
- **The comparison set is narrower than section 6.** Section 4 names
  Summarize and RunColdContext. Section 6 names six programs, and hello's
  budget is separate.
- **The machine-code part is not comparable.** "Common generic instances such
  as `List<Int>`" removes bodies at the price of binary size. "Faster than the
  text path" does not price size.
- **No success state.** Nothing says when step B is done, for example
  `core bodies from text` reaching 0 on the corpus.

**Verdict: fails.**

**Gaps:** G25, G26.

---

### C12. Reach precision

**Claim (7).** A declaration reach misses gives a missing-name error, not a
wrong meaning. The corpus, the cases and `jet check Core` (which reaches
everything) catch it.

**Attack and evidence.**

- **`jet check Core` cannot catch a reach miss.** It reaches everything by
  definition, so it never exercises reach's selectivity.
- **A miss can change meaning.** Registration derives `auto_printable` and
  auto derives from a type's **declared trait impls**
  (`Registration/Checked.jet` lines 3035 to 3046). If interface reach missed
  an explicit Core `impl Printable`, the type could fall back to an
  auto-derived rendering. That is different output, not a missing name.
  [INFERENCE: this failure mode is plausible from the code, not
  demonstrated.]
  - Missed error conversions (`CoreDemand.jet`) could likewise change a
    failure union instead of erroring.
- **The spellings are re-derived.** Today's spellings come from the lexer over
  text (`jet_driver_unit_spellings`). The interface re-derives them from
  checked tables. Any divergence is a new class of reach bug, and only a
  differential test against the text reach, per program, would find it.

**Verdict: fails.** The claim is absolute and unproven, and the named guard
cannot detect the failure.

**Gaps:** G27.

---

### C13. The rejected alternatives

**Claim (5).**
- Per-declaration parsing is forbidden.
- A revived record is slow to decode.
- A mapped object graph needs position-independent ids.
- A per-program cache is forbidden.

**Attack.**

- The section omits the alternative that removes the hardest part of step A,
  **separately compiled generics**: Swift witness tables, OCaml's uniform
  representation, Go dictionaries. With those, a generic Core function ships
  once as machine code and needs no body at the client. This would change
  language-level performance, so it may be an owner choice. It is still the
  strongest prior art against C6 and deserves a reasoned rejection.
- It omits **shipping checked generic bodies in step A**, as Rust MIR and Go
  unified IR do, rather than in a gated step B.
- It omits the 2026-11-05 note's tree rule, refusing an edited tree, as the
  alternative to A3 (C8).

**Verdict: holds only within stated limits.** The rejections given are sound.
The strongest alternatives are missing.

**Gaps:** G28.

---

## Gaps as proposed criterion text

Each gap is written as a criterion a card or ballot can carry.

- **G1 (c22 scope inside a checkout).** "c22's strace clause is stated for
  both cases: (a) entry outside any checkout; (b) entry inside the toolchain
  tree whose Core matches the built-in Core. In (b), `strace -f` of
  `jet run hello.jet` shows no `openat`, `statx` or `getdents64` on the
  tree's `Core/` or `Compiler/Prelude/`, or the card records an owner ruling
  that (b) is exempt."
- **G2 (hello split reconciles).** "The hello CPU table attributes at least
  95% of the process's user plus sys time to named spans, including process
  start and part mapping. User and wall time come from the same set of
  untraced runs, and traced numbers are labeled as traced."
- **G3 (unidentified input).** "The note names the source of every Core or
  Prelude text a hello compile parses today by repository path. `core.jet` is
  identified or removed."
- **G4 (effect vocabulary).** "A compile on built-in Core does not call
  `parse_source` on `EFFECT_CANONICAL_SOURCE`. The canonical effect module's
  rows come from the prebuilt interface, and the trace shows 0 Core bytes
  parsed for hello."
- **G5 (compiler-owned Prelude).** "A compile on built-in Core lexes, parses
  and word-scans none of `MARKER_SOURCE`, `SEMA_PRELUDE_DERIVE_SOURCE`,
  `SEMA_PRELUDE_ERROR_SOURCE` and `SEMA_PRELUDE_UNITS_SOURCE`. Their rules,
  providers and catalogs come from the prebuilt interface. A trace counter
  `prelude.text_bytes_scanned` is 0 for hello and for every corpus program."
- **G6 (nested compiles).** "Comptime, compiler-query and script compiles on
  built-in Core take Core declarations from the same interface reader and
  carry no Core source text in their requests."
- **G7 (default expressions).** "The interface carries every Core default
  parameter and default field expression in checked, non-text form, with the
  owner scope needed to check it at a call site. A corpus case calling a
  defaulted Core function and building a defaulted Core struct passes with
  `core bodies from text` = 0."
- **G8 (embedding route).** "The note names how the interface reaches `jet`
  (a `#run` constant, at the cost of a second compiler compile, or a
  `core-embed` part read as bytes) and states the per-field read cost of that
  route. A1 measures it."
- **G9 (decode cost).** "The load cost includes building sema `Type` values,
  `String`s and `Span`s from interned indexes. The hello budget (G13) covers
  it."
- **G10 (lookup index and spans).** "The interface specifies its name lookup
  index (shipped, or built at load with its cost measured) and keeps full byte
  spans. Diagnostics that point into Core are byte-identical to today's on the
  ui corpus."
- **G11 (size estimate).** "The size estimate is recomputed for fixed-width
  rows and includes reach spellings, the lookup index, seal facts, symbol
  binding and constant values. A1 gates on a stated byte limit, not just a
  recording."
- **G12 (binary size).** "The note states the growth of `jet` from the
  interface, with Core text still embedded, and a limit on it."
- **G13 (hello budget measured soundly).** "For hello on built-in Core: (a)
  the CPU of all Core- and Prelude-attributable work, interface loading and
  materialization included, is at most 1 ms of thread CPU (user plus sys);
  (b) no other hello span grows; (c) process user plus sys time, median of 11
  untraced runs, does not rise; (d) peak RSS does not rise. The budget is
  checked with load timing that does not inflate the measured span by more
  than 10%."
- **G14 (cold means cold).** "Each timing table says whether runs are
  page-cache warm. One hello run with the page cache dropped is reported for
  major faults and sys time."
- **G15 (complete leftover list).** "Section 3.5 lists every category that
  needs a Core body or Core text after step A: generic, `lowered` packages,
  function values, trait-impl methods, generated derive or codec bodies,
  `close` methods, shared-global initializers, error conversions, template
  items, `open_dispatch`, lowering roots, defaults, and `jet build`/`-O1`
  reached bodies. Each has a count from `core.facts` or the trace."
- **G16 (qualifying census).** "Before A2 lands, a census over the example
  and gauntlet corpus reports how many programs' `jet run` would have
  `core bodies from text` = 0 under step A, and which category blocks each of
  the rest."
- **G17 (c22 hello clause owner).** "c22's clause 'cold user time for hello
  is spent on the user code' is assigned to a named card and step with a
  numeric target, for example the share of hello user time in user-code
  spans, or the note records an owner reinterpretation of the clause."
- **G18 (provenance).** "The interface's digest is recorded in
  `provenance.json` beside the runtime key, and the release gate checks it."
- **G19 (drift guard is not circular).** "Besides the A1 codec round trip, a
  guard compares interface-loaded registration rows with rows registered from
  Core source by the text path, run in the build's tests while the text path
  exists in the writer."
- **G20 (tree mode cost).** "A compile in a toolchain tree whose Core differs
  from the built-in Core costs no more than today's lazy tree compile, in wall
  time and RSS, on hello and Summarize. Otherwise the note adopts the
  2026-11-05 rule (stop with an error naming the rebuild step) by owner
  ballot."
- **G21 (matching tree).** "A tree whose Core matches the built-in Core loads
  the built-in interface. The match check's cost is measured and kept under a
  stated limit."
- **G22 (I9 cross-tier parity).** "Every golden and ui row gives identical
  output and diagnostics under `jet run` (O0, stand-ins, image code),
  `jet build -O1` and `jet build -O2`, with the interface path on."
- **G23 (comptime I9).** "Compile-time code resolves Core through the same
  interface and the same seal facts as the main compile. A corpus case
  calling the same Core function at comptime and at run time prints identical
  results at O0, O1 and O2."
- **G24 (RSS target).** "The note sets a peak-RSS target for hello and for a
  `core.files` program on built-in Core, and the trace counter
  `interface.bytes_touched` has a stated limit for hello."
- **G25 (step B is required, not optional).** "c22 is met only when
  `core bodies from text` = 0 for the c22 program set. Step B's speed gate
  chooses between conforming designs and cannot leave c22 unmet. If no part
  of B beats the text path, the note names the fallback that still meets
  c22."
- **G26 (step B metric).** "Step B's 'faster' means: median of N untraced
  runs, wall and user plus sys, `jet check` and `jet run`, on all six section
  6 programs, no program worse beyond a stated noise band, and peak RSS never
  higher (c37). Binary-size growth for shipped generic instances has a stated
  limit."
- **G27 (reach differential).** "Before A2 lands, a differential guard runs
  text reach and interface reach on every corpus program and requires
  identical reached sets. A case where a missed impl would otherwise fall
  back to an auto-derived trait proves a miss is an error, not a new
  meaning."
- **G28 (alternatives).** "Section 5 evaluates separately compiled generics
  (Swift witness tables, OCaml uniform representation, Go dictionaries) and
  shipping checked generic bodies in step A (Rust MIR, Go unified IR), each
  with cost and reason. Any choice that changes language-level performance
  goes to an owner ballot."

## Summary

The note's core move is right and matches strong prior art: a prebuilt,
per-declaration-lazy checked interface instead of text. It holds where it
says: built-in Core, outside a checkout, for hello-class programs. As
written, though, it does not deliver c22.

- Hello still parses Core text (the effect vocabulary) and compiler-owned
  Prelude text.
- The list of what step A leaves out is missing the largest categories:
  lowered packages, function values, trait impls, defaults, and all of
  `jet build`.
- The layout cannot be built "the way `JET_CLI_CORE_SOURCES` is" given the
  build order. It also understates decoding and size.
- The 1 ms budget is not measurable as specified.
- Tree mode turns into a full Core check per compile.
- Step B, the only path to c22 for real programs, is gated on beating a
  forbidden baseline.

Per AGENTS.md, the design stays unbuilt until it is redesigned against G1 to
G28 and reviewed again.
