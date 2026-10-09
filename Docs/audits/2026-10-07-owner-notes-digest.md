# Owner scratch notes: what Jet already does, what is missing, what to do

Prepared 2026-10-07 by OwnerNotes1 for the owner. It covers every note in
`Tools/tower/.tower/scratch/owner-scratch.md` from "Cache hits are critical"
through "unity build vs multiple compilation units". The notes above that line
(the quote, the style-guide idea, the pitch deck and the philosophy wording) are
out of scope by Main's correction.

How to read each section:

- **Today in Jet** lists facts. Each fact cites a ratified decision (D-…), a spec
  section, a source file, or a probe I ran. Probes ran with the shared release
  `jet` (`/mnt/jetscratch/candidates/dev-595e0dca3/jet`); the probe files are in
  `~/.cache/jet-dev/scratch/OwnerNotes1/probe/`. A claim I did not observe is
  marked [INFERENCE].
- **Gap** says what is missing or broken.
- **Recommendation** says what I suggest.
- **Kind** is one of: *answered* (Jet already does it, or a ratified decision
  covers it), *decision-needed* (a ballot draft is ready for you), *work-item*
  (a card draft is ready for Main), or *doc-update*.

Ballot drafts are in `~/.cache/jet-dev/scratch/ballots/`. Card drafts are in
`~/.cache/jet-dev/scratch/OwnerNotes1/cards/`. Nothing was written to Tower.

## Summary

| # | Note | Kind | Draft |
|---|---|---|---|
| 1 | Cache control and seeing cache misses | decision-needed + work-item | `D-CACHE-CONTROL1`, card `cache-profiler` |
| 2 | Type-based switch | answered + work-item (two bugs) | card `union-switch-bugs` |
| 3 | Guidance for hard problems; SOLID; over-encapsulation; primitives-first APIs | answered + doc-update | card `design-guide` |
| 4 | Beat Zig on SoA and pointers; custom allocators; SoA types | answered + work-item (existing cards) | existing #2889, #3142 |
| 5 | Fewer layers to assembly; replace LLVM; speed of light; memcpy | decision-needed + doc-update | `D-BACKEND-OWN1`, card `architecture-doc` |
| 6 | Odin primitives: quaternions, endian numbers, complex | answered (complex) + decision-needed (endian) + work-item (quaternion) | `D-ENDIAN-TYPES1`, card `quaternion` |
| 7 | Zero undefined behavior | answered + work-item | card `defined-behavior-ledger` |
| 8 | Build settings in `fn build()` | answered | — |
| 9 | Multiple named returns without tuples | answered | — |
| 10 | String versus string builder | answered | — |
| 11 | Pointer classes; `~` as bit operators | answered (pointers) + decision-needed (`~`) | option F for open `D-SIGIL-FREED1` |
| 12 | Why Odin avoids move semantics | answered | — |
| 13 | Clean casts, maybe `<>` | answered (+ input to open `D-ANGLE-PAIR1`) | — |
| 14 | Immutables are compile-time constants | answered (with one deliberate difference) | — |
| 15 | Implicit type inference | answered (open `D-INFER-LEVEL1` decides the rest) | — |
| 16 | Enum backing integer type; exhaustive enum matching | decision-needed (backing) + answered (exhaustive) | `D-ENUM-BACKING1` |
| 17 | Remove `alias`; alias by `New :: Old` | decision-needed | `D-ALIAS-BIND2` (full), plus finished `D-VIS-REEXPORT1` |
| 18 | Variadic `min`, `max`, `clamp` in the prelude | decision-needed | `D-PRELUDE-MINMAX1` |
| 19 | Built-in array math and swizzling | answered + work-item (bug) | card `vec-math-bug` |
| 20 | CPU/GPU split compilation; portable GPU kernel library; pushing code to the GPU | answered (design) + work-item | card `gpu-kernel-library` |
| 21 | Portable `sin` and Jet-defined math | work-item | card `portable-math` |
| 22 | A WASM replacement that exposes host hardware | answered (direction) + decision-needed | `D-HOST-CAPS1` |
| 23 | RAII, earlier cleanup, tail recursion | decision-needed | `D-DROP-ASAP1` |
| 24 | Classes versus structs | answered | — |
| 25 | Unity build versus compilation units | answered | — |

---

## 1. Cache control, and seeing when data leaves the cache

**Today in Jet**

- Layout control exists. `#Layout(c, align(64))` aligns a struct to a cache line;
  `align(target, N)` asks for any alignment the target supports (D-PLACE1,
  D-LAYOUT-ALIGN1).
- Sizes are visible. `T.$layout.size` gives the byte size in this build
  (D-LAYOUT-SHOW1=B), and `jet inspect expand` has a `layout` lens with size,
  alignment, stride and field offsets (spec "`jet inspect expand`").
- Column storage is chosen per collection with `Columns<T>` (D-SOA-SITE1).
- The release optimizer may copy hot columns into cache-sized blocks and run
  proven kernels on the worker pool, only when a run-time measurement says it
  pays (D-ACCEL1); `jet inspect accel` shows each decision.
- Hidden costs are surfaced: `jet explain --cost` lists hidden copies and
  fallbacks per line, and a loop lint flags them (D-COSTLAW1).
- Profiles keep source locations (D-PROFILE-DETAIL1) and offer sampling or
  counting (D-PROFILE-METHOD1). A live inspector exists (D-OBSERVE-LIVE1).
- No decision mentions prefetch, cache lines, cache misses or hardware counters
  (searched all 1,565 decisions).

**Gap**

1. No way to *see* cache behavior. The profiler measures time, not L1/L2/L3
   misses, TLB misses or page faults, and nothing maps them to source lines.
2. No way to *steer* the cache: no prefetch hint, no "write without filling the
   cache" store, no named cache-line size for padding shared counters.

**Recommendation**

- Work item (card `cache-profiler`): add a hardware-counter method to the
  existing profiler, `jet perf run --profile-method=memory`. It reads the CPU's
  own counters (Linux `perf_event_open`) and reports, per source line, L1 data
  misses, last-level misses, TLB misses and page faults. The code map and editor
  then shade lines by "where this line's data came from": L1, L2, L3, RAM or
  disk. This adds no language surface, so no ballot.
