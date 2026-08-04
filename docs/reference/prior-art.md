# Prior Art

Every external source Jet has learned from: the languages, tools, ecosystems,
papers, talks, and creator retrospectives that shaped its semantics, syntax,
philosophy, and ecosystem plans. This is a provenance record — a trace from
today's design back to its inputs.

**How to read this.** Jet's rule is "take the invariant, not the surface
syntax": copy the successful idea, understand the constraint that produced it,
and decline the historical baggage. Most entries below are therefore listed
with *what Jet drew from them* — a feature adopted, a mistake avoided, or a
choice deliberately rejected.

**Where the deep synthesis lives.** This page indexes sources. The reasoned
analysis of each one lives in Jet's own primary documents:

- `docs/archive/language-lessons-and-regrets.md` — the master lineage study
  (2026-07-15): 30+ languages, each with creator regrets, official
  retrospectives, and Jet's defensive position.
- `docs/archive/language-shape-research.md` — surface-shape research mining
  other languages for concrete syntax/semantics transplants (archived).
- `docs/proposals/ecosystem-shape.md` — package/config/ecosystem research.
- `docs/plans/epoch-3/universal-language-core.md` — web/UI/notebook/numeric
  reach research.
- `docs/plans/epoch-4/world-class-package-manager.md` — package-manager and
  supply-chain research.
- `docs/research/*` — active dated deep-dive reports; finished mines live under
  `docs/archive/`.

---

## Videos, talks & podcasts

Mined in full with transcript + comment analysis (see `docs/research/`,
`docs/archive/`, and the `mine-video` skill).

**Logan Smith — Rust series (9 videos, mined 2026-07-24).** External validation
of Jet's ratified safety/error/ownership design; report at
`docs/archive/2026-07-24-logan-smith-rust-series-mining.md`.

- 5 Strong Opinions On Everyday Rust — https://www.youtube.com/watch?v=8j_FbjiowvE
- Constructors Are Broken (build-then-construct-valid; "named constructor" not "factory") — https://www.youtube.com/watch?v=KWB-gDVuy_I
- Moves Are Broken (destructive moves + strong invariants) — https://www.youtube.com/watch?v=Klq-sNxuP2g
- Comprehending Proc Macros (bounded metaprogramming; why Jet rejects arbitrary syntax) — https://www.youtube.com/watch?v=SMCRQj9Hbx8
- Two Ways To Do Dynamic Dispatch (auto-box vs monomorphize; no user-facing `dyn`) — https://www.youtube.com/watch?v=wU8hQvU8aKM
- A Simpler Way to See Results (typed errors as values) — https://www.youtube.com/watch?v=s5S2Ed5T-dc
- Use Arc Instead of Vec (why Jet does *not* default to `Arc`) — https://www.youtube.com/watch?v=A4cKi7PTJSs
- Rust Functions Are Weird (But Be Glad) — https://www.youtube.com/watch?v=SqT5YglW3qU
- Choose the Right Option (`Option<&T>` not `&Option<T>`) — https://www.youtube.com/watch?v=6c7pZYP_iIE

**Logan Smith — "Verse: A New Scripting Language? In THIS Economy?"** (mined
2026-07-24). Crash course on Epic's Verse; source of the transactional-rollback
"watch" lesson and strategic validation of the Epoch-6 Canvas bet. Report at
`docs/archive/2026-07-24-verse-video-mining.md`.
https://www.youtube.com/watch?v=ebqKYLKjL6U

**Cross-facet polish batch (11 videos, mined 2026-08-03).** These sources
checked Jet's compiler seams, memory safety, runtime, layout, module, tooling,
environment, onboarding, UI, UX, and DX. The mine kept small polish cuts as
well as large design lessons.

Resulting work: [reflection, layout, and generic-call surface research](../research/reflection-layout-and-generic-call-surface-research.md),
Tower cards #1388 (`inspect expand --json`), #1389 (`inspect unsafe`
diagnostics and locations), #1390 (`layout` facts), and #1391 (final S33
reconciliation), with ratified decisions D-LAYOUT-FACTS1=B and
D-GENERIC-CALL1=A.

- lolzdev — Making my own programming language: keep keyword tables simple;
  preserve sema and TIR; reject unsafe-by-default design.
  https://www.youtube.com/watch?v=6lXZCOXCRME
- Tsoding Daily — Writing Garbage Collector in C: mark before traversal;
  prefer exact roots and edges over conservative stack scans.
  https://www.youtube.com/watch?v=2JgEKEd3tw8
- CsMadeEz — Goroutines Are NOT Threads: keep M:N scheduling, bounded work,
  known task exits, cancellation, and clear concurrency facts.
  https://www.youtube.com/watch?v=vfrAX26cqtg
- Cache Miss — Why Your C++ Struct Is Bigger Than It Should Be: keep explicit
  C and columnar layouts; measure locality instead of sorting fields blindly.
  https://www.youtube.com/watch?v=w50ofEmhRoc
- Adumh00man — Dendritic Nix is the Best Way to Configure a System: keep one
  reproducible graph with one-file defaults and optional module structure.
  https://www.youtube.com/watch?v=buxopFR4VXQ
- Semicolon — Rustc Commands Every Rust Developer Should Know: make errors,
  compiler facts, profiles, and targets easy to inspect without leaking rustc.
  https://www.youtube.com/watch?v=-DaVwuQeQD0
- Adumh00man — Installing Nixos on my Server! (500 ish sub special): preview
  destructive work; preserve locked bootstrap, recovery, driver, and secret state.
  https://www.youtube.com/watch?v=E3G2tl0GAb0
