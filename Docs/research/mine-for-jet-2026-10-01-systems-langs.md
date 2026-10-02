# Mine for Jet: Mojo, Zig, Roc, Skip, EYG, Odin, Hare, Verse, PyO3, and SPJ (2026-10-01)

## Verdict

Jet is further along than the comments assumed, and the best lessons are
structural rather than feature-shaped.

- **Already in Jet:** inferred failure unions (Roc's best idea), a propagation
  trail on uncaught failures (beats Zig's error return trace), a typed IR plus
  a MIR verifier (SPJ's typed stack), exactly-once `#SingleUse` and `#Close`
  handles (linear-type cleanup), inferred effect rows, struct-of-arrays layout,
  SIMD lanes, inline assembly, `jet cc`, and `#Export(c)`.
- **Real gaps:** narrowing authority to paths and hosts, substituting a
  service's implementation in tests (EYG), a width-generic and scalable lane
  type, run-time CPU dispatch, generics that can choose a type (Zig), co-editing
  text and Canvas (EYG), a leak report, a TIR verifier, release-at-last-use
  (Mojo), and complete C header import. Typed pointer stepping is ratified
  (D-RAWPTR2) but not built.
- **Mojo's biggest lesson is structure:** the numeric tower, SIMD, layouts, and
  parallelism live in libraries over a narrow set of compiler intrinsics. Jet's
  #3690 already points that way; card #4179 extends it to `Int`, `Float`, and
  lanes.
- **Defects found by live probes:** HEAD does not build (#4163); a runaway
  `prep` loop reports the wrong error (#4186); `jet run` crashes on the
  columnar layout example (#4187).

This run created 26 cards and later deleted 2 as duplicates, leaving 24. It
also raised 15 owner ballots and logged evidence on 15 cards. Every owner
comment has a disposition in the "Owner comments" section.

## Sources and capture

| ID | Source | Kind | Date | Capture |
|---|---|---|---|---|
| 5_oqWE9otaE | Developer Voices, Loris Cro: "What's Zig got that C, Rust and Go don't have?" | video | 2023-11-15 | creator subtitles, full read |
| i9nFvSpcCzo | Developer Voices, Loris Cro: "Zig as a Multi-OS Build System" | video | 2024-07-17 | creator subtitles, full read |
| 42y2Q9io3Xs | Developer Voices, Drew DeVault: "Will we be writing Hare in 2099?" | video | 2023-12-06 | creator subtitles, full read |
| harelang.org | Hare tutorial, spec (draft), stdlib docs, blog | docs | 2026-10-01 | read |
| UBgam9XUHs0 | Developer Voices, Simon Peyton Jones: "Exploring Verse, Haskell, Language Design and Teaching" | video | 2024-01-31 | original subtitles, full read |
| aKYdj0f1iQI | Developer Voices, Ginger Bill: "Is Odin 'Programming done right'?" | video | 2024-01-10 | original subtitles, full read |
| UmL_CA-v3O8 | Developer Voices, David Hewitt: "PyO3: From Python to Rust and Back Again" | video | 2024-07-03 | original subtitles, full read |
| JRcXUuQYR90 | Developer Voices, Chris Lattner: "Mojo Lang - Tomorrow's High Performance Python?" | video | 2024-05-01 | creator subtitles (en-GB), full read |
| github.com/modular/modular | Mojo stdlib, docs, MAX kernels | repo | commit 99e3a880 | sparse clone |
| wuGx35UIKTk | Developer Voices, Peter Saxton: EYG | video | not recorded (HTTP 429 on metadata) | original subtitles, full read |
| eyg.run | EYG site and repo CrowdHailer/eyg-lang | repo | commit 75f09475 | shallow clone |
| roc-lang.org | Roc docs, FAQ, repo | repo | commit e79b7f85 | shallow clone |
| skiplang.com | Skip docs, repo, SkipLabs successor | repo | 2026-10-01 | read |
| verse-reference | Epic Verse glossary and lessons, plus Lambda Days 2023 (OJv8rFap0Nw, SPJ and Sweeney) and GDC 2023 (5prkKOIilJg) talks | docs, video | 2023-06-26, 2023-04-24 | auto captions (Lambda Days) and creator subtitles (GDC), full read |

All sources were new to `Docs/spec/reference/prior-art.md` (checker status
`new`). Roc and Skip were extended from primary sources, building on the
2026-09-30 report without recapturing its two talks. The Verse Calculus paper
was not rerun.

Capture limits: YouTube returned HTTP 429 on redundant subtitle tracks, after
the chosen tracks had been saved. Epic's language-reference landing page
renders only a table of contents, so glossary and lesson pages were used.
Odin's inline-asm syntax from its docs failed on the nixpkgs Odin build
(dev-2026-05), and some Linear Haskell, Clean, and Idris pages were
unreachable. Audience comments were not in scope.

## Live probes

Toolchains ran through nix: Zig 0.16.0, Odin dev-2026-05, and Hare 0.26.0.1.
Jet was built from HEAD 1aca7cfe2 with lints capped, because HEAD fails its
own `deny(warnings)` (#4163). The capture files were temporary; the results
below and the card logs are the retained record.

| Probe | Result |
|---|---|
| Z1 Zig type function | `List(i32) == List(i32)` true, `List(i32) == List(u8)` false: memoized type identity. |
| Z2 Zig quota | Fails at 1000 backward branches with a note naming `@setEvalBranchQuota`; succeeds after raising it. |
| Z3 Zig allocators | `testing.allocator` reports a leak with an allocation trace and fails the test; a double free aborts. |
| Z4 Zig pointers | `*i32 + 1` is a type error; `[*]i32 + 1` works; slice overrun panics with index and length. |
| Z5 Zig errors | Inferred error sets switch exhaustively; `try` failure prints an a→b→main return trace. |
| Z6 zig cc | Windows PE32+ and static AArch64 musl from one Linux host; first run 10.7 s and 15.9 s, cached 0.3 s and 0.24 s. |
| Z7 Zig cache | Sequential rebuilds 0.38-0.68 s; two simultaneous builds crashed with `panic: DWARF TODO`. |
| O1 Odin `#soa` | Works; `&v_soa[0].x == &v_soa.x[0]`. |
| O2 Odin array math | Correct results, but no SIMD instructions in the emitted assembly at the tested setting. |
| O3 Odin asm templates | Documented syntax rejected by this Odin build. |
| O4 Odin methods | `f.bar()` is a field error; explicit procedure groups compile. |
| H1-H2 Hare | `?` propagates tagged-union errors; ignoring an error result is a compile error. |
| J1 Jet prep loop | E0956 "exhausted its fuel isn't supported" plus a cascading E0107, after 30 s; should be E0952 (#4186). |
| J2 `#SingleUse` | E0140 "`db` still owes `consume`"; discard needs `#Unsafe`. |
| J3 file close | A forgotten writer closes itself at scope end; the text reads back. |
| J4 narrowed authority | `#FX(FS.Read("data"))` is E0930; `FS.Read:"data"` is E0003; manifest strings parse but scope nothing in code. |
| J5 comptime types | `Packet<4>` with `prep N: Int` is E0003; a `prep fn` returning a type is rejected. |
| J6 failure unions | Inferred union runs on `jet run` and AOT; removing an arm gives an exhaustiveness error. |
| J7 typed JSON | `json.decode<Person>` works; a missing field gives a field error; an unknown field is accepted silently. |
| J8 pointer stepping | `p + 1` is E0109 and `p.offset(1)` is E0102 inside `#Unsafe`; D-RAWPTR2's ratified tools are unbuilt. |
| J9 untyped params | E0003 "Expected `:` after the parameter `a`". |
| J10 `Any` | E0350 with a helpful list of alternatives. |
| J11 `#Root` | `twice(5)` and `5.twice()` both work. |
| J12 emoji names | `jet run hello.🛩` runs; imports still resolve only `.jet`. |
| J13 clean | `jet clean` prunes the package store; `jet cache prune` manages the artifact store. |
| J14 inline asm | Removing the `; clobbers r10` comment changes nothing and reports nothing. |
| J15 propagation | Uncaught `Err` prints context lines and a numbered `Trail [E3002]`. |
| J16 columnar | Internal compiler error on `jet run` after two lines (#4187). |
| J17 DataTree | Nested read needs `&tree.field("user")`, `&user.field("name")`, `.text()`. |

## Owner comments, one by one

Each item gives the source evidence, Jet's state, and the disposition.

### Roc and Skip (no video)

- **Roc error and value handling; automatic error unions.** Roc's `?` unwraps
  `Ok` and returns `Err`; error tags from several calls merge into an open union
  that the compiler infers (roc `docs/langref/operators.md:122-160`). Open unions
  need a catch-all `_` in `match`; Roc's closed-row syntax `..[]` is documented
  but not built. Jet already infers failure unions by a call-graph fixed point
  and mints a closed enum per function (D-FAIL-INFER-UNION1=A;
  `crates/jet-sema/src/Sema/Bundle/Pipeline/FailureUnion.rs:1-71`; probe J6).
  Jet's closed union is stricter than Roc's open row: callers match exactly the
  failures that can arrive. No action.
- **Roc automatic type recognition.** Roc infers principal types with no
  annotations, and keeps inference complete by refusing higher-kinded and
  higher-rank types (FAQ). Jet requires parameter types (probe J9). Roc-style
  principal inference does not fit Jet's named structs, because `row.price` on an
  unannotated parameter has no single best type. Ballot **D-INFER-PRIVATE1** (#4182)
  therefore recommends Crystal's model: an untyped file-private helper is checked
  once per distinct set of argument types it is called with.
- **Roc's "JSON solver".** It is type-directed decoding, not a separate solver:
  the format supplies primitive parsers, the expected type supplies the shape,
  and nominal types opt in with `parser_for : _`
  (`docs/langref/parsers.md:3-120`). This corrects the 09-30 report, which said
  the JSON shape is inferred from later use. Roc needs an expected type. Jet
  already has `json.decode<T>` with derived `Decode`, but it builds a whole
  `DataTree` first. Card **#4184** decodes straight from bytes. Jet also accepts
  unknown fields silently (probe J7); that default is recorded, not balloted.
- **Skip compiler performance.** Skip memoized pure functions over an MVCC
  dependency graph with revisioned cells. Its compiler emitted LLVM and linked a
  C++ runtime, and was not self-hosted. SkipLabs now ships a TypeScript reactive
  framework on a native Skiplang runtime. Roc's new Zig compiler canonicalizes
  modules in parallel without waiting for imports. Logged on #3852.
- **Skip error and value handling.** Skip uses exceptions and `Option`, with
  mutability visible in types (`mutable`, `readonly`, `frozen`, `freeze`) and
  pure closures `~>`. Jet's errors-as-values with inferred unions is the
  stronger model. Skip's frozen-closure rule for safe parallelism is close to
  Jet's sendability rules. No action.

### Zig (video 5_oqWE9otaE)

- **Force typed pointer arithmetic.** Zig gives each pointer a kind: `*T` cannot
  step (probe Z4), `[*]T` steps by elements, `[]T` is a bounds-checked slice,
  and `[*:0]T` carries a sentinel. Jet's ratified D-RAWPTR2=A already answers
  this comment: typed element stepping (`p.offset(n)`), `mem.window(p, len)` to
  turn a C pointer plus length into a checked window, plus copy and fill. All
  of it sits under `#Unsafe`, with pointer origins checked in dev and hardened
  builds. That is your "tracked manual memory" idea, ratified but not built:
  probe J8 shows `p + 1` is E0109 and `p.offset(1)` is E0102. I drafted ballot
  D-PTR-CHECKED1, but adversarial review showed it would add a second stepping
  mechanism. I withdrew it and logged the evidence on #3645, which owns
  D-RAWPTR2.
- **Defer for C interop.** Already covered and stronger in Jet. A foreign
  constructor can declare `#Close(fn)`, so the named release runs exactly once
  or compilation fails (syntax-decisions D-FFI capability law). Plain `Close`
  values also close themselves at scope end on success, return, error, break,
  and cancellation, with deferred closes first (probe J3,
  `Examples/features/memory/scope_close_safety_net.jet`). No action.
- **Built-in allocator tracks leaks.** Zig's `testing.allocator` and
  `DebugAllocator` print the allocation site of every leak and abort on double
  free (probe Z3). Jet reports GC promotions only (`jet gc report`). Card
  **#4176** adds an allocation report to `jet test` and `jet dev`: live arena
  and pool slots, unreleased foreign handles, and `Shared` cycles, with sites,
  and no cost in release builds. Safe Jet cannot double free or use after free,
  so the report targets leaks and forgotten handles.
- **Comptime replaces `<>` generics.** Zig's generics are build-time functions
  returning types, memoized by argument (probe Z1). Their weakness is duck
  typing: errors appear only at instantiation, and interfaces have been
  requested since 2018 (issue #1268). Jet's S26 forbids comptime from choosing
  a type, D-META-GATE1 caps standard mode at number parameters (ratified,
  unbuilt), and type factories are opt-in (#3528). Ballot
  **D-GENERIC-TYPEFN1** (#4164) offers four options:
  - keep the ladder (status quo);
  - Pareto (recommended): a bounded type choice,
    `type Storage<T, prep N: Int>: Store<T> = prep { if N <= 16 -> Inline<T, N> else -> Heap<T> }`,
    where every branch is checked once against the bound;
  - unbounded computed shapes, which bring back Zig's late errors;
  - radical Zig style: `Heap(Int)` call syntax, which frees `<>`.

  Adversarial review moved the recommendation from unbounded to bounded
  choice, because bounded choice keeps "polymorphism is traits-only" and never
  creates a type without a declaration.

  On freeing `<>`: the only candidate uses found were Verse-style specifiers
  (Jet already uses `#` markers), effect rows (Jet uses `-[ ]>`), and tensor
  shapes (served by number parameters). None beats plain parser simplicity,
  and D-GENERIC-CALL1 already removed the `a < B > (c)` ambiguity by adjacency.
- **Bounded comptime loops with an override.** Zig's quota is 1000 backward
  branches, raised by `@setEvalBranchQuota` inside the code (probe Z2). Jet's
  budget is 10M steps per binding and cannot be raised. Ballot
  **D-PREP-FUEL1** (#4166) proposes `#Fuel(n)` on the binding. Probe J1 found
  the budget reports the wrong diagnostic today (#4186).

### Hare

- **Improved stdlib, error handling, type system.** Hare's errors are tagged
  unions with an error flag on the type (`type error = !(io::error | invalid)`);
  `?` propagates and `!` asserts (probes H1-H2: ignoring an error is a compile
  error). The stdlib mandate requires every exported symbol documented and
  tested, plus a module-specific `strerror`. Jet's `T E!` contracts and
  inferred unions cover the same ground with less writing. The stdlib mandate
  matches Jet's existing Core conformance work. No new action.
- **Linear types for cleanup.** Hare itself has no linear types; ownership is
  by convention. The survey covered Austral (linear handles threaded through
  calls), Vale (Higher RAII: a type that must reach a named method), Mojo
  (`not Deinitable else "call 'cleanup()'"`), Swift `~Copyable`, Rust affine
  moves plus `#[must_use]`, Granule, and Koka FBIP. Jet already has an
  exactly-once `#SingleUse` whose error says "still owes `consume`" (probe J2).
  What it lacks is naming the real job (commit or rollback) and one rule for
  single-use values and task joins. Ballot **D-OWES1** on the existing card
  #4139 proposes `#Owes(commit, rollback, on_fail: rollback)`: the type names
  its settling actions, and failure exits (`?`, `return Err`, unwinding,
  cancellation) settle automatically. `#MustUse` stays a separate "do not
  ignore" rule, and abandoning a debt keeps the `#Unsafe` gate. Option C makes
  Core transactions, locks, and buffered writers owe their action by default,
  the beginner-visible form you allowed. Card #4151 (unchecked transactions) is
  the first user.

### Simon Peyton Jones

- **Statically typed stack to codegen.** GHC keeps System FC Core typed through
  optimization, and Core Lint re-checks it after passes ([08:12-10:22];
  `GHC/Core/Lint.hs`). STG and Cmm are only partly typed. Jet already has a
  typed TIR and a MIR Lint that re-derives types after every optimizer pass
  (`Docs/spec/mir-lint.md`). Neither compiler has a TIR verifier, so a
  sema-to-TIR or TIR-to-MIR lowering bug can slip through. Card **#4175** adds
  TIR Lint in the self-hosted compiler.
- **Strip sugar, then optimize one thing.** GHC desugars about a hundred source
  forms into ten Core constructors, after typechecking so error messages stay in
  source terms. Jet does the same: AST (49 expression variants) → sema → typed
  TIR → MIR, with optimization only on MIR. Already implemented; no action.

### Odin

- **SoA and auto-lowering to SIMD.** Odin chooses SoA per container
  (`#soa[N]T`, slices, dynamic arrays) and allows both `v[0].x` and `v.x[0]`
  (probe O1). Its claim that array math lowers to SIMD was not confirmed: probe
  O2 found no SIMD instructions at the tested setting. Jet chooses SoA per type
  (`#Layout(columnar)`), but that path runs 15x slower than AoS (#2889) and
  crashes on `jet run` (#4187). Ballot **D-SOA-SITE1** (#4169) proposes a Core
  `Columns<T>` collection, Zig `MultiArrayList` style.
- **Hardware up; inline asm; both directions.** Beginners keep top-down
  auto-vectorization (D-SIMD3). For bottom-up control, ballot **D-LANES1**
  (#4167) proposes exact `Lanes<T, N>` plus a sizeless native `Lanes<T>`, whose
  width the machine sets at run time as on Arm SVE and RISC-V V. It replaces
  twenty fixed names, and reductions keep one fixed order so every tier prints
  the same bits. Ballot **D-CPU-DISPATCH1** (#4168) proposes multiversioned
  functions:
  - the compiler builds one copy per CPU level;
  - `prep if $build.cpu.has(.AVX2)` picks typed `core.arch` intrinsics inside
    each copy;
  - a start-up probe picks the copy, Highway style.

  Review also found that release builds pass `target-cpu=native` against
  ratified law (#4192). Odin's typed, encoding-checked asm templates are logged on #3042.
  Jet's clobber line is a comment today (probe J14).
- **Methods split a language into dialects; operator overloading.** Odin has no
  methods and no operator overloading (probe O4; FAQ). Jet's `#Root` lets one
  function be called as `f(x)` and `x.f()`, and the formatter keeps both
  (D-CALLDUAL1=E; probe J11), which is exactly Odin's dialect. Ballot
  **D-CALL-ONE1** (#4170) recommends retiring `#Root` in favor of
  import-scoped extensions (`pub fn Int.twice(self)` outside Int's module,
  Kotlin style). That leaves one way to declare a dot call and one spelling
  per function. Operators
  stay as ratified (D-OPDEF1, D-OPMIX1, `Numeric`), matching your view that a
  Numeric trait is the right balance.
- **Everything is data or a way to modify data.** Bill's words: "pretty much
  everything is either data or a transformation of data" ([55:25-56:30]). Short
  ballot **D-PHILO-DATA1** (#4185) proposes a philosophy line.

### PyO3

- **Filling Python's and Rust's roles at once.** Teams mix the two for Python's
  ecosystem and Rust's speed. Every boundary call costs more than a native
  call, so work should cross in large blocks (Polars). Async runtimes do not
  meet, and packaging is the main pain (maturin, wheels, abi3). Jet already
  ratified a sidecar Python broker with an opt-in embed tier (D-FFI-PY1=A,
  D-DEP-PY1=A). The lessons for that card: keep calls coarse, map exceptions
  to typed failures, and treat packaging as a first-class product. The typing
  side is ballot D-INFER-PRIVATE1.
- **Meet or beat Rust's Result, `?`, and matching.** Jet beats Rust on three
  points:
  - Failure unions are inferred, so there are no `From` impls and no
    `thiserror`/`anyhow` split (Sabrina Jewson, BurntSushi).
  - Uncaught failures print a context trail (D-FAIL-CTX1=A, probe J15).
  - Matching is exhaustive by type.

  Rust still leads on ecosystem maturity and, in places, on the speed of
  matched workloads. A trace card was drafted and withdrawn when probe J15
  showed Jet already has one.

### Mojo

- **Gradual typing; dynamic middle ground.** Mojo is statically typed, with
  `def` as its only declaration keyword in v1.1. Untyped Python-style code is
  an explicit phase-1 non-goal; Python values are one `PythonObject` type with
  checked conversion. Lattner rejects "type hints over an untyped universe".
  Ballot **D-INFER-PRIVATE1** (#4182) weighs:
  - untyped file-private helpers checked per call, Crystal style (recommended);
  - tool-written signatures;
  - a `Dyn` value replacing `DataTree`, like `PythonObject`;
  - full Roc-style inference.
- **Parallelism as a first-class library.** Parallel-for and vectorize are
  library functions over a private task runtime. Jet has
  `para_map`/`para_fold`, but they run 2.3x slower than serial on cheap kernels
  (#2922, evidence logged).
- **Direct access to compiler instructions.** Mojo's library code uses
  `__mlir_op`, `__mlir_type`, and `__mlir_attr`; LLVM and NVVM dialects reach
  inline assembly and GPU intrinsics. Jet keeps `__core_intrinsic`
  compiler-only. Ballot D-CPU-DISPATCH1 proposes a typed public `core.arch`
  instead of opening that namespace.
- **Hardware-up, forward-facing for new hardware.** Lattner's mechanism is a
  library writer adding a target-specific path with a generic fallback, so new
  hardware is a library pull request ([29:32-31:20]). This is the reason for
  D-LANES1, D-CPU-DISPATCH1, and card #4179.
- **No AST, emit IR directly.** Confirmed with nuance: the parser emits the
  MLIR `lit` dialect (MojoCompilerWalkthrough.md:136-138), so there is still a
  parse-level IR, just no separate tree stage. The front end of the self-hosted
  compiler can adopt this internally with no owner choice. The editing
  question is ballot D-PROGRAM-FORM1, covered under EYG.
- **Int and Float are library structs.** In the current stdlib, `Int`, `UInt`,
  and `Float64` are aliases of one `SIMD[dtype, width]` struct whose field is a
  KGEN MLIR scalar. Every arithmetic method calls a `pop.*` operation, and
  literals are compile-time infinite-precision `IntLiteral` and `FloatLiteral`
  (`std/simd.mojo:115-240,443-605,1069-1634`). Benefits are one arithmetic
  definition, less compiler magic, and hardware specializations in library
  code. The cost is that diagnostics expose `SIMD[...]` types. Jet's exact,
  unbounded `Int` cannot be a SIMD alias, so card **#4179** moves Jet's numeric
  tower into Core `.jet` over named intrinsics and keeps its semantics.
- **Comptime "inspired by Zig, improved".** Square-bracket parameters take types
  and values; `comptime if/for` replaced `@parameter`; `where` clauses with
  custom messages constrain parameters. That fixes Zig's duck-typing errors.
  The improvement over Zig is constrained parameters, which ballot
  D-GENERIC-TYPEFN1 option A keeps through trait bounds.
- **Move and copy constructors, destructors, C interop.** Current names are
  `__init__(out self, *, copy: Self)`, `__init__(..., *, deinit move: Self)`,
  and `__deinit__`; C calls go through `external_call["abs", c_int]`. Jet's
  move `^`, copy `~`, `Close`, and `#Import` cover the same ground. No action.
- **Destroyed right after last use.** Verified in Mojo docs. Jet moves at last
  use (#3836) but drops at scope end. Card **#4177** frees plain memory at last
  use and keeps resource close order unchanged.
- **Never memcpys.** Narrower than it sounds. Moves are free because values are
  not pinned, and `out` result slots build results in the caller's memory, but
  Mojo's docs admit that passing a copyable value to `var` without `^` may copy.
  Jet has no result-slot contract, and #2890 shows a 473 MiB list copy. Card
  **#4178** adds a measured copy budget and in-place returns.
- **Actors.** Mojo had none; Lattner prefers a library with minimal type help.
  Short ballot **D-ACTOR1** (#4183) proposes `core.actor` over tasks and
  channels.
- **2D memory, tiles, vectors, matrices.** Mojo's `Layout` is a shape-and-stride
  algebra. `LayoutTensor` carries it as a compile-time parameter, and `tile`,
  `distribute`, `vectorize`, and `Swizzle` return zero-copy views. Jet already
  ratified public tile values (D-GPU-TILE-SCOPE1=B, #3215). The layout evidence
  is logged there; it depends on number parameters (#3505).
- **Moving logic into libraries; stdlib structure.** The Mojo stdlib tree is
  laid out as follows:
  - **prelude:** ordinary source that auto-imports;
  - **builtin:** scalars, literals, and parameter machinery;
  - **traits:** `Movable`, `Copyable`, `ImplicitlyCopyable`, and `Deinitable`,
    with conditional conformance;
  - **collections, memory, algorithm, layout, gpu, python, runtime, and sys:**
    the rest of the library.

  The compiler keeps parsing, parameter evaluation, lifetimes, overload
  resolution, and lowering. Jet's equivalents are #3690 (Core in Jet only) and
  #4179. The design lesson to copy is that the prelude and trait vocabulary
  are plain library source, not compiler tables.
- **Emoji file extension.** Lattner calls it a first. The current Modular
  repository has no `.🔥` file or rule. Jet already runs `hello.🛩` by accident
  (probe J12). Short ballot **D-EXT-EMOJI1** (#4173) recommends `.jet` only.
- **MLIR versus LLVM.** Mojo lowers MLIR to LLVM IR for code generation
  (walkthrough :121-129). No source gives a measured MLIR-versus-LLVM delta;
  the gains come from domain-level rewrites before LLVM. Jet's equivalent is
  the private typed SSA proposal (#2059). Adopting MLIR would add a large C++
  dependency against I6, with no measured gain. Not recommended.

### Zig (video i9nFvSpcCzo)

- **No `clean` because the cache is reliable.** The source is Andrew Kelley's
  2020 `zig cc` post and the Zig 0.4.0 release notes:
  - manifest rows store inode, mtime (seconds and nanoseconds), a content hash,
    and the path;
  - compiler identity hashes the binary and its dynamic libraries;
  - a file whose mtime equals the current time truncated to filesystem
    granularity is never trusted.

  Two caveats. The video admits a cache bug can still force manual deletion.
  Probe Z7 crashed two simultaneous Zig builds. Jet's run cache hashes content,
  mtime, and length, but identifies the compiler by a 4 KiB prefix plus
  metadata (`Source/RunCache.rs:88-181`). Three hostile cases are logged on
  #2531: same-second double write, a rebuilt compiler with an identical
  prefix, and a zero-mtime filesystem. `jet clean` prunes storage and is never
  needed for correctness.
- **Cross-compilation as a wedge; interop; Nix replacement.** All four interop
  pieces you selected exist or are carded:
  - `jet cc`/`jet c++` on the pinned Jetpack toolchain;
  - `#Export(c)`;
  - foreign builds (#1347);
  - cross toolchains (#758, #1058, #1060; Zig evidence logged).

  The weak link is header import: Jet's own parser maps simple declarations
  and skips unions, bitfields, callbacks, variadics, and most macros. Short
  ballot **D-CIMPORT1** (#4174) proposes reading headers with the
  already-pinned Clang.

### EYG

- **How EYG handles effects.** Effects are algebraic: `perform Label(value)`
  reaches the nearest handler, which may resume, return early, mock, or deny.
  Unhandled effects stop with `UnhandledEffect`. Effect rows are inferred,
  with open and closed contexts. Jet's effect rows are inferred and checked at
  build time, then erased, which is cheaper, and Jet's authority tree is richer.
  Jet's gap is that no scope can swap an effect's implementation, so tests hit
  the real network or thread a fake client through every call. Ballot
  **D-EFFECT-HANDLE1** (#4180) recommends Core service providers (HTTP
  transport, file system, process runner) supplied through the existing
  `#Context`, as in `#Context(http: FakeWeather{}) { ... }`. Effects stay
  erased, fakes are typed trait implementations, and authority is never
  widened. Adversarial review rejected the first draft, which keyed
  replacement on effect names, because leaves are permissions, not operations.
- **Enums as row types.** EYG and Roc model unions as open rows. Jet's unions
  are closed by ratified law (D-UNIONTYPE1=A, D-ENUM-EVOLUTION1=A), and the
  main use of open rows, inferred error unions, is already covered. Adding
  rows would create a second union mechanism (I8). Not balloted; open a ballot
  if you want rows reconsidered.
- **AST over the wire for text plus block editing.** EYG's IR is a closed node
  list encoded as DAG-JSON with content-addressed references; text is one
  optional parser. Jet's Canvas already sends semantic edits checked against a
  file revision, but a stale edit is refused rather than merged. Ballot
  **D-PROGRAM-FORM1** (#4171) recommends replaying a stale semantic edit
  against the newest revision, then re-checking it, so text and Canvas edits
  both land unless they touch the same thing. A stored node-ID tree, EYG's
  model, was weighed and lost, because plain saves and git checkouts break
  stored IDs. On content addressing, Jet already hashes build records.
  Per-definition hashing (option D) would break git and grep for identity
  co-editing does not need.
- **Narrow effects to domains and files.** EYG's policies
  (`allow_get_hosts`, `allow_under`) and Deno's flags are the models. Jet's
  rights carrier already understands `FS.Read:/data` and
  `Net.Connect:api.example.com`, but no surface accepts them and network hosts
  are never checked (probe J4). Ballot **D-AUTH-NARROW1** (#4165) proposes
  scoped leaves in `#FX` and the manifest, checked at build time when literal
  and at run time otherwise, including redirects and symlinks.
- **Sandbox as a wasm replacement.** The speaker is careful: "run anywhere"
  means a new host is quick to write, not a shipped universal sandbox. For
  Jet, the sandbox properties come from D-AUTH-NARROW1 and D-EFFECT-HANDLE1
  plus the proposed Jetpack portable format (unratified). Wasm's documented
  costs (component-model complexity, late GC, threads, 64-bit memory) come
  from the WebAssembly component-model explainer and are evidence only.

### Verse

Verse writes generics as functions over types, which supports D-GENERIC-TYPEFN1.
It uses failure as control flow (`Foo[]` versus `Foo()`, `<decides>`) and
`<transacts>` rollback, with an effect hierarchy of `<transacts>`, `<varies>`,
`<computes>`, and `<converges>`. Jet's `?`, `??`, `#Transact`, and inferred
effect rows cover the same needs. Open rollback defects #3978 and #3967 matter
more than new syntax. Verse's persistable classes may add only defaulted fields
after publication, a useful rule for Jet's Codable evolution, and it accepts
three equivalent block spellings, which Jet's one-canonical-form rule rejects.
No new ballot.

## Corrections and disputed claims

- "Mojo has no AST": true only as "no separate tree stage". The parser still
  emits an MLIR dialect.
- "Mojo never memcpys": true for moves; copies remain possible elsewhere.
- "Mojo supports `.🔥`": a 2024 claim; absent from the current repository.
- "MLIR replaces LLVM": Mojo still lowers through LLVM IR; no measured delta.
- "Odin array programming lowers to SIMD": not reproduced (probe O2).
- "Zig needs no clean": true for ordinary staleness; cache bugs and concurrent
  builds (probe Z7) are exceptions.
- "Roc infers JSON shape from later use" (09-30 report): it needs an expected
  type.
- The Skip "SKStore" name from the 09-30 report is not in the checked sources;
  the mechanism is a revisioned dependency graph.

## Avoid list

- Duck-typed generics that fail only at instantiation (Zig #1268). Keep trait
  bounds and one-place checks.
- An untyped universe with optional hints (Mojo's stated reason). Keep checking
  before running.
- Emoji or alias file extensions and multiple equivalent block spellings
  (Verse); one canonical form.
- Global, stack-based permission systems (Java SecurityManager, JEP 411). Keep
  grants in one auditable block per package.
- Full algebraic effects with continuations on every tier unless a workload
  proves the need; they duplicate tasks, `Stream`, and `#Transact`.
- A separate C++ compiler framework (MLIR) without a measured gain.

## Beat vectors

- **Versus Rust:** inferred failure unions plus trails, effects and authority
  rows, and scope-end close with exactly-once foreign handles.
- **Versus Zig:** typed generics with one-place checks (D-GENERIC-TYPEFN1
  option A) and audited, origin-checked pointer stepping (D-RAWPTR2).
- **Versus Mojo:** one meaning across AOT, JIT, interpreter, and web; authority
  narrowing; exact `Int`.
- **Versus EYG:** static effect checking at no run-time cost, plus scoped
  replacement where tests need it.
- **Versus Odin:** columnar collections for any type, and safe typed intrinsics
  behind proven CPU checks.

## Agent-optimality

These sources move diagnostic clarity most. Zig's quota note names the fix,
Roc's errors name the conflicting use, and EYG reports all errors in one pass.
Jet is weakest where probes found a wrong or missing message: E0956 for a
runaway prep loop (#4186), an ICE on columnar lists (#4187), a silent clobber
comment (#3042), and E0102's fix suggesting a method be defined on Core's `Ptr`
type. Each is carded or logged.

## Board changes

New ballots, all on the owner's decide lane once ready:

| Ballot | Card | Recommended |
|---|---|---|
| D-GENERIC-TYPEFN1 | #4164 | bounded type choice; `<>` kept |
| D-AUTH-NARROW1 | #4165 | scoped leaves in `#FX` and the manifest |
| D-EFFECT-HANDLE1 | #4180 | Core service providers in `#Context` |
| D-OWES1 | #4139 | declared debts with `on_fail` |
| D-PREP-FUEL1 | #4166 | `#Fuel(n)` on the binding |
| D-LANES1 | #4167 | exact `Lanes<T, N>` plus native `Lanes<T>` |
| D-CPU-DISPATCH1 | #4168 | multiversioned functions |
| D-SOA-SITE1 | #4169 | `Columns<T>` replaces the marker |
| D-CALL-ONE1 | #4170 | extensions replace `#Root` |
| D-INFER-PRIVATE1 | #4182 | untyped helpers checked per call |
| D-PROGRAM-FORM1 | #4171 | replay stale edits |
| D-ACTOR1 | #4183 | library actors in Core |
| D-EXT-EMOJI1 | #4173 | `.jet` only |
| D-PHILO-DATA1 | #4185 | add a boundary line |
| D-CIMPORT1 | #4174 | pinned Clang reads headers |

New cards without ballots:
- #4163 HEAD build break;
- #4175 TIR Lint;
- #4176 allocation report;
- #4177 free at last use;
- #4178 copy budget;
- #4179 numeric tower in Core Jet;
- #4184 direct typed decode;
- #4186 prep fuel diagnostic;
- #4187 columnar ICE;
- #4192 release builds use target-cpu=native against D-FFI-ASM-CPU1=B.

Evidence logged on #4139, #2531, #3042, #3215, #2889, #3852, #3836, #4151,
#3505, #3528, #758, #2922, #3645, #1344, and #4175. Two cards were created and
deleted as duplicates:
- #4172 (failure trace), after probe J15 showed D-FAIL-CTX1 already provides
  the trail;
- #4181 (pointer ladder), after review showed D-RAWPTR2 already decides it.

## Ballot review

The four short ballots (D-ACTOR1, D-EXT-EMOJI1, D-PHILO-DATA1, D-CIMPORT1) use
the short profile, with no reader passes. Each of the eleven full ballots had a
fresh Luna 5.6 beginner (RLI5) pass and an adversarial pass by the run's one
Opus 5.5 reviewer. Material findings were repaired before submission:

| Ballot | What review changed |
|---|---|
| D-GENERIC-TYPEFN1 | Invalid one-line `if` examples fixed; recommendation moved from unbounded computed types to bounded type choice; every amended law listed. |
| D-AUTH-NARROW1 | Baseline corrected (manifests already accept `"FS.Read:dir"`); intersection, path-base, SSRF, and child-process rules added; scoped-handle option added. |
| D-EFFECT-HANDLE1 | Leaf replacement rejected (leaves are permissions); recommendation moved to Core service providers in `#Context`. |
| D-OWES1 | `#MustUse` kept separate; `#Unsafe` abandon gate kept; `on_fail` settlement added; task and generic rules stated. |
| D-PREP-FUEL1 | Split example fixed to four chunks; dependency budgets need a root grant; step defined. |
| D-LANES1 | Twenty lanes, not ten; a build-time width cannot express SVE, so A became exact plus native lanes with fixed-order reductions. |
| D-CPU-DISPATCH1 | Ratified flag spelling used; recommendation moved to multiversioned functions; `target-cpu=native` defect carded (#4192). |
| D-SOA-SITE1 | `Columns<T>` is compiler-provided, not reflection-built; ratification waits on a paired performance cell. |
| D-CALL-ONE1 | Option added and recommended: extensions replace `#Root`. |
| D-INFER-PRIVATE1 | Return rule corrected (omitted means unit); Roc inference fails on named types, so A became per-call checked helpers. |
| D-PROGRAM-FORM1 | Canvas baseline corrected (semantic, revision-checked edits); recommendation moved to replaying stale edits; compiler-stage question removed. |
| D-PTR-CHECKED1 | Withdrawn: D-RAWPTR2=A already ratifies audited, checked pointer stepping (#3645). |

The full review files were capture artifacts; their findings are recorded in
each ballot's long form and review summary.

## Finding dispositions

Findings: F1 HEAD build break; F2 prep fuel diagnostic; F3 columnar ICE; F4
`target-cpu=native`; F5 comptime type choice; F6 authority narrowing; F7
effect substitution; F8 linear debts; F9 typed pointer stepping; F10 prep
budget override; F11 lane type; F12 CPU dispatch; F13 SoA per collection; F14
call spelling; F15 Python-side typing; F16 co-editing; F17 actors; F18 emoji
extension; F19 philosophy line; F20 C header import; F21 TIR Lint; F22
allocation report; F23 release at last use; F24 hidden copies; F25 numeric
tower in Core; F26 direct typed decode; F27 cache hostile cases; F28 checked
asm templates; F29 tiles and layouts; F30 columnar speed; F31 incremental
compiler; F32 library parallelism; F33 cross-compilation; F34 `jet cc`
toolchain; F35 propagation trail; F36 error unions versus Rust; F37 defer for
FFI; F38 desugar then optimize; F39 operators and Numeric; F40 enums as rows;
F41 MLIR; F42 Verse; F43 Hare errors; F44 PyO3 boundary; F45 Mojo no-AST front
end; F46 unknown JSON fields; F47 E0102 fix text on `Ptr`; F48 Odin auto-SIMD;
F49 Zig concurrent builds; F50 sandbox as a wasm replacement.

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 | card | #4163 |
| F2 | card | #4186 |
| F3 | card | #4187 |
| F4 | card | #4192 |
| F5 | card | #4164 |
| F6 | card | #4165 |
| F7 | card | #4180 |
| F8 | card | #4139 |
| F9 | card | #3645 |
| F10 | card | #4166 |
| F11 | card | #4167 |
| F12 | card | #4168 |
| F13 | card | #4169 |
| F14 | card | #4170 |
| F15 | card | #4182 |
| F16 | card | #4171 |
| F17 | card | #4183 |
| F18 | card | #4173 |
| F19 | card | #4185 |
| F20 | card | #4174 |
| F21 | card | #4175 |
| F22 | card | #4176 |
| F23 | card | #4177 |
| F24 | card | #4178 |
| F25 | card | #4179 |
| F26 | card | #4184 |
| F27 | card | #2531 |
| F28 | card | #3042 |
| F29 | card | #3215 |
| F30 | card | #2889 |
| F31 | card | #3852 |
| F32 | card | #2922 |
| F33 | card | #758 |
| F34 | card | #1344 |
| F35 | decision | D-FAIL-CTX1=A |
| F36 | decision | D-FAIL-INFER-UNION1=A |
| F37 | decision | D-SHAPE-RESOURCE2=A |
| F38 | no-action | already implemented: AST -> sema -> typed TIR -> MIR, with optimization only on MIR |
| F39 | decision | D-OPDEF1=A |
| F40 | decision | D-UNIONTYPE1=A |
| F41 | no-action | rejected: MLIR adds a large C++ dependency (I6) with no measured gain; private SSA (#2059) covers domain-level rewrites |
| F42 | no-action | covered: Verse failure and transactions map to #Transact; open rollback defects are #3978 and #3967 |
| F43 | no-action | already implemented: Hare-style typed error unions match Jet's T E! contracts and inferred unions |
| F44 | decision | D-FFI-PY1=A |
| F45 | no-action | internal implementation choice for the self-hosted front end; no owner gate |
| F46 | no-action | recorded default: typed JSON decode ignores unknown fields; no source proposes a change |
| F47 | card | #3645 |
| F48 | no-action | archived: Odin auto-SIMD claim not reproduced (probe O2); evidence only |
| F49 | no-action | archived: concurrent Zig builds crashed (probe Z7); peer evidence only |
| F50 | no-action | covered: sandbox and portability needs map to #4165, #4180, and the proposed Jetpack portable format |
<!-- /audit-dispositions -->

Strongest unverified assumption: that replaying stale semantic edits
(D-PROGRAM-FORM1) merges concurrent text and Canvas edits without visible
surprises. No
prototype has measured merge behavior on real editing sessions.