- Ballot `D-CACHE-CONTROL1` (short): expert cache hints in `core.mem`
  (`mem.prefetch`, a streaming store, and `$build.cpu.cache_line` for padding).
  Recommended A keeps them as ordinary expert functions, safe and audited by the
  profiler, instead of new syntax.

**Kind:** decision-needed + work-item.

## 2. A type-based switch

**Today in Jet**

- `if value == { … }` arm tables are the one branching form (spec "Branching,
  results, and loops").
- Named sum types (`enum` with payloads) switch by case. Probe
  `probe/typeswitch.jet` prints `num 3` and `text x`.
- Anonymous unions switch by member *type*: `fn area(shape: Circle | Square)`
  with arms `.Circle(c) ->` and `.Square(q) ->` (D-UNIONTYPE1=A, syntax-decisions
  "Anonymous union types"). This is the type-based switch.
- Failure unions also match by type (D-FAIL-INFER-UNION1).
- Struct, list and map shape patterns are ratified (D-PAT-RECORD1, D-PAT-MAP1).
- A trait value (`fn f(s: Shape)`) is dynamic dispatch with no downcast. Open,
  growable data uses `Dyn` (D-INFER-PRIVATE1).

**Gap: two real bugs found by probing**

- `probe/union5.jet`: passing a struct literal straight into a union parameter,
  `area(Circle{r: 1.0})`, prints `0.0` instead of `3.0` on `jet run`. That is a
  silent wrong answer. The same call with a named value (`probe/union4.jet`)
  prints `3.0`.
- `probe/union3.jet`: passing one named value to a union parameter twice is an
  internal compiler error: "MIR pass legality-verification rejected the
  program: … uses moved place".

**Recommendation:** card `union-switch-bugs` (P0 correctness). No new feature
is needed; the language already has the switch you asked about.

**Kind:** answered + work-item.

## 3. Guidance for hard problems; SOLID; over-encapsulation; primitives-first APIs

These four notes are one theme: how Jet helps people design well without
boxing experts in.

**Today in Jet**

- *Hard problems.* Diagnostics state what, why and how to fix (philosophy
  pillar 1, Docs/spec/diagnostics.md). Rule-trap audit before 1.0
  (D-BEGINNER-TRAPS1). Optional architecture advice in `jet review
  --architecture` (D-ARCH-ADVICE1). One place for compiler decisions, `jet
  inspect decisions` (D-INSPECT-DECISIONS1). Claims, replay and proof tools
  (`jet prove`, `.jetreplay`). A first-hour guide exists
  (Docs/spec/guides/first-hour.md). There is no written method for hard
  problems.
- *Primitives first, then wrappers.* Core keeps a friendly door and a
  lower-level door and teaches both with a shared golden example
  (D-ONCE-LAYER1=B, e.g. `core.crypto` and `core.crypto.expert`). Core API rules
  are law (D-CORE-DOCTRINE1, Docs/spec/stdlib-api-laws.md). Public APIs return
  results instead of filling dummy outputs (D-OUTPUT-RETURNS1). Chip knowledge
  lives in library instruction entries, so the bottom layer is reachable from
  Jet (D-CORE-BOUNDARY1).
- *Over-encapsulation.* Every safety rule has a written, audited way out:
  `#Unsafe("reason")` with raw pointers (D-RAWPTR2), `#Arithmetic(.Wrapping)`
  (D-WRAP-SCOPE1), `#Multiversion` and named CPU instructions (D-CPU-DISPATCH1),
  inline assembly (D-FFI-ASM-COMPLETE1), `#!Multiversion` and `#Scalar`
  refusals. Teams may forbid any gate kind and every gate states its reason
  (D-GATE-LAW1). Visibility is public by default; only modules and struct fields
  can be `priv` (owner directive 21, card #4723), so libraries cannot hide
  everything.
- *SOLID.* No decision mentions it. Jet has no classes or inheritance, so the
  inheritance parts of SOLID do not apply.

**Gap**

- No written "how to approach a hard problem in Jet" method.
- No page that maps SOLID to Jet and names its weak points.
- The "always an audited way out" rule is spread over many decisions instead of
  stated once.

**Recommendation (doc-update, card `design-guide`)**: one guide,
`Docs/spec/guides/design.md`, with three short parts:

1. *A method for hard problems*, built on tools Jet already has: write the
   data types first (enums, unions, `Columns<T>`); state facts as types
   (ranges, distinct types, typestates); write a claim or test that fails;
   measure with `jet perf` and `jet explain --cost`; read `jet inspect
   decisions` and the generated code; then reach for the lower door.
2. *SOLID, translated.* Keep: single responsibility (one module per job),
   interface segregation (small traits), dependency inversion (pass traits or
   providers, D-CORE-PROVIDER1). Drop: open/closed and Liskov as written,
   because they assume inheritance; Jet's closed enums plus exhaustive matching
   are the better tool for "add a case and find every place that must change".
   Warn against SOLID's known failure: many tiny interfaces and layers that hide
   the data and the cost.
3. *The escape-hatch ladder.* One table: each safety default, its audited
   opt-out, and how to see the cost. This is the structural answer to
   "over-safe and over-prescriptive": the default is safe, but every limit has a
   named way through that a reviewer can find.

**Kind:** answered + doc-update.

## 4. Beat Zig on struct-of-arrays and pointers; custom allocators

**Today in Jet**

- *SoA.* `Columns<T>` holds any struct as columns; `ps.y` is a column view and
  the list API still works (D-SOA-SITE1). All tiers share one column store
  (D-SOA-TIER1). The release optimizer can block columns for cache reuse
  (D-ACCEL1).
- *Pointers into arrays.* Safe code uses views and places: `values[a..b]` is a
  no-copy view, `&values[i]` is a write window, and the compiler proves disjoint
  writes (spec "Named views and places"). Live links use `@T` (D-MEMREF1).
  Raw pointers exist behind `#Unsafe`: `?mem.Ptr<T>`, `p.offset(n)`,
  `p.cast<U>()`, `mem.window(p, len)`, `mem.alloc_raw`, `mem.copy`, `mem.fill`,
  with checks kept in dev and hardened builds (D-RAWPTR2, D-HARDENED1).
- *Allocators.* `core.mem` has `Arena`, `Bump`, `Pool` and `Fixed` without
  `#Unsafe` (spec "Allocator families"). A program swaps or wraps its whole
  allocator in `package.jet` (D-ALLOC-PROGRAM1). Jet's own allocator becomes the
  default only where it beats the system's malloc (D-ALLOC-DEFAULT1). A function
  can promise a memory ceiling, `-[!Mem.Alloc(above: 65536)]>`
  (D-AUTHORITY-MEM2). Freestanding targets name their allocator as a target
  fact (D-FREESTAND-FACTS1).
- What allocators interfere with: views and arena regions are tied to their
  owner, so `arena.reset()` invalidates views (E0632); task crossings need
  owned values (spec "Arena regions and scope-bound views").

**Gap**

- Measured: columnar particles run 15x slower than plain structs (card #2889,
  ready). The Zig claim is not met until that card closes.
- Enum layout locality is unmeasured (card #3142).
- A `Columns<T>` living in a caller-chosen arena is not spelled out [INFERENCE:
  no decision combines D-SOA-SITE1 with the allocator families].

**Recommendation:** no new ballot. Finish #2889 and #3142 with a paired cell
against Zig's `MultiArrayList` in the gauntlet, as the performance gate
requires. When that cell runs, add an arena-backed `Columns<T>` variant to the
same cell; if it needs new API, that becomes a ballot then.

**Kind:** answered + work-item (existing cards).

## 5. Fewer layers to assembly; replace LLVM; speed of light; memcpy

**Today in Jet**

- Ratified plan: one runtime and one Jet-written code generator, with O0 for
  `jet run`, O1 for `jet build`, and O2 for `--release`. O2 "uses LLVM first",
  and "a Jet-written O2 may come later" (D-EXEC1). The old JIT is retired
  (D-EXEC2). All back ends read one Jet-owned MIR (D-TIER-FORM1).
- In the tree: a native x86-64 back end written in Jet exists in
  `Compiler/JetBackend/Source/` (LIR, `X64/Select.jet`, `X64/RegAlloc.jet`,
  `X64/Encoder.jet`, ELF writer and linker in `Image/`). Today's shipping path
  still emits Rust and calls rustc, which uses LLVM
  (Docs/spec/architecture.md lines 12-16; Source/CmdCompile.rs).
- Your no-Rust directive (OWNER-DIRECTIVES 19) removes the rustc path, so
  LLVM would only remain if O2 called it directly.
- Seeing the assembly: `jet inspect code … --view disasm` is ratified
  (D-CODE-INSPECT1) but marked "not shipped". Compiler decisions are visible in
  `jet inspect decisions`.
- Copies: last uses move without a mark, shared values copy only on first write,
  text slices are views (D-COPY-DEFAULT1, D-FOUND-VIEW1, D-MEM-COPYSEM1).
  `jet audit copies` lists every copy. Copy reduction is a staffed goal
  (directive 22) with open cards #4318 (codegen may never add a copy sema did
  not decide), #4567 (destructuring copies fields), #4462, #4682, #4709.
- Speed of light: the performance gate requires beating every peer on every
  required cell (AGENTS.md "Performance gate"). No decision frames design as
  "work back from hardware limits", and none mentions tail calls.

**Gap**

- D-EXEC1 still plans LLVM for release builds; your note asks to consider
  replacing it.
- Docs/spec/architecture.md is stale: it still describes the Cranelift JIT and
  says "self-hosting does not mean replacing" rustc and LLVM.
- No guaranteed tail calls (see section 23).
- No "speed of light" budget: a stated hardware bound per benchmark cell
  (bytes moved, operations, memory bandwidth) to show how far Jet is from the
  limit, not only from Rust.

**Recommendation**

- Ballot `D-BACKEND-OWN1` (short): make Jet's own back end the O2 path too, and
  retire LLVM once the Jet O2 strictly beats it on the gate. LLVM stays only as
  a measured comparator until then.
- Doc-update (card `architecture-doc`): rewrite the architecture pipeline
  diagram to the D-EXEC1/D-EXEC2/no-Rust state.
- Fold "speed of light" into the gauntlet as a reported column: for each cell,
  the hardware bound and Jet's percentage of it. This is a reporting change on
  an existing manifest, so it belongs on the gauntlet card, not a ballot.
- Memcpy: keep driving the existing copy cards; #4318 is the structural fix.

**Kind:** decision-needed + doc-update.

## 6. Odin-style primitive types: quaternions, endian numbers, complex

**Today in Jet**

- *Complex:* ratified and built. `Complex` and `Fraction` are Core types with
  operators (D-NUMTYPE1); `3i` makes a complex literal (D-LITCARRIER1=D;
  `Compiler/JetSema/Source/Sema/Expressions/Literals.jet:161`).
- *Quaternion:* D-LITCARRIER1=D reserves `i`, `j` and `k` and shows
  `q :: 1 + 2i + 3j + 4k -> Quaternion`, but no `Quaternion` type exists (no
  match in Core, Compiler or crates).
- *Small floats:* `F16`, `BF16`, and 8-bit storage formats (D-LOWFLOAT1).
  Probe `probe/vecmath.jet` (first version): `F16{1.5}` fails E0108, and
  `print` cannot show an `F16` (E0112). So F16 is ratified but not usable yet.
- *Number families:* `Signed<bits>`, `Unsigned<bits>`, `IEEE<e, f>` with
  standard aliases (D-CORE-NUMFAMILY1).
- *Endian:* byte order lives only in codecs: `Bytes.write_u32_le`, the binary
  pack contract, and bit patterns (D-CORE-IO-BUF1, D-BINARY-CANONICAL1,
  D-BITS1). There is no `U32be` or `F64le` type.
- Probe finding: a bare `2.0` is a `Decimal`, so `2.0 + 3.0i` fails E0109
  ("Decimal and Complex"). This follows D-LITCARRIER1 (`3.4` defaults to
  Decimal) but is a beginner trap for complex math.

**Gap:** no quaternion type, no endian-fixed number types, F16 not usable,
and `Decimal + Complex` has no rule.

**Recommendation**

- Quaternion: work item (card `quaternion`). The reserved `j`/`k` suffixes
  already settle the spelling, so build `Quaternion` beside `Complex` in
  `core.math`, with `*` as the Hamilton product, `conj`, `normalize`,
  `from_axis_angle`, `rotate(Vec3)`, and `slerp`.
- Endian numbers: ballot `D-ENDIAN-TYPES1` (short). Recommended A: storage-only
  types `U16le`, `U32be`, `F64le` and so on, generated from the number families,
  usable in `#Layout(c)` and packed structs, that convert to the normal number
  on read. Arithmetic always happens on native numbers.
- F16 and the `Decimal + Complex` rule go on the quaternion card as related
  fixes, since they share the numeric-literal path.

**Kind:** answered (complex) + decision-needed (endian) + work-item.

## 7. Zero undefined behavior

**Today in Jet**

- Safe Jet is memory- and type-safe; unsafe work needs a written
  `#Unsafe("reason")` (invariant I1, AGENTS.md).
- Whole numbers do not overflow: `Int` grows; fixed widths stop with an error
  on overflow, and wrapping or saturating must be asked for (D-INTBIG1,
  D-WRAP-SCOPE1).
- `7 / 2` is `3.5`; whole-number division is `/%` (D-INTDIV1, D-FLOORDIV1).
- Number crossings that might lose a value are checked as they happen
  (D-NUMWIDEN-CROSS1). Bit casts are named functions (D-FLOAT-BITS1).
- No null: absence is `T?` (spec "Data types and methods").
- Bounds are checked; proven ranges remove the check (D-TYPE2-REFINE1).
- Every runtime stop is a registered report on every tier (D-FAIL-BREACH1).
- Inside `#Unsafe`, dev and hardened builds check range, liveness and alignment
  (D-MEM-SENTRY1, D-HARDENED1), and `jet inspect guarantees` reports what each
  component's safety rests on (D-MEM-GUARANTEE1).

**Gap:** there is no single table of "every operation and its defined result"
(division by zero, shift by the full width or more, float-to-integer of NaN or
infinity, enum from an unknown number, stack overflow, out of memory,
evaluation order). The spec has no such section. Odin's promise is exactly
that table. Jet must also make sure its own back end never emits code whose
meaning the CPU or LLVM leaves undefined.

**Recommendation:** work item (card `defined-behavior-ledger`): add one spec
section, "Defined behavior", listing every operation with its result on every
tier, plus one conformance test per row. Any row that is still undefined
becomes a bug. Inside `#Unsafe` the honest promise is narrower: misuse is
caught by the hardened profile rather than being undefined in practice.

**Kind:** answered + work-item.

## 8. Build settings in `fn build()`

**Today in Jet**

- `fn build(b: BuildContext) -> BuildPlan` registers executables, libraries,
  tests, assets and actions, and `b.toolchain(name, target_triple)` names a
  target (spec "Programmable builds").
- Settings are typed and declared in `package.jet`, with a default; profiles and
  the command line contribute values (D-CONF-KEY1, D-CONF-MERGE1).
  `fn build` may compute a value for a declared setting, recorded in the lock
  file (D-CONF-SPLIT1).
- Code reads facts at build time: `$build.os`, `$build.profile`,
  `$build.settings.x`, `$package.version` (D-BUILD-FACT3); `prep if $build.os
  == { … }` picks code per target.
- Per-CPU tuning: `#Multiversion(levels)` builds one copy per CPU level and
  picks the best at start-up; `jet build --cpu` fixes one level (D-CPU-DISPATCH1,
  D-CORE-FEATURE-FLOW1). Machine sizes stay `Int` in user code; C sizes are
  checked at the C edge (D-USIZE-TARGET1).
- `fn build` can set default flags, with the command line still winning
  (D-BUILDCTX-FLAGS1).
- GPU work is planned by the compute placement rules, not by build flags
  (D-COMPUTE-PLACE1).

**Gap:** a per-target setting value (for example `word_bits` that differs per
target in one build) is covered by the contribution law at the "target"
scope only implicitly [INFERENCE: D-CONF-MERGE1 lists item…package scopes and
profile/CLI layers, not a target layer].

**Recommendation:** answered. If you want per-target values written inside one
`fn build`, ask for a ballot; today the documented way is a target profile in
`package.jet` plus `$build` reads.

**Kind:** answered.

## 9. Multiple return values with names, without tuples

**Today in Jet**

- A function returns several named values with a named result list:
  `fn divmod(a: Int, b: Int) -> (quotient: Int, remainder: Int)`.
- Callers bind them by name, in any order: `(quotient, remainder) ::
  divmod(17, 5)` or read `r.remainder` (D-TUPLE-DESTRUCT1: binding is by member
  name only, never by position).
- Probe `probe/returns.jet` prints `3`, `2`, `1`.
- Public APIs return results rather than filling output parameters
  (D-OUTPUT-RETURNS1).

**Gap:** none in behavior. Jet calls this a "tuple", but because members are
bound by name it behaves like Odin's named results. (Odin also allows a bare
`return` that sends the named results back; Jet does not, and should not,
because the tail value rule already returns the last expression, D-TAIL-RETURN1.)

**Kind:** answered.

## 10. String versus string builder

**Today in Jet**

- One `String` type. Whether it can change is decided by the binding:
  `name :: "Ada"` can never change; `name := ""` can grow with `+=`
  (spec "Values, expressions, and calls"; D-STRING-EMPTY1).
- Text slices (`trim`, `after`, `before`, `lines`) are no-copy views
  (D-FOUND-VIEW1, spec "Zero-copy string views").
- Functions that return a `String` and are printed or appended write straight
  to the destination, so no throwaway string is built (D-EFF-FORMAT-WRITER1).
- `Bytes` is the growable byte buffer; the separate `StringBuf` and `BytesBuf`
  were deleted (D-CORE-IO-BUF1).

**Gap:** none in capability. Your rule, "strings fixed, builders grow", already
holds through the binding: a `::` string is fixed, a `:=` string is the
builder, and the compiler reuses its storage. A second type would make every
API choose between two text types.

**Recommendation:** answered; keep one type. If you still want a separate
builder type after reading this, say so and I will draft the ballot (it would
reopen D-EFF-FORMAT-WRITER1 and D-CORE-IO-BUF1).

**Kind:** answered.

## 11. Pointer classes, and `~` as "not" with `~=` and `&~`

**Today in Jet**

- Pointer classes exist exactly as you listed, behind `#Unsafe`: `mem.Ptr<T>`
  is never null, and `?mem.Ptr<T>` is the nullable form whose null is `None`
  (D-RAWPTR2). Safe code uses views, `&` write windows and `@T` links instead of
  pointers (D-MEMREF1).
- Bit operators: on 2026-10-04 you voted that bit arithmetic and power become
  methods (`flags.bit_and(mask)`, `x.pow(2)`), so `&`, `|`, `!` and `^` keep
  only their everyday jobs (D-SIGIL-ALGEBRA1=B). That replaced the earlier
  `!` bit-not and `~|` xor (D-BITNOT1, D-XORSPELL1).
- Copy is now `x.copy()` (D-COPY-WORD1), so `~` is free. The open ballot
  D-SIGIL-FREED1 asks what `~` and `^` should mean next. Its option E is
  C-family bits (`~x` not, `a ^ b` xor).

**Gap:** your note describes Odin's set (`~x` not, `a ~ b` xor, `~=`, `&~`
and-not), which is not one of D-SIGIL-FREED1's options and conflicts with the
10-04 vote for named methods.