- Indently — `__init__.py` Explained in Just 7 minutes: keep short explicit
  module facades and reject import-time execution.
  https://www.youtube.com/watch?v=xn7nZLWXYSg
- DistroTube — The Age Of Beginner Friendly Distros Is Over: judge beginner
  UX by install, maintenance, recovery, hardware, docs, and offline states.
  https://www.youtube.com/watch?v=A6XI_0DWQOw
- Code to the Moon — 10 Underrated Rust Features & Patterns: keep exclusive
  mutation, bounded structured work, explicit modules, and uncolored tasks.
  https://www.youtube.com/watch?v=7QwqShxyHtc
- Let's Get Rusty — How unsafe Rust made Polars 30x times faster than Pandas:
  keep audited unsafe internals behind safe APIs; require measured attribution.
  https://www.youtube.com/watch?v=l6tisoOzTuk

**Visually Explained — Python mechanisms (4 videos, mined 2026-08-03).**
Stepwise visual explanations checked Jet's source-local transformations,
pull-driven streams, typed variadics, and JSON boundaries. Jet keeps one typed
mechanism for each job and rejects Python's dynamic baggage.

- Python Decorators - Visually Explained: keep transformations visible and
  inspectable; reject a second runtime decorator mechanism.
  https://www.youtube.com/watch?v=3tyaO-OE0K0
- Python Generators - Visually Explained: teach suspend/resume and one-pass
  streams; reject memory claims until Jet's thread-backed stream is measured.
  https://www.youtube.com/watch?v=GWZf_B129zs
- Python *args vs **kwargs - Visually Explained: use explicit typed parameters
  by default and typed variadics only for genuine open arity.
  https://www.youtube.com/watch?v=FFpDsC6B2qw
- JSON in Python - Visually Explained: preserve missing versus explicit null;
  reject object-only JSON roots and direct file truncation as safe defaults.
  https://www.youtube.com/watch?v=4rmBOxn0PdI

**Jonathan Blow on Jai** (The Standup w/ ThePrimeagen, transcript) — closed-beta
migration discipline; staged spelling migration through coexistence → warning →
removal → changelog. https://podscripts.co/podcasts/the-standup-with-theprimeagen/legendary-game-dev-jonathan-blow

**Onboarding reference** (Tower card note) — https://youtu.be/OPuztQfM3Fg

---

## Programming languages

### Systems & safety languages

**C** — predictable representation, direct FFI, freestanding reach; avoid:
unsafe-by-default, NUL-terminated strings, array-to-pointer decay, `_s` parallel
APIs, textual headers.
- WG14 N2659 (ordinary C is unsafe) — https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2659.htm
- WG14 N1990 / N2660 / N3360 (recover array bounds) — https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1990.htm · https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2660.pdf · https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3360.htm
- WG14 N1967 (Annex K bounds-checking failure) — https://open-std.org/jtc1/sc22/wg14/www/docs/n1967.htm
- WG14 N1400 / N2896 (preprocessor/header semantics) — https://www.open-std.org/JTC1/SC22/wg14/www/docs/n1400.htm · https://open-std.org/jtc1/sc22/wg14/www/docs/n2896.htm
- WG14 N1254 / N2885 (integer promotion/overflow) — https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1254.htm · https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2885.pdf
- "The Most Expensive One-byte Mistake" (ACM Queue) — https://queue.acm.org/detail.cfm?id=2010365

**C++** — zero-cost abstraction, deterministic lifetime, generics, native
interop; reject: inheritance, implicit narrowing, C-style casts, raw array
decay, dangling views, no-common-ABI.
- Stroustrup interviews: Slashdot / DevX / Italian — https://www.stroustrup.com/slashdot_interview.html · https://www.stroustrup.com/devXinterview.html · https://www.stroustrup.com/italian_interview.html
- P2771R1 (memory-safety views) — https://isocpp.org/files/papers/P2771R1.html
- CppCoreGuidelines view-lifetime issue #2276 — https://github.com/isocpp/CppCoreGuidelines/issues/2276
- mp-units — points & quantities (affine space) — https://mpusz.github.io/mp-units/latest/tutorials/affine_space/points_and_quantities/