**Recommendation:** decide it inside the open D-SIGIL-FREED1 rather than in a
new ballot. I drafted an extra option, "F: Odin bit set with `~`", in
`ballots/D-SIGIL-FREED1.option-F.json` for Main to add. It keeps the named
methods for readers who prefer words and gives `~` its Odin meanings. If you
pick F, D-SIGIL-ALGEBRA1 is amended for `~` only.

**Kind:** answered (pointers) + decision-needed (`~`).

## 12. Why Odin avoids move semantics

**Today in Jet**

- Odin has no destructors, so it has nothing to move: values are plain bytes,
  and cleanup is `defer` plus explicit allocators. As I understand its
  author's public writing (not re-checked in this session), the objections are
  hidden control flow (a destructor runs where nothing is written), the
  "rule of five" complexity C++ grew, and unclear validity of moved-from values.
- Jet needs ownership because it promises memory safety without a garbage
  collector (I1). It keeps the cost out of sight: a last use moves with no mark,
  shared values copy only when written, and `x.copy()` is the rare explicit copy
  (D-COPY-DEFAULT1, D-COPY-WORD1, D-CALLSITE-MARK1). Cleanup is visible through
  `defer`, `#MustFinish(commit, rollback)` and a fixed field order
  (D-OWES1, D-FIELD-DROP-ORDER1).