**Rust** — ownership, deterministic cleanup, exhaustive ADTs, strong
diagnostics, Cargo's integrated workflow; watch: learning-curve accidental
detail, async-as-parallel-surface, compile time, orphan-rule glue friction,
build-script supply chain, feature unification.
- The Rust I Wanted Had No Future (Graydon Hoare) — https://graydon2.dreamwidth.org/307291.html
- 2024 lang roadmap / 2024 State of Rust / 2025 compiler-perf survey — https://blog.rust-lang.org/inside-rust/2022/04/04/lang-roadmap-2024/ · https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/ · https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results/
- Async project goals 2024h2 / 2026 roadmap — https://rust-lang.github.io/rust-project-goals/2024h2/async.html · https://rust-lang.github.io/rust-project-goals/2026/roadmap-just-add-async.html
- Relaxing the orphan rule — https://rust-lang.github.io/rust-project-goals/2024h2/Relaxing-the-Orphan-Rule.html
- RFC 2451 re-rebalancing coherence — https://github.com/rust-lang/rfcs/blob/master/text/2451-re-rebalancing-coherence.md
- Little Orphan Impls (Niko Matsakis) — https://smallcultfollowing.com/babysteps/blog/2015/01/14/little-orphan-impls/
- Cargo: build scripts / features / workspaces / resolver — https://doc.rust-lang.org/stable/cargo/reference/build-scripts.html · https://doc.rust-lang.org/cargo/reference/features.html · https://doc.rust-lang.org/cargo/reference/workspaces.html · https://doc.rust-lang.org/nightly/cargo/reference/resolver.html
- crates.io malware postmortem — https://blog.rust-lang.org/inside-rust/2023/09/01/crates-io-malware-postmortem/
- Cargo issues #14414 / #8088 — https://github.com/rust-lang/cargo/issues/14414 · https://github.com/rust-lang/cargo/issues/8088
- The Book: ownership ch04 / enums ch06 / patterns ch19-01 — https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html · https://doc.rust-lang.org/book/ch06-00-enums.html · https://doc.rust-lang.org/book/ch19-01-all-the-places-for-patterns.html
- API Guidelines: predictability — https://rust-lang.github.io/api-guidelines/predictability.html
- struct/impl colocation study (`docs/archive/2026-07-24-rust-struct-impl-colocation.md`): Clippy `multiple_inherent_impl` — https://github.com/rust-lang/rust-clippy/blob/master/clippy_lints/src/inherent_impl.rs · Clippy #6446 explicit-drop — https://github.com/rust-lang/rust-clippy/issues/6446 · Canonical ordering discipline — https://canonical.github.io/rust-best-practices/ordering-discipline.html · PingCAP trait style — https://pingcap.github.io/style-guide/rust/traits.html · users.rust-lang style thread — https://users.rust-lang.org/t/a-question-of-style-for-impl-of-structs/118029

**Zig** — explicit control, freestanding, comptime, expected-type shorthand,
allocator visibility, integrated build; watch: release-mode safety-off, removed
`usingnamespace`, inferred-error-set instability, pre-1.0 churn, unresolved
async.
- Language reference (illegal behavior / inferred error sets / build system) — https://ziglang.org/documentation/master/
- Issue #20663 (removed `usingnamespace`) / #5913 (async proposal) — https://github.com/ziglang/zig/issues/20663 · https://github.com/ziglang/zig/issues/5913
- 0.10 release notes / release-month expectations / 0.11 postponed — https://ziglang.org/download/0.10.0/release-notes.html · https://ziglang.org/news/what-to-expect-from-release-month/ · https://ziglang.org/news/0.11.0-postponed-again/

**D** — systems reach, compile-time introspection, ranges, readable generics;
avoid: no-VCS start, closed compiler, split stdlib (Phobos/Tango), GC-default +
`@nogc`/BetterC parallel modes, retrofitted `@safe`, fragmented reflection.
- Origins of the D Programming Language (HOPL, Walter Bright & Andrei Alexandrescu) — https://erdani.org/research/hopl.pdf
- Ruminations on D — Walter Bright interview / author blog (Better C) — https://dlang.org/blog/2016/08/30/ruminations-on-d-an-interview-with-walter-bright/ · https://dlang.org/blog/author/walterbright/
- How to Write @trusted Code in D — https://dlang.org/blog/2016/09/28/how-to-write-trusted-code-in-d/
- My Vision of D's Future — https://dlang.org/blog/2019/10/15/my-vision-of-ds-future/
- Warnings are not language-defined — https://dlang.org/articles/warnings.html

**Jai** — fast iteration, integrated tooling, compile-time execution, explicit
context; watch: closed distribution risk, ambient thread context, comptime
host/target questions.
- Four Years of Jai (Smári McCarthy) — https://smarimccarthy.is/posts/2024-12-02-four-years-of-jai/
- (see Jonathan Blow podcast above)

**Ada & SPARK** — strong typing, contracts, explicit representation,
safety-critical discipline, proof tooling; watch: full-vs-provable-subset gap,
proof parallel-specification burden, late package tooling.
- Guidelines for Safe & Secure Ada/SPARK — https://learn.adacore.com/courses/Guidelines_for_Safe_and_Secure_Ada_SPARK/chapters/introduction.html
- Intro to SPARK (PDF) — https://learn.adacore.com/pdf_books/courses/intro-to-spark.pdf
- AdaCore DO-178C airborne analysis / railway guide — https://learn.adacore.com/booklets/adacore-technologies-for-airborne-software/chapters/analysis.html · https://learn.adacore.com/booklets/adacore-technologies-for-railway-software/chapters/technology.html
- Alire / Ada projects-to-work-on (late tooling) — https://alire.ada.dev/ · https://ada-lang.io/docs/projects-to-work-on/
- ParaSail: A Pointer-Free Pervasively-Parallel Language (Tucker Taft) — https://programming-journal.org/2019/3/7/

### Managed & application languages

**Go** — readable code, fast builds, batteries-included tools, low-ceremony
concurrency, cohesive packaging; watch: `if err != nil` ceremony vs hidden `try`
returns, retrofitted generics, typed-nil interface, dependency redesigns.
- On/No syntactic support for error handling — https://go.dev/blog/error-syntax
- Generics proposal / generic interfaces — https://go.dev/blog/generics-proposal · https://go.dev/blog/generic-interfaces
- FAQ (typed-nil interface) — https://go.dev/doc/faq
- Experiment, Simplify, Ship (dependency history) — https://go.dev/blog/experiment
- Surveys 2022 / 2024-H1 / 2025 — https://go.dev/blog/survey2022-q2-results · https://go.dev/blog/survey2024-h1-results · https://go.dev/blog/survey2025
- ref/mod, ref/spec (switch), diagnostics, fuzzing, race detector, workspaces — https://go.dev/ref/mod · https://go.dev/ref/spec#Switch_statements · https://go.dev/doc/diagnostics · https://go.dev/doc/security/fuzz/ · https://go.dev/doc/articles/race_detector · https://go.dev/doc/tutorial/workspaces
- Issues #60056 / #60430 — https://github.com/golang/go/issues/60056 · https://github.com/golang/go/issues/60430