- The open D-COPY-MOVE-SPELL1 decides how an early move is written.

**Gap:** none; this is a deliberate difference. Odin's criticism is real for
C++, where moves are user-written constructors that can do anything. In Jet,
moves are compiler-generated and never run user code, which removes the hidden
control flow Odin objects to.

**Kind:** answered.

## 13. Clean casts, maybe with `<>`

**Today in Jet**

- There are no casts. A conversion is named by its destination:
  `Target.from_int(x)`, `Int.parse(text)` (D-SHAPE-CONVERT1, syntax-decisions
  "no `as`, cast punctuation, or source-owned `to_*` aliases").
- `Type{value}` is the one typed head, and with one value of another type it
  performs the checked conversion: `Float{rows()}` (D-DOTCTOR3 amendment).
- Lossless widening is automatic, including literals (D-INTLIT-WIDTH1=F,
  D-NUMWIDEN-CROSS1=E). Bit reinterpretation is a named function
  (D-FLOAT-BITS1). Raw pointer reinterpretation is `p.cast<U>()` inside
  `#Unsafe` (D-RAWPTR2).
- Generics no longer use `<>` (D-GENERIC-TYPEFN1), and the open D-ANGLE-PAIR1
  asks what freed angle brackets should mean; it recommends keeping them as
  comparisons only.