**Swift** — beginner-friendly syntax, named args, enums/matching, value
semantics, safe defaults, native app tooling; watch: 1→3 migration churn,
concurrency migration friction, ARC cycles, type-checker blowups, library
evolution, retroactive conformance.
- Swift 3 migration guide / release — https://www.swift.org/migration-guide-swift3/ · https://www.swift.org/blog/swift-3.0-released/
- Approachable Concurrency vision — https://github.com/swiftlang/swift-evolution/blob/main/visions/approachable-concurrency.md
- ARC guide / Extensions / Statements / API design guidelines — https://docs.swift.org/swift-book/documentation/the-swift-programming-language/automaticreferencecounting/ · https://docs.swift.org/swift-book/LanguageGuide/Extensions.html · https://docs.swift.org/swift-book/ReferenceManual/Statements.html · https://www.swift.org/documentation/api-design-guidelines/
- Type-checker: perf case study / 2026 improvements — https://forums.swift.org/t/a-type-checking-performance-case-study/2117 · https://forums.swift.org/t/recent-improvements-to-the-type-checker/87048
- Library Evolution — https://www.swift.org/blog/library-evolution/
- Explicit-copy: SE-0377 acceptance / class-copy discussion — https://forums.swift.org/t/accepted-se-0377-revision-make-borrowing-and-consuming-parameters-require-explicit-copying-with-the-copy-operator/65293 · https://forums.swift.org/t/copy-operator-doesnt-clone-a-class-instance/84592

**Java** — portability, mature tooling, broad libraries, long-lived API
discipline; avoid: reflection serialization, primitive/object split, erased
generics, universal nullability, checked exceptions, finalization.
- Towards Better Serialization (Brian Goetz) — https://openjdk.org/projects/amber/design-notes/towards-better-serialization
- Valhalla: background / in-defense-of-erasure / project — https://openjdk.org/projects/valhalla/design-notes/state-of-valhalla/01-background · https://openjdk.org/projects/valhalla/design-notes/in-defense-of-erasure · https://openjdk.org/projects/valhalla
- Non-reifiable varargs types — https://docs.oracle.com/javase/tutorial/java/generics/nonReifiableVarargsType.html
- Anders Hejlsberg on checked exceptions (Artima) — https://www.artima.com/intv/anders.html
- JEP 421 (deprecate finalization) — https://openjdk.org/jeps/421

**Kotlin** — concise application syntax, data classes, null-aware APIs,
expression orientation, multiplatform; avoid: five scope functions, platform
types, coroutine shared-state races, split failure observation, build-perf pain.
- Scope functions — https://kotlinlang.org/docs/scope-functions.html
- Java interop (platform types) — https://kotlinlang.org/docs/java-interop.html
- Exception handling / cancellation / shared-mutable-state / control-flow — https://kotlinlang.org/docs/exception-handling.html · https://kotlinlang.org/docs/cancellation-and-timeouts.html · https://kotlinlang.org/docs/shared-mutable-state-and-concurrency.html · https://kotlinlang.org/docs/control-flow.html
- JetBrains surveys: pains 2023 / Multiplatform 2021 — https://blog.jetbrains.com/kotlin/2022/11/how-kotlin-is-going-to-fix-your-pains-in-2023/ · https://blog.jetbrains.com/kotlin/2021/01/results-of-the-first-kotlin-multiplatform-survey/

**C# & .NET** — IDE/compiler integration, properties, structured async,
diagnostics, cross-platform libraries; avoid: optional nullability, `async void`,
sync-over-async deadlocks, deferred LINQ surprises, passive registry.
- Hejlsberg retrospective (Ars) — https://arstechnica.com/civis/threads/an-interview-with-anders-hejlsberg-c-s-lead-architect.104080/
- Nullable reference types / async / async return types — https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/builtin-types/nullable-reference-types · https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/keywords/async · https://learn.microsoft.com/en-us/dotnet/csharp/asynchronous-programming/async-return-types
- Sync-wrapper guidance — https://devblogs.microsoft.com/dotnet/should-i-expose-synchronous-wrappers-for-asynchronous-methods/
- LINQ intro — https://learn.microsoft.com/en-us/dotnet/csharp/linq/get-started/introduction-to-linq-queries
- Switch expression exhaustiveness — https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/operators/switch-expression#non-exhaustive-switch-expressions
- csharplang repo (every restriction is complexity) — https://github.com/dotnet/csharplang
- NuGet: State of the ecosystem / Broken by Design / central package mgmt — https://devblogs.microsoft.com/dotnet/state-of-the-nuget-ecosystem/ · https://devblogs.microsoft.com/dotnet/nuget-is-broken/ · https://learn.microsoft.com/en-us/nuget/consume-packages/central-package-management
- .NET diagnostics — https://learn.microsoft.com/en-us/dotnet/core/diagnostics/