**Gap:** none in capability. A `<T>value` cast would be a second spelling of
`T{value}` (against "one mechanism", philosophy priority 4) and would make `<`
ambiguous again.

**Recommendation:** answered. `T{value}` is the clean cast. I suggest Main add
"casts" to D-ANGLE-PAIR1's list of proposed uses, with this reason, so your
idea is recorded where that vote happens.

**Kind:** answered.

## 14. Immutables are compile-time constants

**Today in Jet** (verified by probe `probe/alias.jet` and spec)

- At file level, `NAME :: value` is folded while building and is a true
  compile-time constant; it must be literal data (E0622) and ALL_CAPS (E0357).
  Computed constants use `NAME :: prep { … }` (spec "Compile-time names and
  staged syntax"; D-PREP-SURFACE2).
- Small constants are copied into each use; lists and maps are built once
  (spec same section). `const` is retired (D-CONST-RETIRE1).
- Inside a function, `x :: value` is an immutable *run-time* binding: it can
  hold a value computed from inputs, and it is folded when its value is
  constant. Parameters are read-only by default (D-PARAM-CONST1).

**Gap:** one deliberate difference from Odin. Odin's `::` always means
compile-time; a local Odin value that depends on inputs must be a variable.
Jet uses `::` for "never changes", which is wider and safer.

**Recommendation:** answered. The word for compile-time code is open in
D-COMPTIME-WORD1 (`prep` versus `comptime`). Note that section 17's ballot
would let file-level `::` also name types and declarations, which brings Jet
closer to Odin's "everything at file level is a constant" model.

**Kind:** answered.

## 15. Implicit type inference

**Today in Jet**

- Locals are inferred; the type rides the value (`limit :: U8{250}`), and
  writing a type on the name is a teaching error with an automatic fix
  (D-BIND-TYPE2).
- Literals take the expected type (D-LITCARRIER1=D), `None` and `.new(…)` take
  it from context, and generic calls infer their type arguments
  (D-GENERIC-CALL1, D-GENERIC-TYPEFN1).
- Failure types of a function with no written contract are inferred
  (D-FAIL-INFER-UNION1); effects are inferred.
- Signatures (parameter types, and results that callers depend on) stay
  written.
- Open: D-INFER-LEVEL1 asks whether a local's type may be settled by a later
  line in the same function (`names := []` then `names.push("Ada")`). It
  recommends yes, inside one function only.

**Where it should not be allowed:** public signatures, struct fields and
module constants that cross files. These are the contract that readers, tools
and fast incremental builds rely on (D-INCR-UNIT1 dirties dependents only when
an interface changes).

**Kind:** answered (the open ballot decides the remaining case).

## 16. Enum backing integer type; exhaustive enum matching

**Today in Jet**

- Exhaustive matching is enforced. Probe `probe/enums.jet` (first version):
  a missing case is `E0307: This 'if' doesn't cover every case — missing:
  East`. Guarded arms do not count toward coverage (D-MATCH-GUARD1), an `else`
  after every case is listed is rejected (L0303), and multi-head functions use
  the same proof (D-CHOOSE-HEADS1). Adding a case breaks every incomplete
  table on purpose (D-ENUM-EVOLUTION1).
- Numbered cases parse: `enum Mode { Read = 1  Write = 4 }`, and the numbers
  may be computed at build time (D-META-CONST1). A checked `Mode.from_int(n)`
  is ratified (D-ENUM-INGRESS1), and all values can be read as data
  (D-ENUM-CASEVALUES1).
- Probe: `Mode.from_int(4)` is not available yet (E0107), and there is no
  `to_int`. Packed structs can store enums at a bit width (D-BITS1).
- There is no way to choose the backing integer type of an enum.

**Gap:** choosing the backing type (Odin's `enum u8`), the two conversions not
built, and one remaining softness: a caller may still write `else` instead of
listing cases, which hides new cases. That is the ratified choice
(D-ENUM-EVOLUTION1), not a bug.

**Recommendation**

- Ballot `D-ENUM-BACKING1` (short): `#Layout(U8) enum Mode { Read = 1 }`, using
  the existing layout marker, for payload-free enums only; it also fixes the
  conversions as `Mode.from_int` and `mode.to_int()`.
- If you want "always list every case" to be stricter (forbid `else` on enum
  tables), that reopens D-ENUM-EVOLUTION1, so I did not draft it. Say if you
  want one; the natural form is a package policy key, not a new rule for all.

**Kind:** answered (exhaustive) + decision-needed (backing type).

## 17. Remove the `alias` keyword; alias by `New :: Old`

**Today in Jet**

- `alias Name<T> :: …` is a transparent alias, allowed only to shorten
  generic spellings (D-TYPEALIAS1 as amended by D-ALIAS-OP1=B;
  `crates/jet-foundation/src/Syntax/core_surface.rs:423-426`).
- New nominal types use `Usd :: distinct Decimal` (D-DIST1), so `Name :: …`
  already declares a type in one case.
- Probe `probe/alias.jet`: `alias Rows :: [[String]]` fails E0324 ("only for
  shortening generic spellings"), and `Grid :: [[Int]]` fails E0357 (must be
  ALL_CAPS) and E0622 (must be a literal).
- Generic types are becoming build-time recipes, `prep fn Pair(T: Type) ->
  Type` (D-GENERIC-TYPEFN1), so types are build-time values.
- Core itself uses `alias U8 :: Unsigned<8>` (D-CORE-NUMFAMILY1).

**Gap:** your note asks for one rule, `New :: Old`, with no keyword.

**Recommendation**

- Ballot `D-ALIAS-BIND2` (full, because it removes a keyword). Recommended A:
  at file level, `Name :: <type>` declares a transparent alias, `Name ::
  distinct <type>` declares a new type, and the `alias` keyword is removed with
  an automatic fix. The case rule tells them apart from constants: types are
  PascalCase. It needs its two fresh reader passes before Main submits it.
- The re-export ballot `D-VIS-REEXPORT1` is finished (see "Re-export ballot"
  below). Its option A already uses your rule for re-exports
  (`clamp :: core.math.clamp`). The two ballots fit together: if both pass,
  every alias in Jet is `New :: Old`.

**Kind:** decision-needed.

## 18. Variadic `min`, `max`, `clamp` in the prelude

**Today in Jet**

- Probe `probe/prelude_minmax.jet`: `min(3, 1)` is E0102 "Nothing named `min`
  exists here"; `3.min(1)` and `12.clamp(0, 10)` are E0311.
- `core.math.min`, `max` and `clamp` exist for two `Int`s only, with
  `min_float`, `max_float` and `clamp_float` for floats
  (`Core/math/math.jet:21-34, 390-395`).
- Ratified: one `math.max` for every number type through a `Numeric` trait
  (D-TRAIT-OVERLOAD1); card #4659 (ready) makes them generic over
  `Comparable` with a fixed NaN rule.
- The prelude is a closed list; adding a name needs a ballot and must pass seven
  tests (D-CORE-PRELUDE1, D-PRELUDE-LAW1).

**Recommendation:** ballot `D-PRELUDE-MINMAX1` (short). Recommended A:
`min(a, b, …)`, `max(a, b, …)` and `clamp(x, lo, hi)` join the prelude, generic
over `Comparable`, at least two arguments for min and max, and the same
functions stay in `core.math`. A list uses `list.min()` (already there).

**Kind:** decision-needed.

## 19. Built-in array math and swizzling

**Today in Jet**

- Built in, no import: `Vec2`, `Vec3`, `Vec4`, `Mat3`, `Mat4` with `+`, `-`,
  `*`, `/`, `dot`, `cross`, `matmul`, `transpose`
  (`crates/jet-foundation/src/Syntax/math_layout.rs:112-122`, D-LINALG1,
  D-MATHLIB1).
- Swizzling is ratified and built: read `v.zyx`, write `v.xy = …`, with errors
  for bad lanes (E3110) and repeated write lanes (E3111) (D-SWIZZLE1;
  `Compiler/JetCodegen/Source/Codegen/Expressions.jet:6375`).
- `Matrix<M, N>` and `Vec<N>` literals, broadcasting and `*` meaning are
  ratified (D-MATRIX-LIT1, D-MATRIX-BCAST1, D-MATRIX-MUL1); arithmetic needs no
  import, solving needs `use core.linalg` (D-MATRIX-HOME1).
- SIMD lanes `Lanes<T, N>` and native-width `Lanes<T>` (D-LANES1), automatic
  vectorization by default (D-SIMD3). Mixed scalar/vector operators follow one
  rule (D-OPMIX1). Typed coordinate spaces (D-SPACE-GEOMETRY1).

**Gap: one bug.** Probe `probe/vec2.jet` on `jet run`: `a.x` works, but the
result of `a + a` cannot be used: `b.dot(a)` stops with `panic: dot: bad
recv`, and `c.x` stops with `panic: lane: bad recv` (`probe/vecmath.jet`).
Vector arithmetic results are broken on the run path.

**Recommendation:** card `vec-math-bug` (P0). The feature set you asked for is
already decided; it needs this fix and a golden example covering
`+`, swizzle, `dot` on results.

**Kind:** answered + work-item.

## 20. CPU/GPU split compilation, a portable GPU kernel library, and pushing code to the GPU

**Today in Jet** (decisions)

- `core.compute` is the one owner of arrays, fusion, autodiff and devices,
  with an expert device tier beneath it (D-COMPUTE1, D-COMPUTE-TYPE1).
- Placement is automatic by default, with receipts for every transfer, and
  experts can pin device and precision (D-COMPUTE-PLACE1).
- A safe kernel is an ordinary Jet function marked `#Kernel(.parallel)`; the
  compiler proves bounds, races and barriers (D-COMPUTE-KERNEL-SURFACE1,
  D-COMPUTE-KERNEL1). It is launched with an ordinary call,
  `compute.launch(add, inputs: (xs, ys))` (D-GPU-COMPUTE-LAUNCH1). Graphics
  stages use `#Shader(.vertex)` (D-GPU-SHADER-SURFACE1). Group memory,
  atomics, lane exchange, tiles and helpers have decisions (D-GPU-*-SCOPE1).
- One portable contract over CPU SIMD, CUDA, Metal, Vulkan and WebGPU, with a
  CPU reference result and typed capability checks such as tensor cores
  (D-COMPUTE-BACKEND1). Raw vendor kernels need a provider-issued proof and
  `#Unsafe` (D-COMPUTE-RAWBOUNDARY1). GPU compilers ship as optional components
  (D-GPU-PACKAGING1).

**Today in Jet** (code)

- `Core/compute/compute.jet` has `Tensor` and named devices
  (`device_cuda`, `device_metal`, `device_vulkan`, `device_webgpu`).
- I found no GPU code generator: no SPIR-V, PTX or Metal emitter in
  `Compiler/`, and no GPU crate in the Cargo manifests. [INFERENCE] compute
  operations run on the CPU today.
- Cards #3026 (user-authored kernels) and #3028 (GPU provider matrix and real
  dispatch) are ready but not started.

**Gap:** the design matches Mojo's split (same source, compiler decides CPU or
GPU), but nothing runs on a GPU yet, and there is no first-party kernel library
equivalent to Mojo's.

**Recommendation:** work item (card `gpu-kernel-library`), after #3026 and
#3028: a portable kernel library in `core.compute` written as `#Kernel` Jet
code (map, reduce, scan, sort, matrix multiply, convolution, attention), each
checked against the CPU result, each with a gauntlet cell against the vendor
library. The beginner path stays `compute.launch` or plain tensor operations;
no new syntax is needed.

**Kind:** answered (design) + work-item.

## 21. Portable `sin`, and Jet-defined math

**Today in Jet**

- `math.sin` calls a compiler built-in, `core.math.sin`
  (`Core/math/math.jet:122`), which ends in the host's math library
  [INFERENCE from the Rust runtime path]. Results can differ by a last bit
  between systems.
- Chip-specific instructions live in Core as typed entries with a plain Jet
  body (D-CORE-BOUNDARY1), so Jet already has the pattern "define it in Jet,
  let the chip speed it up".
- The GPU contract demands a CPU reference result (D-COMPUTE-BACKEND1).

**Gap:** the elementary functions (`sin`, `cos`, `exp`, `log`, `pow`) are not
defined in Jet, so results depend on the platform and cannot be the CPU
reference that GPU results are checked against.

**Recommendation:** work item (card `portable-math`): write the elementary
functions in Jet in `core.math` with a stated accuracy (correctly rounded where
the cost allows, as the CORE-MATH project does), and test that every tier and
target gives the same bits. Fast, less accurate versions stay available as a
named compute precision profile, never silently. This changes results, not
APIs; if it ever needs new names, that becomes a ballot.