**Python** — welcoming first run, low ceremony, readable code, REPL/notebook
flow, broad reach; avoid: `lambda`/`map`/`filter`/`reduce` overlap, packaging
fragmentation, GIL, erased optional hints, 2→3 break.
- Guido oral history (Computer History Museum) / Python Regrets — https://archive.computerhistory.org/resources/access/text/2018/07/102738719-05-01-acc.pdf · https://legacy.python.org/doc/essays/ppt/regrets/PythonRegrets.pdf
- Packaging strategy discussion — https://pyfound.blogspot.com/2023/02/python-packaging-strategy-discussion.html
- PEP 703 (no-GIL) / PEP 484 (type hints) / PEP 8 — https://peps.python.org/pep-0703/ · https://peps.python.org/pep-0484/ · https://peps.python.org/pep-0008/
- Py2 deprecation discussion / migration tracker #20812 — https://blog.python.org/2011/03/recent-discussion-on-python-dev/ · https://bugs.python.org/issue20812
- asyncio / C-API leading-underscore discussion — https://docs.python.org/3/library/asyncio.html · https://discuss.python.org/t/c-api-what-should-the-leading-underscore-py-mean/18486

**JavaScript** — immediate execution, ubiquitous deployment, script-to-app
growth; avoid: truthiness/coercion, ASI, dual `null`/`undefined`, CommonJS/ESM
split, npm global-graph fragility.
- JavaScript: The First 20 Years (Eich & Wirfs-Brock, HOPL) — https://www.cs.tufts.edu/~nr/cs257/archive/brendan-eich/js-hopl.pdf
- Original 1996 ECMAScript spec (TC39) — https://archives.ecma-international.org/1996/TC39/96-002.pdf
- MDN JS style guide / lexical grammar — https://developer.mozilla.org/en-US/docs/MDN/Writing_guidelines/Code_style_guide/JavaScript · https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Lexical_grammar
- Node packages/modules — https://nodejs.org/api/packages.html
- npm left-pad postmortem / supply-chain plan — https://blog.npmjs.org/post/141577284765/kik-left-pad-and-npm · https://github.blog/security/supply-chain-security/our-plan-for-a-more-secure-npm-supply-chain/

**TypeScript** — strong editor feedback, structural data, gradual adoption, JS
reach; avoid: deliberate unsoundness, ambient `any`, erase-not-validate,
runtime-bearing constructs, `tsconfig` sprawl.
- Type compatibility (soundness) / FAQ — https://www.typescriptlang.org/docs/handbook/type-compatibility.html · https://github.com/microsoft/typescript/wiki/faq
- Basic types / from scratch / for functional programmers — https://www.typescriptlang.org/docs/handbook/2/basic-types.html · https://www.typescriptlang.org/docs/handbook/typescript-from-scratch · https://www.typescriptlang.org/docs/handbook/typescript-in-5-minutes-func.html
- Migration to modules / TS 5.8 — https://devblogs.microsoft.com/typescript/typescripts-migration-to-modules/ · https://devblogs.microsoft.com/typescript/announcing-typescript-5-8/
- tsconfig / choosing compiler options — https://www.typescriptlang.org/docs/handbook/tsconfig-json · https://www.typescriptlang.org/docs/handbook/modules/guides/choosing-compiler-options.html

**Julia** — interactive scientific work, multiple dispatch, specialization,
notebooks, whole-data ops, native numerics; watch: JIT time-to-first-use,
open-world invalidation, dispatch ambiguity, performance cliffs.
- Invalidations / 1.10 highlights / 1.6 highlights — https://julialang.org/blog/2020/08/invalidations/ · https://julialang.org/blog/2023/12/julia-1.10-highlights/ · https://julialang.org/blog/2021/03/julia-1.6-highlights/
- Methods (ambiguity) / performance tips — https://docs.julialang.org/en/v1/manual/methods/ · https://docs.julialang.org/en/v1/manual/performance-tips/
- 2023 User & Developer Survey — https://21693537.fs1.hubspotusercontent-na1.net/hubfs/21693537/2023%20Julia%20User%20and%20Developer%20Survey.pdf
- Artifacts — https://julialang.org/blog/2019/11/artifacts/

### Functional, concurrent & language-oriented systems

**OCaml** — algebraic data, exhaustive matching, strong inference, modules;
avoid: universal polymorphic compare, drifting `.mli` shadow files.
- Removing polymorphic compare from Core / runtime crash example — https://discuss.ocaml.org/t/removing-polymorphic-compare-from-core/2994 · https://discuss.ocaml.org/t/generic-compare-that-crash-at-runtime/7411
- OCaml modules — https://ocaml.org/docs/modules

**F#** — approachable ML inference, ADTs, pipelines, object/functional interop,
data ergonomics, units of measure; watch: deciding semantics in parser not
typechecker; host-nullability leakage.
- Don Syme interview (Patrick Stevens) — https://www.patrickstevens.co.uk/posts/2018-09-10-don-syme/
- Component design guidelines / units of measure / spec (PDF) — https://learn.microsoft.com/en-us/dotnet/fsharp/style-guide/component-design-guidelines · https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/units-of-measure · https://fsharp.org/specs/language-spec/4.0/FSharpSpec-4.0-final.pdf

**Haskell** — purity, algebraic abstraction, effect separation, type-driven
APIs; avoid: space-leak laziness, `[Char]` default string, partial Prelude,
extension sprawl, split GHC/Cabal/Stack/HLS tooling.
- A History of Haskell: Being Lazy with Class (SPJ et al.) — https://simon.peytonjones.org/assets/pdfs/haskell-being-lazy-with-class.pdf
- What Would You See Changed in Haskell? — https://blog.haskell.org/what-would-you-see-changed-in-haskell/
- Towards a better end-user tooling experience — https://discourse.haskell.org/t/towards-a-better-end-user-experience-in-tooling/5512
- Haskell 2010 report — https://www.haskell.org/onlinereport/haskell2010/

**Gleam** — small typed surface, exhaustive data, one pipe, BEAM/JS reach;
watch: unverified externals, unscoped Hex names, `use` sugar needing editor
reveal.
- Externals / multi-target externals / pipelines — https://gleam.run/documentation/externals/ · https://tour.gleam.run/advanced-features/multi-target-externals/ · https://tour.gleam.run/functions/pipelines/
- Improved performance & publishing — https://gleam.run/news/improved-performance-and-publishing/
- FAQ (hot code upgrades) — https://gleam.run/frequently-asked-questions/

**Erlang & Elixir** — supervision, fault containment, lightweight tasks, message
passing, rolling upgrades; avoid: textual record includes, runtime-only type
errors; watch: hot upgrade needs typed state migration.
- Erlang academic FAQ (records) / release handling — https://www.erlang.org/faq/academic.html · https://www.erlang.org/doc/system/release_handling.html
- Design Principles of the Elixir Type System (arXiv 2306.06391) — https://arxiv.org/abs/2306.06391
- Elixir supervision / pipe operator / patterns & guards — https://elixir-lang.org/getting-started/mix-otp/supervisor-and-application.html · https://hexdocs.pm/elixir/Kernel.html#%7C%3E/2 · https://hexdocs.pm/elixir/patterns-and-guards.html

**Clojure** — small semantic core, persistent data, data-oriented programming,
REPL-driven dev, composable transforms; watch: error/startup/tooling
frustrations, opt-in spec instrumentation, governance clarity.
- State of Clojure 2016 / 2025 — https://clojure.org/news/2017/01/31/state-of-clojure-2016 · https://clojure.org/news/2026/02/18/state-of-clojure-2025
- spec / governance — https://clojure.org/guides/spec · https://clojure.org/news/2012/02/17/clojure-governance
- Data structures / threading macros — https://clojure.org/reference/data_structures · https://clojure.org/guides/threading_macros

**Racket** — language-oriented programming, tiny extensible core, hygienic
macros, teaching tooling; avoid: contract cost driving unsafe twins, per-file
`#lang` tool fragmentation.
- Language & Performance (no-contract submodules) — https://docs.racket-lang.org/style/Language_and_Performance.html
- The Racket Manifesto (PDF) — https://www2.ccs.neu.edu/racket/pubs/manifesto.pdf
- Creating languages guide — https://docs.racket-lang.org/guide/languages.html

**Smalltalk** — one message model, live inspection, immediate feedback, runtime
exploration; avoid: image-based persistence blocking VCS/deploy/migration.
- Typed Image-based Programming (arXiv 2110.08993) / Representing Code History (arXiv 1309.4334) — https://arxiv.org/abs/2110.08993 · https://arxiv.org/abs/1309.4334
- GNU Smalltalk syntax — https://www.gnu.org/software/smalltalk/manual/html_node/The-syntax.html

**Verse** (Epic) — explicit failure contexts, transactional rollback, rollback-
safe vs non-rollback effect distinction. Jet disposition: "watch" — rollback
only via explicit checked transaction regions.
- Failure & control flow guide / glossary / tracker reset example / speculative execution — https://dev.epicgames.com/documentation/en-us/fortnite/basics-of-writing-code-9-failure-and-control-flow-in-verse · https://dev.epicgames.com/documentation/en-us/fortnite/verse-glossary · https://dev.epicgames.com/documentation/fortnite/verse-api/fortnitedotcom/devices/tracker_device/reset · https://dev.epicgames.com/documentation/fortnite/speculative-execution