**Kind:** work-item.

## 22. A WASM replacement that exposes the host's hardware

**Today in Jet**

- Ratified direction: Jet's own sandbox replaces Docker and WebAssembly. The
  bundle holds verified, portable Jet code; the host checks it, grants each
  power, and *compiles it locally*; foreign code runs in a walled-off helper
  (D-PORTABLE1). Card #4195 (ready) designs the format.
- Because the host compiles locally, the bundle can use everything the host
  chip has: CPU copies per level (D-CORE-FEATURE-FLOW1) and GPU capability
  negotiation (D-COMPUTE-BACKEND1). That is the answer to WebAssembly's lowest
  common denominator.
- Older decisions still use Wasmtime for plugins (D-DEP-WASM1) and keep
  `wasm32-wasip2` as a server target (D-WASISRV1).

**Gap:** no decision says how portable code *sees* what the host offers (for
example "tensor cores present, 24 GB device memory, AVX-512") and what happens
when a bundle needs something the host lacks.

**Recommendation:** ballot `D-HOST-CAPS1` (short). Recommended A: the host's
hardware becomes build facts when the host compiles the bundle
(`$build.cpu`, `$build.gpu`), the bundle declares what it requires and what it
can use in its manifest, and a missing required capability is refused before
running with a named reason. No run-time probing API. Also: reconcile
D-DEP-WASM1 with D-PORTABLE1 on card #4195.

**Kind:** answered (direction) + decision-needed.

## 23. RAII, earlier cleanup, and tail recursion

**Today in Jet**