**Koka** — inferred effect rows, handlers, research-backed effect semantics
(direct lineage of Jet's `=[E]=>` inferred effect rows). Watch: dense rows and
handler calculus must not dominate beginner code.
- Effect rows (arXiv 1406.2061) / scoped effects (arXiv 2304.09697) / effect handlers evidently (ICFP 2020) — https://arxiv.org/abs/1406.2061 · https://arxiv.org/abs/2304.09697 · https://www.dhil.net/research/papers/effect_handlers_evidently-extended-icfp2020.pdf
- Koka book / repo — https://koka-lang.github.io/koka/doc/book.html · https://github.com/koka-lang/koka

**Pony** — capabilities as authority, actor isolation, race freedom,
runtime/type co-design; avoid: six-capability proof vocabulary on ordinary
references.
- Reference capabilities / combining capabilities / object capabilities / papers — https://www.ponylang.io/learn/reference-capabilities/ · https://tutorial.ponylang.io/reference-capabilities/combining-capabilities.html · https://tutorial.ponylang.io/object-capabilities/object-capabilities.html · https://www.ponylang.io/learn/papers/

**Roc** — friendly static FP, expected-type shorthand, explicit app/platform
split, aggressive simplification (fixed arity, no default currying, no HKP).
- FAQ / examples / homepage — https://www.roc-lang.org/faq · https://www.roc-lang.org/examples/ · https://roc-lang.org/

**Unison** — semantic identity, dependency-aware refactoring, content-addressed
definitions; avoid: DB-backed code weakening text/Git tooling, exposed hashes.
- The big idea / hashes / general FAQs — https://www.unison-lang.org/docs/the-big-idea/ · https://www.unison-lang.org/docs/language-reference/hashes/ · https://www.unison-lang.org/docs/usage-topics/general-faqs/

**Eiffel** — contracts beside code, invariant-driven design, readable intent;
avoid: covariant-parameter catcalls, retrofitted void safety,
rescue/retry-as-control-flow.
- Catcall solution / Type Safe Eiffel — https://www.eiffel.org/node/251 · https://www.eiffel.org/node/187
- Void safety / Exception mechanism / Design by Contract — https://www.eiffel.org/doc/eiffel/Void-safety-_how_Eiffel_removes_null-pointer-dereferencing · https://www.eiffel.org/doc/solutions/Exception_Mechanism · https://www.eiffel.org/doc/solutions/Design_by_Contract_and_Assertions

**Scala** — `CanThrow` capability experiment (effect-as-capability reference).
- https://docs.scala-lang.org/scala3/reference/experimental/canthrow.html

### Configuration, proof & array languages

**Nix** — hermetic builds, content-addressed artifacts, reproducibility,
declarative environments, immutable activation; avoid: many overlapping
composition layers, flake abstraction churn, whole-tree store copies, late lazy
failures.
- Nix language basics / dev manual (PDF) / store manual — https://nix.dev/tutorials/nix-language.html · https://nix.dev/nix-dev.pdf · https://nix.dev/manual/nix/stable/
- Flakes concept / wiki / Scaling Up Flakes (TIB video) — https://nix.dev/concepts/flakes.html · https://wiki.nixos.org/wiki/Flakes · https://av.tib.eu/media/61027
- Best practices / derivation / distributed builds / GC roots / profiles / store-object-info / why-depends — https://nix.dev/guides/best-practices.html · https://nix.dev/manual/nix/2.28/store/derivation/ · https://nix.dev/tutorials/nixos/distributed-builds-setup.html · https://nix.dev/manual/nix/latest/package-management/garbage-collector-roots · https://nix.dev/manual/nix/2.32/package-management/profiles · https://nix.dev/manual/nix/2.34/protocols/json/store-object-info.html · https://nix.dev/manual/nix/2.34/command-ref/new-cli/nix3-why-depends
- Nixpkgs (cross-compilation) — https://nixos.org/manual/nixpkgs/stable/

**Dhall** — typed configuration, total evaluation, integrity imports,
normalization; avoid: totality excluding ordinary data algorithms, huge normal
forms, upgrade friction.
- Design choices / language tour — https://docs.dhall-lang.org/discussions/Design-choices.html · https://docs.dhall-lang.org/tutorials/Language-Tour.html
- Slow compile times / 2019 survey — https://discourse.dhall-lang.org/t/slow-compile-times/634 · https://haskellforall.com/2019/02/dhall-survey-results-2019-2019

**CUE** — order-independent unification, constraints as data, conflict
diagnostics preserving both sources; avoid: turning ordinary code into a general
logic/disjunction language.
- Spec / disjunctions / introduction / logic of CUE — https://cuelang.org/docs/reference/spec/ · https://cuelang.org/docs/tour/types/disjunctions/ · https://cuelang.org/docs/introduction/ · https://cuelang.org/docs/concept/the-logic-of-cue/

**Nickel** — typed config, mergeable records, contracts, priorities, gradual
schema; avoid: lazy contract deferral, transforming (non-idempotent) contracts,
placement-dependent precedence.
- Correctness / contracts / merging — https://nickel-lang.org/user-manual/correctness/ · https://nickel-lang.org/user-manual/contracts/ · https://nickel-lang.org/user-manual/merging/

**APL & BQN** — whole-data operations, rank-aware thinking, compact algebra,
optimizer-friendly array semantics; avoid: empty-array type loss, glyph
overloading, runtime rank/shape conventions.
- Based array theory / problems / paradigms / docs index — https://mlochbaum.github.io/BQN/doc/based.html · https://mlochbaum.github.io/BQN/problems.html · https://mlochbaum.github.io/BQN/doc/paradigms.html · https://mlochbaum.github.io/BQN/doc/index.html

---

## Package managers, build systems & registries

Research for Jetpack (Epoch 4) and the ecosystem shape. See
`docs/plans/epoch-4/world-class-package-manager.md` and
`docs/proposals/ecosystem-shape.md`.

- Cargo (Rust) — build scripts, features, workspaces, resolver (see Rust above)
- Go modules — https://go.dev/ref/mod
- Nix store & package-manager model / derivations / flakes / profiles / GC roots (see Nix above)
- Guix substitutes & reproducibility (PDF) — https://guix.gnu.org/manual/en/guix.pdf
- Bazel hermeticity — https://bazel.build/concepts/hermeticity
- Homebrew bottles — https://docs.brew.sh/Bottles
- vcpkg manifest mode — https://learn.microsoft.com/en-us/vcpkg/concepts/manifest-mode
- Conan package identity — https://docs.conan.io/2/reference/conanfile/methods/package_id.html
- Gradle variant-aware resolution — https://docs.gradle.org/current/userguide/variant_aware_resolution.html
- Maven dependency mechanism — https://maven.apache.org/guides/introduction/introduction-to-dependency-mechanism.html
- NuGet central package management — https://learn.microsoft.com/en-us/nuget/consume-packages/central-package-management
- uv resolution (Astral) — https://docs.astral.sh/uv/concepts/resolution/
- pnpm build-script approval / settings — https://pnpm.io/cli/approve-builds · https://pnpm.io/settings
- Yarn Plug'n'Play strict graph — https://yarnpkg.com/features/pnp
- Bundler lock & platforms — https://bundler.io/man/bundle-lock.1.html
- SwiftPM package security — https://docs.swift.org/swiftpm/documentation/packagemanagerdocs/packagesecurity/
- OCI image-spec artifact manifests — https://github.com/opencontainers/image-spec/blob/main/manifest.md
- (D's Phobos/Tango split, npm/left-pad, NuGet, crates.io malware — cited under their languages above)

---

## Supply-chain & security standards

- SemVer — https://semver.org/ (see `docs/reference/versioning.md`)
- SLSA build track — https://slsa.dev/spec/v1.2/build-track-basics
- Sigstore — https://docs.sigstore.dev/about/overview/
- The Update Framework (TUF) — https://theupdateframework.github.io/specification/latest/

---

## Web, UI & notebook frameworks

Research for the universal language core (Epoch 3) and the web/WASM backend. See
`docs/plans/epoch-3/universal-language-core.md`.

- Vite — features/HMR, SSR — https://vite.dev/guide/features.html · https://vite.dev/guide/ssr.html
- Next.js App Router — https://nextjs.org/docs/app
- SvelteKit — intro, form actions; Svelte runes — https://svelte.dev/docs/kit/introduction · https://svelte.dev/docs/kit/form-actions · https://svelte.dev/docs/svelte/what-are-runes
- React — Compiler, Server Components — https://react.dev/learn/react-compiler · https://react.dev/reference/rsc/server-components
- Jupyter — architecture, kernels — https://docs.jupyter.org/en/stable/ · https://docs.jupyter.org/en/stable/projects/kernels.html

---

## Numeric & array computing

- NumPy — broadcasting, ufuncs — https://numpy.org/doc/stable/user/basics.broadcasting.html · https://numpy.org/doc/stable/reference/ufuncs.html
- CUDA C programming guide — https://docs.nvidia.com/cuda/cuda-c-programming-guide/
- (BQN/APL array semantics — see array languages above)

---

## Visual, projectional & RAD editors

Research for the Epoch-6 Canvas (a source-backed, Blueprint-class visual editor)
and its "one editor, two surfaces" RAD direction.

- **Unreal Engine Blueprints** — the visual-editor north-star and market
  opening (Epic replacing it with Verse; D-CANVAS-RAD1). Referenced throughout
  `docs/plans/epoch-6/`.
- **Scratch** — block-based beginner programming reference (Canvas plans).
- **JetBrains MPS** — projectional editing over one semantic program; avoid:
  AST-as-persistence needing special diff/merge, unfamiliar selection/deletion.
  - MPS concepts / FAQ / editor — https://www.jetbrains.com/mps/concepts/ · https://www.jetbrains.com/help/mps/mps-faq.html · https://www.jetbrains.com/help/mps/editor.html
- **Hazel** — typed holes, meaning for incomplete programs, gradual structure
  editing; avoid: strictly tree-structured editing.
  - Hazel project / Gradual Structure Editing with Obligations (VLHCC 2023) — https://hazel.org/ · https://hazel.org/papers/teen-tylr-vlhcc2023.pdf
- Projectional editing experiment 2024 (Dagstuhl SLATE) — https://drops.dagstuhl.de/entities/document/10.4230/OASIcs.SLATE.2024.5

---

## Academic papers & long-form retrospectives

Consolidated (also cited inline above):

- HOPL: D (Alexandrescu & Bright) — https://erdani.org/research/hopl.pdf
- HOPL: JavaScript first 20 years (Eich & Wirfs-Brock) — https://www.cs.tufts.edu/~nr/cs257/archive/brendan-eich/js-hopl.pdf
- A History of Haskell: Being Lazy with Class — https://simon.peytonjones.org/assets/pdfs/haskell-being-lazy-with-class.pdf
- ParaSail (Taft) — https://programming-journal.org/2019/3/7/
- Koka effect rows / scoped effects / effect handlers evidently — https://arxiv.org/abs/1406.2061 · https://arxiv.org/abs/2304.09697 · https://www.dhil.net/research/papers/effect_handlers_evidently-extended-icfp2020.pdf
- Elixir type system — https://arxiv.org/abs/2306.06391
- Typed image-based programming / Representing code history — https://arxiv.org/abs/2110.08993 · https://arxiv.org/abs/1309.4334
- Cognitive Dimensions of Notations tutorial (Blackwell) — https://www.cl.cam.ac.uk/~afb21/CognitiveDimensions/CDtutorial.pdf

---

## Writing, controlled-language & documentation standards

Governs Jet's user-facing prose (the `simple` skill) and diagnostics-as-product
discipline.

- ASD-STE100 (Simplified Technical English) — https://www.asd-ste100.org/ · https://asd-ste100.org/
- George Orwell, "Politics and the English Language" (prose rules; `simple` skill) — public-domain essay, not linked.
- Cognitive Dimensions of Notations (see above) — usability framework for syntax/notation choices.

---

## Notes on provenance

- URLs above were traced from `docs/`, `.agents/skills/`, and Tower board data
  across Jet's history. Where a source is a creator regret vs. an official
  retrospective vs. a durable ecosystem issue, the distinction is preserved in
  `docs/archive/language-lessons-and-regrets.md`.
- Video sources were mined with the `mine-video` skill (full transcript +
  stratified comment analysis); dated reports live in `docs/research/`.
- This record is descriptive, not a ranking. A listed mistake never outweighs a
  language's successes; Jet's rule is to copy the successful invariant and
  decline the baggage that was necessary elsewhere.