- A value is cleaned up at the end of its scope, in reverse order, unless it
  was moved first (`Compiler/JetCodegen/Source/Codegen/DropFlags.jet` header:
  "execution order and drop timing are unchanged"; D-FIELD-DROP-ORDER1 "at
  scope exit").
- A last use moves the value away (D-COPY-DEFAULT1), so a moved value is freed
  by whoever received it, but a value that is merely no longer used waits for
  the scope end.
- `defer`, failure-only cleanup `scope.on_fail` (D-DEFER-FAIL1), and
  `#MustFinish` (D-OWES1) exist.
- No decision mentions tail calls (searched all decisions).

**Gap:** exactly your note. A large value that is dead halfway through a
function still holds its memory until the scope ends. Pending cleanups also
block tail calls: a recursive call in tail position cannot reuse the frame
while cleanup is still owed, so deep recursion grows the stack and keeps memory
alive. Mojo frees each value right after its last use for this reason.

**Recommendation:** ballot `D-DROP-ASAP1` (short). Recommended A: memory-only
values are freed right after their last use; values with visible cleanup
(`Close`, `#MustFinish`, locks) keep the scope-end rule so side effects stay
where readers expect them; and a call in tail position is guaranteed not to
grow the stack. Jet already computes last uses, so the cost is in the
compiler, not in user code.

**Kind:** decision-needed.

## 24. Classes versus structs

**Today in Jet**

- There is no `class` and no inheritance. Data is `struct` and `enum`;
  behavior is methods in the type, in `impl`, or `fn Type.method`; shared
  behavior is a `trait`; a trait in type position gives dynamic dispatch (spec
  "Data types and methods"). Keywords: `crates/jet-foundation/src/Syntax/
  core_surface.rs:418-471` and `math_layout.rs:701`.

**Answer:** classes bundle three things: data layout, behavior, and a type
hierarchy with dynamic dispatch. Jet splits them, so each is chosen on purpose.
You never pay for a hidden vtable or a hidden heap object, and closed enums
plus exhaustive matching replace most inheritance trees. Jet does not need
classes.

**Kind:** answered.

## 25. Unity build versus compilation units

**Today in Jet**

- Within one package the compiler may split the code into hidden units; a
  function body edit recompiles one unit and relinks (D-BUILD-UNITS1).
- Edits dirty a module only when its interface changes; packages are sealed
  objects reused from a cache (D-INCR-UNIT1, D-LIB-REUSE1, D-BUILD-STORE1).
- Generic and inlinable code lives in each unit's interface part, so it is
  still optimized across units.
- Release builds already use link-time optimization across the program and
  its runtime (architecture.md R8).
- Cards: #4155 (output proportional to source, separate compilation) and #4310
  (parallel compilation on all cores).

**Answer:** Jet takes the best of both. A unity build gives the optimizer the
whole program and avoids repeated header parsing; its costs are slow
incremental rebuilds and no parallelism. Jet has no headers to re-parse, sees
the whole package in its checker, and compiles hidden units in parallel, then
optimizes across them at release. So debug edits stay fast and release builds
keep whole-program speed.

**Kind:** answered.

---

## Re-export ballot (`D-VIS-REEXPORT1`)

File: `~/.cache/jet-dev/scratch/ballots/D-VIS-REEXPORT1.json` (full profile,
still `draft: true`; Tower's own readiness check reports zero gaps when run
read-only through `OwnerNotes1/validate.mjs`).

- I checked all 14 beginner friction rows (history://ReexportBeginner) and all
  9 adversarial findings plus the citation audit (history://ReexportAdversary)
  against the draft. Most were already repaired. I closed the rest: the lesson
  now shows today's constant `LIMIT :: 10`; option B's loss no longer uses the
  untaught word "package"; the alias mitigation says "plain dotted name";
  option A's technical note states that `::` inside a function is unchanged and
  how generic recipes are aliased; the TS2308 citation was re-verified at the
  TypeScript source.
- `reviewPasses` records both readers (beginner: ReexportBeginner, rli5;
  adversarial: ReexportAdversary).
- Relation to your alias note: option A *is* your `New :: Old` rule, applied to
  re-exports. The new `D-ALIAS-BIND2` extends it to type expressions and removes
  the keyword. The detail field explains how they combine.
- Ready for Main to submit with `tower decision add` then `--ready`.

## What Main should do next

1. Submit `D-VIS-REEXPORT1` and the short ballots after a quick read.
2. Run the two fresh reader passes for `D-ALIAS-BIND2` (full) before
   submitting it.
3. Add option F to the open `D-SIGIL-FREED1` and the "casts" note to
   `D-ANGLE-PAIR1`.
4. Turn the card drafts into Tower cards; the two P0 bugs (`union-switch-bugs`,
   `vec-math-bug`) first.

## Draft files

Cards (`~/.cache/jet-dev/scratch/OwnerNotes1/cards/`), P0 first:

1. `union-switch-bugs.json` (P0; owned by P0Fix1)
2. `vec-math-bug.json` (P0; owned by P0Fix1)
3. `defined-behavior-ledger.json` (P1)
4. `cache-profiler.json`
5. `design-guide.json` (doc, needs owner acceptance)
6. `architecture-doc.json` (doc)
7. `quaternion.json`
8. `gpu-kernel-library.json` (blocked by #3026, #3028)
9. `portable-math.json`

Ballots (`~/.cache/jet-dev/scratch/ballots/`). Each passes Tower's own
`ballotGaps` check, run read-only with `OwnerNotes1/validate.mjs`, except where
noted:

- `D-VIS-REEXPORT1.json` (full, readers recorded, 0 gaps)
- `D-ALIAS-BIND2.json` (full; only gaps are the two reader passes still owed)
- `D-PRELUDE-MINMAX1.json` (short, card #4659)
- `D-DROP-ASAP1.json` (short, card #4620)
- `D-BACKEND-OWN1.json` (short, card #4013)
- `D-CACHE-CONTROL1.json` (short; card is the `cache-profiler` draft, so create it first)
- `D-ENUM-BACKING1.json` (short, card #3142)
- `D-ENDIAN-TYPES1.json` (short; card is the `quaternion` draft, so create it first)
- `D-HOST-CAPS1.json` (short, card #4195)
- `D-SIGIL-FREED1.option-F.json` (an extra option for the open D-SIGIL-FREED1, not a new ballot)
