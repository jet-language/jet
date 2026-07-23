# Lessons from Jet's language lineage

**Research date:** 2026-07-15

**Status:** research report; not language law

**Scope:** the languages and language families named in Jet's language-shape and universal-language research, plus the ecosystems whose failure modes directly constrain Jet's stated goals.

Jet aims to combine beginner immediacy, systems control, memory safety, one semantic model, and a complete first-party ecosystem. That puts it in the blast radius of mistakes made by systems languages, managed languages, functional languages, scripting languages, configuration languages, and proof-oriented languages. This report records those mistakes before Jet's public contracts harden.

The report distinguishes three kinds of evidence:

- **Creator regret:** a designer directly says a choice was a mistake, regret, or choice they would now change.
- **Official retrospective:** a language project, standards body, or core maintainer documents a limitation, redesign, migration, or persistent survey result.
- **Durable ecosystem issue:** a recurring problem supported by official documentation, surveys, or multiple years of project response. A popular complaint by itself is not treated as fact.

“Jet disposition” means one of:

- **Guarded:** ratified Jet law already prevents the failure.
- **Owned:** an existing card or planned verification lane owns the remaining work.
- **Watch:** no present decision is required, but a named future design must carry the lesson.
- **Gap:** current law does not settle a relevant choice. A gap is not automatically a ballot; the choice must be concrete and owner-gated.

## Executive findings

The research produced twelve repeated lessons.

1. **Safety cannot be retrofitted cleanly.** C, C++, D, Java, C#, Eiffel, Python, TypeScript, Kotlin interop, and Swift concurrency all show the cost of adding safety after unsafe or underspecified behavior has become normal. Jet's non-null, memory-safe, type-safe default and reason-bearing `#Unsafe` gate are the right foundation. No build profile may weaken them.
2. **One semantic job must have one mechanism.** Kotlin's five scope functions, C/C++ parallel “safe” APIs, D's reflection families, Python packaging, Nix override layers, JavaScript module systems, and Haskell extension/tool stacks all became permanent teaching and tooling taxes.
3. **Visible failure needs low ceremony, not hidden control flow.** Go's repetitive error checks and rejected `try`, Java's checked-exception evolution problems, C#'s `async void`, and exception-heavy proof systems all support Jet's typed fallibility plus visible `?`. The compiler must keep the path concise without making exits invisible.
4. **Concurrency is a semantic system, not syntax sugar.** Rust async, Swift 6 concurrency, Kotlin coroutines, C# scheduler capture, Python's GIL, and BEAM hot upgrades show that cancellation, cleanup, sendability, scheduling, failure observation, FFI, and live state must be designed together.
5. **Open extension requires coherence.** Rust's orphan rule, Swift retroactive conformances, and Julia's method ambiguity show that unrelated packages must not be able to create downstream dispatch conflicts. Jet already uses a local-type-or-local-trait orphan rule and rejects duplicate implementations.
6. **Inference must be bounded, unique, and inspectable.** Swift's type-checker blowups, Zig's inferred error-set instability, Rust ownership friction, Koka effect rows, and Julia specialization cliffs all argue for inference only when one nearby answer exists, with explicit public boundaries and explain tools.
7. **Interop is a quarantine boundary.** Kotlin platform types, TypeScript erasure, Python C extensions, C strings and pointers, Gleam externals, and ABI fragmentation show that foreign facts cannot silently become safe Jet facts. Layout, ownership, absence, errors, concurrency, and target support require checked adapters.
8. **Tooling and packaging are part of the language contract.** Go's dependency redesigns, Python packaging fragmentation, npm incidents, Rust build scripts/features, Haskell's release matrix, Kotlin/Gradle performance, and Ada's late package tooling show that compiler, formatter, editor, resolver, builder, installer, and publisher must behave as one tested product.
9. **Compatibility mistakes compound.** C++ baggage, Java's primitives/generics/nullability, JavaScript web compatibility, D1→D2, Python 2→3, Swift 1→3, and Zig churn show why fundamentals must settle before stability and why migrations must be executable before breaking changes ship.
10. **Lazy or dynamic internals must fail eagerly at trust boundaries.** Nix, Dhall, Nickel, CUE, Julia, LINQ, and Clojure show how delayed evaluation moves errors away from causes and hides repeated work. Jet may optimize lazily, but build, export, decode, audit, and deployment boundaries must fully validate reachable output with provenance.
11. **Contracts and proofs must not become a shadow program.** SPARK's proof annotations, Racket's unchecked twin modules, Nickel's transforming contracts, and Eiffel's recovery semantics support Jet's existing law: contract conditions are pure, observational, checked in every build, and fail distinctly from typed domain errors.
12. **Take the invariant, not the surface syntax.** Pony capabilities, Koka effects, APL/BQN array operations, Smalltalk messages, Unison hashes, and Racket languages each contain powerful ideas whose full exposed vocabulary or storage model would violate Jet's beginner facet or source-truth rules.

## Jet's current defensive position

The strongest lessons are already reflected in Jet law:

| Repeated failure | Current Jet defense | Remaining verification |
|---|---|---|
| Unsafe defaults and safety profiles | I1; no safe-source escape except reason-bearing `#Unsafe`; optimization cannot change safety | Unsafe-region audit, FFI hostile corpus, memory fact matrix cards #642–#649 |
| Backend/typechecker disagreement | I2, I3, R1–R3; front end owns checking and diagnostics | Keep every new backend on executable TIR parity lanes |
| Multiple mechanisms | I8; syntax registry with decision IDs; one canonical Core operation | Continue naming/coherence audits before new surface ships |
| Null and sentinel absence | `Option`; no `null` | Validate every FFI/codec boundary before safe construction |
| Error invisibility or ceremony | Typed `T ? E`, visible `?`, explicit conversion implementations | Adversarially test nested propagation, callbacks, and task failures |
| Async coloring and detached failure | Blocking-looking task APIs, structured task groups, cancellation law, must-use handles | Ensure every constructor shares failure, cleanup, and cancellation semantics |
| Dispatch ambiguity | Local type-or-trait orphan law; duplicate implementation diagnostics | Cross-package glue and cache-invalidation hostile tests |
| Contract drift or profile elision | D-PREPOST1: pure conditions, checked in every build, E3005 runtime stop | Prove optimizer removal only when condition is statically established |
| Lazy config failures | Ratified ecosystem law: eager typed graph checks with source and provenance | Deep-force every reachable plan/export/audit output in acceptance tests |
| Breaking-change chaos | Ratified editions, deprecation registry, `jet fix`, explicit edition bump | Package-graph readiness and binary-library policy when binary distribution lands |
| Ambient build authority | Typed effect-declared plans, sandboxing, lock/receipt provenance | Host/target separation and capability receipts for comptime/build hooks |
| Text/tool fragmentation | Plain files are source truth; one CLI/schema/diagnostic registry | Release the compiler/LSP/fmt/pkg matrix as one tested distribution |

## Systems and safety languages

### C

**What Jet takes:** predictable representation, direct FFI, freestanding reach, and a small machine-facing core.

**Lessons:**

- C's standards committee now describes most ordinary C functions as unsafe and cites memory-safety defects as a dominant vulnerability source. Even basic operations rely on caller-maintained preconditions. Safety attributes cannot repair a language whose ordinary mode is unsafe. [WG14 N2659](https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2659.htm)
- Array-to-pointer decay erases the bounds facts later safety tools need. Multiple WG14 proposals attempt to recover or preserve them. [N1990](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1990.htm), [N2660](https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2660.pdf), [N3360](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3360.htm)
- Annex K's parallel bounds-checking library saw sparse adoption, incompatible semantics, and nonconforming implementations. Adding `_s` alternatives did not make the default ecosystem safe. [WG14 N1967](https://open-std.org/jtc1/sc22/wg14/www/docs/n1967.htm)
- NUL-terminated strings permanently couple text to a sentinel, lose length, require repeated scans, and make embedded NUL a boundary hazard. [“The Most Expensive One-byte Mistake”](https://queue.acm.org/detail.cfm?id=2010365)
- Headers and the preprocessor make dependency semantics textual and order-sensitive; even idempotence depends on convention. [WG14 N1400](https://www.open-std.org/JTC1/SC22/wg14/www/docs/n1400.htm), [N2896](https://open-std.org/jtc1/sc22/wg14/www/docs/n2896.htm)
- Integer promotions, narrowing, and overflow remain sufficiently surprising that the committee continues adding checked and saturating mechanisms. [WG14 N1254](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1254.htm), [N2885](https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2885.pdf)

**Jet disposition:** guarded. Safe arrays and future views must always retain length, owner lifetime, and mutability; C pointers and C strings exist only at explicit FFI boundaries. Never ship `foo` and `foo_safe`. Keep numeric conversions and overflow defined in sema.

### C++

**What Jet takes:** zero-cost abstraction, deterministic lifetime, generic programming, and broad native interoperability.

**Creator regrets and official issues:**

- Bjarne Stroustrup says templates arrived after multiple inheritance and that the first release lacked a serious library; the gap encouraged class hierarchies where generic composition fit better. He also calls C declarator syntax an experiment that failed. [Stroustrup's Slashdot interview](https://www.stroustrup.com/slashdot_interview.html)
- Stroustrup would ban C-style casts and narrowing conversions and replace raw arrays with vector-like types, but C compatibility makes removal impractical. [Stroustrup's DevX interview](https://www.stroustrup.com/devXinterview.html)
- WG21's memory-safety work documents how easily views dangle and why sanitizers cannot recover every semantic invalidation rule. [P2771R1](https://isocpp.org/files/papers/P2771R1.html)
- Stroustrup identifies the absence of a common ABI as one of C++'s hardest technical problems; practical interoperability often falls back to one compiler or a C boundary. [Stroustrup's Slashdot interview](https://www.stroustrup.com/slashdot_interview.html)
- Compatibility and feature accumulation create dialects and second-order interactions: individually useful features cannot be redesigned to cooperate cleanly. [Stroustrup retrospective](https://www.stroustrup.com/italian_interview.html)

**Jet disposition:** guarded and watch. Jet correctly rejects inheritance, implicit narrowing, C-style casts, raw array decay, and backend-owned safety. Future `View`/`ViewMut` work must statically prevent dangling and mutation invalidation. Jet's stable external boundary must be Jet-owned typed metadata plus a deliberately versioned ABI, never rustc or C++ layout by accident.

### Rust

**What Jet takes:** ownership, deterministic cleanup, exhaustive algebraic data, strong diagnostics, Cargo's integrated workflow, and safe systems reach.

**Official lessons:**

- Rust's language team says learning difficulty includes many small accidental details even after the ownership model is understood; surveys continue to identify perceived difficulty as an adoption barrier. [Rust 2024 language roadmap](https://blog.rust-lang.org/inside-rust/2022/04/04/lang-roadmap-2024/), [2024 State of Rust survey](https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/)
- Async stabilized as an incomplete parallel surface: traits, closures, recursion, scoped tasks, drop, runtime choice, and library interoperability required years of follow-on work. [2024 async goal](https://rust-lang.github.io/rust-project-goals/2024h2/async.html), [2026 async roadmap](https://rust-lang.github.io/rust-project-goals/2026/roadmap-just-add-async.html)
- Compile time is a recurring productivity and retention cost. [Rust compiler-performance survey](https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results/)
- The orphan rule protects global coherence but makes third-party “glue” integrations awkward. [Rust goal: relaxing the orphan rule](https://rust-lang.github.io/rust-project-goals/2024h2/Relaxing-the-Orphan-Rule.html)
- Cargo automatically executes package build scripts, which creates a supply-chain execution boundary; crates.io has documented malware using that boundary. [Cargo build scripts](https://doc.rust-lang.org/stable/cargo/reference/build-scripts.html), [crates.io malware postmortem](https://blog.rust-lang.org/inside-rust/2023/09/01/crates-io-malware-postmortem/)
- Cargo feature unification unions choices across a graph. Defaults are difficult to disable, and mutually exclusive features demand graph-wide coordination. [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html)
- Graydon Hoare's personal retrospective rejects several later Rust choices while acknowledging that his preferred version might not have achieved Rust's success. This is creator preference, not a Rust-project admission. [“The Rust I Wanted Had No Future”](https://graydon2.dreamwidth.org/307291.html)

**Jet disposition:** guarded and owned. Jet's tier-one ownership vocabulary should stay smaller than Rust's lifetime surface while preserving the same soundness. Async must not ship until ordinary calls, traits, cleanup, cancellation, FFI, and runtime behavior share one model. Build/comptime work must be effect-declared and sandboxed. Package features must not let distant dependencies silently change API or semantics.

### Zig

**What Jet takes:** explicit control, freestanding targets, comptime evaluation, expected-type shorthand, allocator visibility, and integrated build tooling.

**Official lessons:**

- Zig disables many safety checks in release-fast and release-small modes. That trades optimization profile for semantic safety. [Zig language reference: illegal behavior](https://ziglang.org/documentation/master/)
- Zig removed `usingnamespace` because it obscured name origins, forced semantic analysis to discover namespace contents, and complicated incremental dependency modeling. [Zig issue #20663](https://github.com/ziglang/zig/issues/20663)
- Inferred error sets make functions generic, complicate function pointers, vary across targets, and conflict with recursion. [Zig language reference: inferred error sets](https://ziglang.org/documentation/master/)
- Explicit allocator parameters preserve control but push a long policy decision tree into ordinary library usage. [Zig language reference](https://ziglang.org/documentation/master/)
- Pre-1.0 churn keeps the language, standard library, package manager, and learning material in motion. Zig's own roadmaps make stabilization a multi-system task, not just a parser milestone. [Zig 0.10 release notes](https://ziglang.org/download/0.10.0/release-notes.html), [release-month expectations](https://ziglang.org/news/what-to-expect-from-release-month/)
- Async syntax and semantics were proposed, revised, and then removed from release plans while the design remained unresolved. [Zig async proposal #5913](https://github.com/ziglang/zig/issues/5913), [0.11 postponement](https://ziglang.org/news/0.11.0-postponed-again/)

**Jet disposition:** guarded. Optimization never weakens safety. Imports remain explicit. Private effect/error inference may omit uniquely known facts, but public, recursive, FFI, and target-sensitive boundaries pin them. Allocator control belongs behind a revealable expert opt-in, not beginner ceremony. Compiler intrinsics should stay smaller than a second builtin language.

### D

**What Jet takes:** systems reach, compile-time introspection, ranges, readable generics, and willingness to span low- and high-level work.

**Creator and project retrospectives:**

- D's creator-authored HOPL paper names process mistakes including starting without version control, keeping the compiler closed too long, and shipping failed special features such as `bit` and D-in-HTML. [“Origins of the D Programming Language”](https://erdani.org/research/hopl.pdf)
- The D1-to-D2 break nearly destroyed the community. The same retrospective records the social and ecosystem cost of a language-generation reset. [D HOPL retrospective](https://erdani.org/research/hopl.pdf)
- A neglected standard library allowed Phobos and Tango to become incompatible alternatives, splitting users and library authors. [D HOPL retrospective](https://erdani.org/research/hopl.pdf)
- A GC-default systems language later needed `@nogc` and BetterC, creating a parallel mode that also removes unrelated facilities. [Walter Bright interview](https://dlang.org/blog/2016/08/30/ruminations-on-d-an-interview-with-walter-bright/), [D as Better C](https://dlang.org/blog/author/walterbright/)
- Retrofitted `@safe`/`@trusted` boundaries are easy to annotate incorrectly and can become invalid after maintenance. [“How to Write @trusted Code in D”](https://dlang.org/blog/2016/09/28/how-to-write-trusted-code-in-d/)
- D leadership identifies `__traits`, `std.traits`, and custom reflection code as fragmented ways to do one job. [“My Vision of D's Future”](https://dlang.org/blog/2019/10/15/my-vision-of-ds-future/)
- Warnings are not language-defined and may differ between compilers. [D warnings documentation](https://dlang.org/articles/warnings.html)

**Jet disposition:** guarded. One open compiler, one curated Core, one ownership model across hosted and freestanding targets, one reflection/index API, coded diagnostics, and staged edition migrations directly answer D's experience. The semantic history in Git, Tower, decision IDs, examples, and snapshots is infrastructure, not paperwork.

### Jai

**What Jet takes:** fast iteration, integrated tooling, compile-time execution, explicit context/control, and game-oriented native performance.

Jai remains closed and pre-1.0, so the evidence base is narrower than for the other languages.

- Jonathan Blow says closed beta preserves permission to make incompatible changes and describes staged spelling migrations through coexistence, warning, removal, and changelog. [Jonathan Blow interview transcript](https://podscripts.co/podcasts/the-standup-with-theprimeagen/legendary-game-dev-jonathan-blow)
- A four-year production user identifies closed distribution, hiring, limited libraries, drifting documentation, and incomplete testing support as material risks. This is notable-user evidence, not a representative survey. [“Four Years of Jai”](https://smarimccarthy.is/posts/2024-12-02-four-years-of-jai/)
- Jai's implicit thread context carries allocator and logging authority; crossing dynamic-library and target boundaries makes that ambient state harder to reason about. [“Four Years of Jai”](https://smarimccarthy.is/posts/2024-12-02-four-years-of-jai/)
- Powerful compile-time execution creates host-versus-target, debugging, cache, and macro-rewrite questions that must be settled before stable release. [“Four Years of Jai”](https://smarimccarthy.is/posts/2024-12-02-four-years-of-jai/)

**Jet disposition:** watch. Preserve Jai's iteration goals without a closed compiler or private ecosystem bottleneck. Ambient task/thread context must be typed, scoped, inspectable, and FFI-stable. Comptime effects and every host input belong in the build identity and audit receipt.

### Ada and SPARK

**What Jet takes:** strong typing, contracts, explicit representation, safety-critical discipline, structured concurrency, and proof-oriented tooling.

**Official lessons:**

- Full Ada is broader than the subset used for high-assurance analysis; SPARK excludes facilities that are difficult to prove, including arbitrary access patterns and full tasking. [Ada/SPARK safe-and-secure guidelines](https://learn.adacore.com/courses/Guidelines_for_Safe_and_Secure_Ada_SPARK/chapters/introduction.html)
- Tucker Taft's later ParaSail work removes globals, parameter aliasing, and reassignable pointers to reduce rules and enable safe parallelism. [“ParaSail: A Pointer-Free Pervasively-Parallel Language”](https://programming-journal.org/2019/3/7/)
- Proof can require loop invariants, contracts, ghost state, quantified predicates, and other parallel specification structure. [Introduction to SPARK](https://learn.adacore.com/pdf_books/courses/intro-to-spark.pdf)
- Proof tools may require assertions, assumptions, dismissals, tests, or manual assistance; foreign code and compiler preservation remain separate trust questions. [AdaCore DO-178C analysis](https://learn.adacore.com/booklets/adacore-technologies-for-airborne-software/chapters/analysis.html)
- Exceptions obstruct some proof and cleanup reasoning. [AdaCore railway guide](https://learn.adacore.com/booklets/adacore-technologies-for-railway-software/chapters/technology.html)
- Package tooling and common bindings arrived later than the language's core safety story. [Alire](https://alire.ada.dev/), [Ada community project gaps](https://ada-lang.io/docs/projects-to-work-on/)

**Jet disposition:** guarded. The safe default language, not a restricted dialect, must remain analyzable. Proof reports must distinguish proved, disproved, unproved, assumed, foreign, and tool-limited facts. Contracts reuse ordinary predicates and types. Exceptions are not Jet's canonical domain-error path. Ecosystem completeness must ship alongside language safety.

## Mainstream managed and application languages

### Go

**What Jet takes:** readable code, fast builds, batteries-included tools, low-ceremony concurrency, simple deployment, and cohesive package workflows.

**Official lessons:**

- Error handling remains one of Go's oldest complaints: repeated `if err != nil` blocks can obscure the work. The rejected `try` design showed the opposite failure—returns hidden inside expressions—and the team also acknowledged bringing a proposal to the community too fully formed. [“[ On | No ] syntactic support for error handling”](https://go.dev/blog/error-syntax)
- Generics were a top request from the 2009 release until 2022. Retrofitting them left initial limits and exposed complex pointer-receiver patterns that need extra type parameters. [Generics proposal](https://go.dev/blog/generics-proposal), [2022 survey](https://go.dev/blog/survey2022-q2-results), [generic interfaces](https://go.dev/blog/generic-interfaces)
- An interface containing a typed nil pointer is itself non-nil. The official FAQ calls the result confusing. [Go FAQ](https://go.dev/doc/faq)
- Dependency management passed through GOPATH, vendor conventions, `dep`, and modules. The Go team's retrospective says vendor interoperability was too optimistic because tools shared syntax without sharing semantics. [“Experiment, Simplify, Ship”](https://go.dev/blog/experiment)
- Surveys still find users re-reading command documentation and struggling with large-project organization. A small language does not automatically produce a simple ecosystem. [2025 Go survey](https://go.dev/blog/survey2025), [2024 H1 Go survey](https://go.dev/blog/survey2024-h1-results)

**Jet disposition:** guarded. `Option` avoids nil/interface dual states. Typed fallibility plus visible `?` should keep failures local without repetitive boilerplate or expression-hidden returns. Generics and ownership capabilities must be foundational. One resolver, one lock model, generated help, and canonical project organization remain product requirements.

### Swift

**What Jet takes:** beginner-friendly syntax, named arguments, enums and pattern matching, value semantics, safe defaults, and strong native application tooling.

**Official lessons:**

- Swift 1–3 changed standard-library names, labels, Objective-C imports, collections, and nullability. The migrator helped, but dependencies and manual fixes still made the transition ecosystem-wide. [Swift 3 migration guide](https://www.swift.org/migration-guide-swift3/), [Swift 3 release](https://www.swift.org/blog/swift-3.0-released/)
- Swift's concurrency steering group says data-race safety meets its correctness goal while migration can be frustrating and can violate progressive disclosure. Task ordering and actor reentrancy remain difficult. [Approachable Concurrency vision](https://github.com/swiftlang/swift-evolution/blob/main/visions/approachable-concurrency.md)
- ARC hides most lifetime work but strong reference cycles and closure capture cycles leak until users apply `weak` or `unowned` correctly. [Swift ARC guide](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/automaticreferencecounting/)
- Overload resolution and inference can become combinatorial. Invalid code may produce “unable to type-check in reasonable time” rather than the actual error. [Type-checking performance case study](https://forums.swift.org/t/a-type-checking-performance-case-study/2117), [2026 type-checker update](https://forums.swift.org/t/recent-improvements-to-the-type-checker/87048)
- Library evolution trades static layout and exhaustiveness knowledge for binary resilience and can bake defaults into clients. It must be enabled deliberately from the first release. [Library Evolution in Swift](https://www.swift.org/blog/library-evolution/)
- Extensions permit retroactive conformance of foreign types to foreign protocols, creating collision risk if an upstream package later adds the same conformance. [Swift extensions](https://docs.swift.org/swift-book/LanguageGuide/Extensions.html)

**Jet disposition:** guarded and watch. Ownership should avoid silent ARC cycles. Concurrency defaults must stay approachable without weakening race proofs. Inference needs deterministic complexity budgets and a real diagnostic when invalid code is hard. Jet's orphan law already prevents Swift-style foreign/foreign conformance. Binary library resilience becomes a future owner gate only when Jet distributes stable binary libraries; source-package rebuilding is not the same problem.

### Java

**What Jet takes:** portability, mature tooling, managed-service ergonomics, broad libraries, and long-lived API discipline.

**Maintainer retrospectives:**

- Brian Goetz writes that Java serialization “makes nearly every mistake imaginable”: it bypasses constructors, access control, and invariants while appearing statically typed. [Towards Better Serialization](https://openjdk.org/projects/amber/design-notes/towards-better-serialization)
- Java's primitive/object split was an early compromise whose nonuniformity spread through generics, boxing, libraries, and the VM. Project Valhalla exists in part to repair that tax. [State of Valhalla: The Road to Valhalla](https://openjdk.org/projects/valhalla/design-notes/state-of-valhalla/01-background)
- Erased generics enabled migration but made parameterizations indistinguishable at runtime, forbade generic arrays, and permitted raw/unchecked holes that can fail later with `ClassCastException`. [Oracle non-reifiable types](https://docs.oracle.com/javase/tutorial/java/generics/nonReifiableVarargsType.html), [In Defense of Erasure](https://openjdk.org/projects/valhalla/design-notes/in-defense-of-erasure)
- Universal nullability remains a foundational repair target decades later. [Project Valhalla](https://openjdk.org/projects/valhalla)
- Checked exceptions create API evolution pressure: adding a failure breaks callers, while broad catches discard the useful type information. [Anders Hejlsberg on checked exceptions](https://www.artima.com/intv/anders.html)
- Finalization has unpredictable latency, resurrection hazards, unspecified threading, security issues, and permanent maintenance cost. Java is deprecating it for removal. [JEP 421](https://openjdk.org/jeps/421)

**Jet disposition:** guarded. `Codable` must be explicit typed reconstruction that validates invariants; no reflection serializer may bypass constructors. Jet's value model should optimize representation behind sema/TIR without wrapper shadow types. Generic type facts must remain available wherever reflection, codec, or FFI promises need them. `Option`, typed error values, and deterministic RAII cleanup directly avoid Java's null, checked-exception, and finalizer traps.

### Kotlin

**What Jet takes:** concise application syntax, data classes, null-aware APIs, expression orientation, and pragmatic multiplatform reach.

**Official issues:**

- Kotlin's documentation says its five scope functions perform essentially the same action, overlap, are tricky to choose, and become confusing when nested. [Kotlin scope functions](https://kotlinlang.org/docs/scope-functions.html)
- Java interop introduces platform types such as `String!`, flexible collection mutability, and configurable nullability migrations. Foreign uncertainty punctures the otherwise safe model. [Calling Java from Kotlin](https://kotlinlang.org/docs/java-interop.html)
- JetBrains surveys identify build setup, build speed, indexing, freezes, and IDE performance as major ecosystem pain, especially in Multiplatform projects. [Kotlin pain survey](https://blog.jetbrains.com/kotlin/2022/11/how-kotlin-is-going-to-fix-your-pains-in-2023/)
- Coroutines retain ordinary shared-state races on multithreaded dispatchers. Syntax and suspension do not create race freedom. [Shared mutable state and concurrency](https://kotlinlang.org/docs/shared-mutable-state-and-concurrency.html)
- Root `launch` and `async` observe failures differently; cancellation is cooperative and custom suspension can ignore it. [Coroutine exception handling](https://kotlinlang.org/docs/exception-handling.html), [cancellation](https://kotlinlang.org/docs/cancellation-and-timeouts.html)
- Kotlin Multiplatform users reported memory-model, setup, library, and IDE gaps. Cross-target branding preceded complete cross-target experience. [Kotlin Multiplatform survey](https://blog.jetbrains.com/kotlin/2021/01/results-of-the-first-kotlin-multiplatform-survey/)

**Jet disposition:** guarded. I8 rejects scope-function synonym families. Every foreign bridge must validate nullability, ownership, and mutability instead of creating a platform-type escape. Ownership/sendability—not coroutine syntax—proves race freedom. All task constructors need one failure-observation and cancellation law. Multi-target claims require semantic parity tests and first-party library coverage.

### C# and .NET

**What Jet takes:** strong IDE/compiler integration, properties and application ergonomics, structured async APIs, diagnostics, and productive cross-platform libraries.

**Creator and official lessons:**

- Anders Hejlsberg identifies non-nullable references as a foundational feature he wished C# had from the start. Nullable reference types later arrived as optional static analysis over the same runtime types, with warning migrations and a suppression operator. [Hejlsberg retrospective](https://arstechnica.com/civis/threads/an-interview-with-anders-hejlsberg-c-s-lead-architect.104080/), [nullable reference types](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/builtin-types/nullable-reference-types)
- `async void` cannot be awaited, does not expose completion, and routes exceptions outside ordinary caller handling. Official guidance limits it to legacy event handlers. [C# `async`](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/keywords/async), [async return types](https://learn.microsoft.com/en-us/dotnet/csharp/asynchronous-programming/async-return-types)
- Synchronous wrappers around async operations can deadlock under ambient synchronization contexts, producing “async all the way down” pressure and `ConfigureAwait` folklore. [Microsoft async wrapper guidance](https://devblogs.microsoft.com/dotnet/should-i-expose-synchronous-wrappers-for-asynchronous-methods/)
- LINQ's deferred queries run at enumeration, observe later source changes, and may repeat expensive work or return different results on re-enumeration. [Introduction to LINQ Queries](https://learn.microsoft.com/en-us/dotnet/csharp/linq/get-started/introduction-to-linq-queries)
- C#'s design process explicitly recognizes every optional language restriction as added language complexity. [C# language-design repository](https://github.com/dotnet/csharplang)
- NuGet's history shows that a registry requires active security, protocol, tooling, and migration stewardship rather than passive package hosting. [State of the NuGet Ecosystem](https://devblogs.microsoft.com/dotnet/state-of-the-nuget-ecosystem/), [NuGet: Broken By Design](https://devblogs.microsoft.com/dotnet/nuget-is-broken/)

**Jet disposition:** guarded. Safety is never an optional warning mode. Spawned work returns a must-use typed handle; event bridges report failures explicitly. Runtime scheduling cannot depend on ambient UI/server context. Eager collection operations and explicit stream/lazy types keep evaluation timing visible. Package stewardship is part of Jetpack's product contract.

### Python

**What Jet takes:** a welcoming first run, low ceremony, readable code, REPL/notebook flow, rich standard capabilities, and broad domain reach.

**Creator and official lessons:**

- Guido van Rossum has repeatedly regretted the overlapping `lambda`/`map`/`filter`/`reduce` family, especially unreadable `reduce` uses that common named operations should replace. [Computer History Museum oral history](https://archive.computerhistory.org/resources/access/text/2018/07/102738719-05-01-acc.pdf), [Python Regrets](https://legacy.python.org/doc/essays/ppt/regrets/PythonRegrets.pdf)
- The Python Packaging Authority and PSF describe packaging as too complex and fragmented and have pursued a unified experience, legacy retirement, and clearer support. [Python packaging strategy summary](https://pyfound.blogspot.com/2023/02/python-packaging-strategy-discussion.html)
- The GIL became a decades-long multicore constraint whose removal must preserve C-extension compatibility and may re-enable the GIL for extensions lacking thread-safety declarations. [PEP 703](https://peps.python.org/pep-0703/)
- PEP 484 makes type hints optional, erased, and non-enforcing; unchecked code defaults toward `Any`, and separate stubs can drift from implementation. [PEP 484](https://peps.python.org/pep-0484/)
- Python 3 intentionally broke compatibility to repair foundational issues. Migration required dual-source strategies, dependency readiness, warnings, `__future__`, and mechanical tools, and still split the ecosystem for years. [Python 2 deprecation discussion](https://blog.python.org/2011/03/recent-discussion-on-python-dev/), [migration tracker](https://bugs.python.org/issue20812)

**Jet disposition:** guarded. Jet should reproduce Python's first-minute experience with `jet run`, REPLs, notebooks, and a broad Core without weakening its one static language. Types are semantics, not optional lint metadata. FFI thread-safety and foreign execution authority must be explicit. One package graph and one supported workflow prevent Python-style tool families. Editions require automated rewrites and dependency readiness before removal.

### JavaScript

**What Jet takes:** immediate execution, ubiquitous deployment, event-driven applications, and the ability to grow from a tiny script.

**Historical and official lessons:**

- JavaScript was designed under extreme time pressure for small browser scripting, then became an immutable web substrate and a million-line application language. [Eich and Wirfs-Brock, “JavaScript: The First 20 Years”](https://www.cs.tufts.edu/~nr/cs257/archive/brendan-eich/js-hopl.pdf)
- Implicit coercion and loose equality create durable traps; even MDN's own style guide bans most `==` use. [MDN JavaScript style](https://developer.mozilla.org/en-US/docs/MDN/Writing_guidelines/Code_style_guide/JavaScript), [TypeScript from scratch](https://www.typescriptlang.org/docs/handbook/typescript-from-scratch)
- Automatic semicolon insertion makes some line breaks change behavior silently, including `return` followed by a newline. [MDN lexical grammar](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Lexical_grammar)
- `null`, `undefined`, and historical behavior such as `typeof null === "object"` cannot be repaired without breaking the web. [Original JavaScript specification](https://archives.ecma-international.org/1996/TC39/96-002.pdf)
- CommonJS and ESM created a long-running loader split involving `.mjs`, `.cjs`, package `type`, conditional exports, runtime detection, and dual-package hazards. [Node package/module documentation](https://nodejs.org/api/packages.html)
- npm's global dependency graph magnifies tiny-package removals and account compromise. The left-pad incident broke thousands of projects; current npm security work continues tightening identity and publishing. [npm left-pad postmortem](https://blog.npmjs.org/post/141577284765/kik-left-pad-and-npm), [npm supply-chain plan](https://github.blog/security/supply-chain-security/our-plan-for-a-more-secure-npm-supply-chain/)

**Jet disposition:** guarded. No truthiness, coercive equality, duplicate absence values, or ambiguous statement termination. Script and package mode use the same module semantics. A prototype surface never becomes stable merely because it shipped once. Jetpack needs immutable releases, provenance, scoped identity, least-authority builds, and a lock that survives registry incidents.

### TypeScript

**What Jet takes:** strong editor feedback, structural data ergonomics, gradual adoption tooling, and practical JavaScript ecosystem reach.

**Official lessons:**

- TypeScript deliberately accepts unsound operations for JavaScript compatibility, including bivariant methods and unchecked index assumptions. [Type compatibility: soundness](https://www.typescriptlang.org/docs/handbook/type-compatibility.html), [TypeScript FAQ](https://github.com/microsoft/typescript/wiki/faq)
- `any` disables checking and propagates through expressions. Strictness historically depended on configuration rather than being a universal language guarantee. [TypeScript for functional programmers](https://www.typescriptlang.org/docs/handbook/typescript-in-5-minutes-func.html)
- Types erase and do not validate runtime input; assertions ask the checker to trust the programmer rather than converting or checking a value. [TypeScript basics](https://www.typescriptlang.org/docs/handbook/2/basic-types.html)
- Runtime-bearing TypeScript constructs such as namespaces, enums, parameter properties, and `import =` complicated alternate tools and Node's type-stripping path. [TypeScript's migration to modules](https://devblogs.microsoft.com/typescript/typescripts-migration-to-modules/), [TypeScript 5.8](https://devblogs.microsoft.com/typescript/announcing-typescript-5-8/)
- `tsconfig` grew hundreds of options, while module choices vary by Node version, bundler, DOM, worker, and test environment. [tsconfig](https://www.typescriptlang.org/docs/handbook/tsconfig-json), [choosing compiler options](https://www.typescriptlang.org/docs/handbook/modules/guides/choosing-compiler-options.html)

**Jet disposition:** guarded. JavaScript compatibility cannot weaken safe Jet. Foreign/dynamic values remain untrusted until decoded. No ambient `any`, safety flag, or trust-me cast exists outside audited gates. One compiler owns runtime semantics. Expert target pins refine a single typed project model instead of composing flag dialects.

### Julia

**What Jet takes:** interactive scientific work, multiple dispatch's expressiveness, specialization, notebooks, whole-data operations, and native numerical performance.

**Official and survey lessons:**

- Time-to-first-use is a structural JIT cost. Large packages and plotting expose compilation latency; invalidation work and native-code caches arrived later. [Julia invalidations](https://julialang.org/blog/2020/08/invalidations/), [Julia 1.10 highlights](https://julialang.org/blog/2023/12/julia-1.10-highlights/)
- Open-world method extension invalidates compiled code when a new method changes prior dispatch. Some invalidation is unavoidable if unrestricted interactive extension and specialization coexist. [Julia invalidations](https://julialang.org/blog/2020/08/invalidations/)
- Multiple dispatch can become ambiguous as independently authored packages add methods. Resolution may require coordination between developers. [Julia method ambiguities](https://docs.julialang.org/en/v1/manual/methods/)
- Official performance guidance asks users to avoid untyped globals, abstract fields, changing variable types, and value-as-type overspecialization. Beginner-looking code can cross large performance cliffs. [Julia performance tips](https://docs.julialang.org/en/v1/manual/performance-tips/)
- Surveys identify slow compilation and self-contained application generation as major problems. [2023 Julia survey](https://21693537.fs1.hubspotusercontent-na1.net/hubfs/21693537/2023%20Julia%20User%20and%20Developer%20Survey.pdf)
- Older binary packages searched ambient host libraries and ran hand-written build scripts; artifact/JLL work improved reproducibility but initially added load latency. [Julia artifacts](https://julialang.org/blog/2019/11/artifacts/), [Julia 1.6 highlights](https://julialang.org/blog/2021/03/julia-1.6-highlights/)

**Jet disposition:** guarded and watch. Production dispatch remains coherent and closed enough for stable compilation; interactive redefinition belongs to dev mode with visible invalidation. Static types should remove common performance cliffs, while inspection explains allocations, dispatch, and device transfers. Release binaries stay self-contained. Native dependencies use exact content-addressed artifacts rather than ambient search paths.

## Functional, concurrent, and language-oriented systems

### OCaml

**What Jet takes:** algebraic data, exhaustive matching, strong inference, value-oriented programming, and expressive modules.

- OCaml's universal polymorphic comparison has surprising semantics, can fail at runtime for functions and external values, and performs poorly enough that Jane Street removed it from Core's ordinary path. [Removing polymorphic compare from Core](https://discuss.ocaml.org/t/removing-polymorphic-compare-from-core/2994), [runtime crash example](https://discuss.ocaml.org/t/generic-compare-that-crash-at-runtime/7411)
- Separately authored interface files can duplicate the source contract and drift from the implementation, even though the module system itself is powerful. [OCaml modules](https://ocaml.org/docs/modules)

**Jet disposition:** guarded. Equality and ordering must be sema-proven for a type; function, resource, and foreign-handle comparison is rejected unless a sound explicit operation exists. Public interfaces and semantic indexes derive from checked declarations rather than shadow source files.

### F#

**What Jet takes:** approachable ML inference, algebraic data, pipelines, pragmatic object/functional interop, and excellent data-work ergonomics.

- Don Syme says attempts to decide semantics too early in the compiler are usually regretted because feature interaction belongs in the typechecker. He also notes that powerful abstraction features are difficult to unwind after adoption. [Don Syme compiler interview](https://www.patrickstevens.co.uk/posts/2018-09-10-don-syme/)
- .NET interop imports host nullability, erased unit annotations, and C#-shaped APIs unless the boundary actively repairs them. [F# component design guidelines](https://learn.microsoft.com/en-us/dotnet/fsharp/style-guide/component-design-guidelines)

**Jet disposition:** guarded. The parser describes; sema decides. Host models cannot leak through FFI as Jet facts. Units, ownership, nullability, effects, and failures remain present in Jet metadata even when a foreign ABI erases them.

### Haskell

**What Jet takes:** purity, algebraic abstraction, effect separation, type-driven APIs, and strong equational reasoning.

**Creator history and community evidence:**

- Haskell's creators describe laziness as both a defining experiment and a source of space leaks; strict features became necessary. Early I/O designs were painfully clumsy before monadic I/O unified the model. [“A History of Haskell: Being Lazy with Class”](https://simon.peytonjones.org/assets/pdfs/haskell-being-lazy-with-class.pdf)
- A 2026 community consultation repeats long-standing requests: replace `[Char]` as the default string representation, remove partial Prelude defaults, repair records, reduce extension sprawl, improve compile time and errors, and integrate GHC/Cabal/Stack/HLS/GHCup. [“What Would You See Changed in Haskell?”](https://blog.haskell.org/what-would-you-see-changed-in-haskell/)
- End-user tooling depends on several independently released components whose compatible versions do not always arrive together. [Towards a better end-user experience in tooling](https://discourse.haskell.org/t/towards-a-better-end-user-experience-in-tooling/5512)

**Jet disposition:** guarded. One `String`, total collection defaults, one record model, one normal language edition, and one tested tool distribution avoid the accumulated baseline problem. Jet should take purity and typed effects without requiring a monad-first standard library or extension-selection culture.

### Gleam

**What Jet takes:** a small typed surface, exhaustive data, one main pipe, beginner-friendly functional code, and BEAM/JavaScript reach.

- Gleam externals cannot be verified by its compiler or language server and may make a project single-target. BEAM and JavaScript concurrency/IO models differ enough that libraries often choose one target. [Gleam externals](https://gleam.run/documentation/externals/), [multi-target externals](https://tour.gleam.run/advanced-features/multi-target-externals/)
- The BEAM global module namespace and Hex's unscoped package names create collisions and packages that can appear official. [Gleam publishing changes](https://gleam.run/news/improved-performance-and-publishing/)
- Gleam's `use` sugar benefits from explicit editor actions that reveal and restore its desugared form. [Gleam publishing and tooling post](https://gleam.run/news/improved-performance-and-publishing/)

**Jet disposition:** guarded. Externals are checked adapters, not declarations trusted on faith. Every Core API states target support and passes cross-target semantic tests. Package identity and display include provenance. Every sugar has one inspectable lowering and formatter/editor round trip.

### Erlang and Elixir

**What Jet takes:** supervision, fault containment, lightweight tasks, message passing, service lifecycle discipline, and rolling-operation experience.

- Erlang's own FAQ calls records ugly and error-prone because textual include files can define incompatible copies without protection; many type errors surface only at runtime. [Erlang historical FAQ](https://www.erlang.org/faq/academic.html)
- Static typing is one of Elixir's most important community requests, but retrofitting it must accommodate an existing dynamic world of macros, patterns, unions, recursive types, and tests. [The Design Principles of the Elixir Type System](https://arxiv.org/abs/2306.06391)
- Hot upgrades cannot make already-running state and code type-safe merely because new source compiles. [Gleam FAQ on hot code upgrades](https://gleam.run/frequently-asked-questions/)

**Jet disposition:** guarded and watch. Nominal data and imports replace textual schemas. Jet takes supervision on top of a type-first language. Any future hot replacement needs versioned state, typed migrations, compatibility proofs, and rollback; ordinary compile safety must never be advertised as live-upgrade safety.

### Clojure

**What Jet takes:** a small semantic core, persistent data, data-oriented programming, REPL-driven development, and composable transformations.

- Clojure surveys repeatedly rank error messages, startup, tooling, documentation, and hiring among the ecosystem's main frustrations. Dynamic and lazy execution can move failure away from its cause. [2016 survey](https://clojure.org/news/2017/01/31/state-of-clojure-2016), [2025 survey](https://clojure.org/news/2026/02/18/state-of-clojure-2025)
- Clojure spec is separate, opt-in runtime instrumentation; function and return specifications can be tested rather than universally enforced. [Clojure spec guide](https://clojure.org/guides/spec)
- Rich Hickey's governance retrospective says imprecise communication about decision authority created avoidable confusion; the project deliberately prefers stability over accepting every contribution. [Clojure governance](https://clojure.org/news/2012/02/17/clojure-governance)

**Jet disposition:** guarded. Diagnostics, startup, docs, and tooling are release criteria. Safety and type invariants never depend on optional instrumentation. A small language still needs explicit governance and a strict evolution budget.

### Racket

**What Jet takes:** language-oriented programming, a tiny extensible core, hygienic transformation ideas, and excellent teaching/tooling integration.

- Racket contracts can impose enough runtime cost that documentation recommends parallel unsafe `no-contract` submodules. [Racket Language and Performance](https://docs.racket-lang.org/style/Language_and_Performance.html)
- `#lang` enables whole file-local languages. The power is real, but different files may obey different semantics and require language-specific tools. [The Racket Manifesto](https://www2.ccs.neu.edu/racket/pubs/manifesto.pdf)

**Jet disposition:** guarded. Jet contracts remain mandatory in every profile and may disappear only when proved redundant. There is no ordinary safe/unchecked twin API. DSLs and generated source re-enter one Jet frontend; plain Jet remains complete and tools never need to guess which user-defined language a file contains.

### Smalltalk

**What Jet takes:** one message model, live inspection, immediate feedback, object exploration, and tools that expose runtime state.

- Image-based persistence creates a superb live environment but complicates collaboration, version control, deployment, and schema migration. Later typed-image research explicitly revisits these limitations. [Typed Image-based Programming](https://arxiv.org/abs/2110.08993), [Representing Code History](https://arxiv.org/abs/1309.4334)
- Uniform dynamic messages keep the surface elegant, but missing-message and type errors remain runtime events.

**Jet disposition:** guarded. Plain text and ordinary version control are complete source truth; live images, REPL state, indexes, and Canvas are rebuildable views or caches. Jet can use one call/message meaning without dynamic fallback.

### Hazel

**What Jet takes:** typed holes, useful feedback for incomplete programs, and principled editor support during errors and merge conflicts.

- Hazel is explicitly a research environment rather than mature ecosystem evidence. Its central result is that incomplete programs can retain static and dynamic meaning instead of making tools fall back to ad-hoc guesses. [Hazel project](https://hazel.org/)
- Hazel's own gradual-structure-editing research describes the awkwardness of strictly tree-structured editing as a remaining limitation. The research response is to make editing progressively more text-like, not to require every user to manipulate an AST directly. [Gradual Structure Editing with Obligations](https://hazel.org/papers/teen-tylr-vlhcc2023.pdf)

**Jet disposition:** guarded. The compiler and editor should preserve expected types, bindings, and partial diagnostics around holes or errors, but ordinary source remains freely editable text. Typed holes may support tooling without becoming deployable runtime values or forcing structural editing.

### JetBrains MPS and lens research

**What Jet takes:** multiple projections over one semantic program, domain views, structural transformations, and round-trip-aware tooling.

- MPS stores and versions the AST rather than ordinary source text. Its official FAQ says special diff/merge infrastructure is required because generic text tools see a persistence format rather than the user's concrete syntax. [MPS FAQ](https://www.jetbrains.com/help/mps/mps-faq.html)
- Projectional editing has unfamiliar selection and deletion behavior; MPS documents cases where deleting punctuation removes a whole statement and where invalid text shown in a cell is not yet the model value. [MPS editor documentation](https://www.jetbrains.com/help/mps/editor.html)
- Arbitrary language composition avoids parser ambiguity, but semantic composition and language-specific tooling remain separate work. [MPS FAQ](https://www.jetbrains.com/help/mps/mps-faq.html)

**Jet disposition:** guarded. Beginner, exact, generated, audit, graph, and domain views may project the same semantic facts, but every edit round-trips to normal Jet text and generic Git tools remain sufficient. Canvas and lenses never become the only truthful editor or persistence format.

### Verse

**What Jet takes:** explicit failure contexts, transactional rollback, and an effect distinction between rollback-safe and non-rollback work.

- Verse requires failable expressions to appear in failure contexts and rolls back prior mutations when a later expression fails. The visibility of the context is the important safeguard. [Epic's failure and control-flow guide](https://dev.epicgames.com/documentation/en-us/fortnite/basics-of-writing-code-9-failure-and-control-flow-in-verse)
- Rollback is not universal: `no_rollback` operations cannot run in failure contexts, and native functions are not checked by the same rollback validator. [Verse glossary](https://dev.epicgames.com/documentation/en-us/fortnite/verse-glossary), [Verse API effect example](https://dev.epicgames.com/documentation/fortnite/verse-api/fortnitedotcom/devices/tracker_device/reset)
- Verse is still too young for a durable creator-regret or broad ecosystem ledger. The useful evidence is the semantic boundary, not a claim that its surface has proved optimal.

**Jet disposition:** watch. Ordinary `?` propagation never implies rollback. If Jet adds checked transaction regions, the region must be explicit, sema must prove every effect rollback-safe or compensated, native/FFI calls need typed rollback contracts, and audit output must show commit and compensation boundaries.

### Koka

**What Jet takes:** inferred effect rows, explicit handlers where useful, and research-backed effect semantics.

- Tracking every effect gives strong guarantees, but scoped and higher-order effects need forwarding/scoping machinery, and evidence lookup can add runtime and implementation cost. [Koka effect rows](https://arxiv.org/abs/1406.2061), [scoped effects](https://arxiv.org/abs/2304.09697), [effect evidence](https://www.dhil.net/research/papers/effect_handlers_evidently-extended-icfp2020.pdf)
- Koka describes itself as a research language rather than production ecosystem evidence. [Koka repository](https://github.com/koka-lang/koka)

**Jet disposition:** owned. Infer common private effects, reveal them on request, and pin public/audit boundaries. Dense rows and handler calculus must not dominate beginner code. D-SHAPE-EFFECTOMIT1 and existing effect work already own this lesson; no new ballot is needed.

### Pony

**What Jet takes:** capabilities as authority, actor isolation, race freedom, and runtime/type-system co-design.

- Pony's official learning material calls reference capabilities a major stumbling block. Six capability modes, recovery, and viewpoint adaptation expose a large proof vocabulary on ordinary references. [Pony reference capabilities](https://www.ponylang.io/learn/reference-capabilities/), [capability combination tables](https://tutorial.ponylang.io/reference-capabilities/combining-capabilities.html)
- Pony's guarantees depend on co-design across its type system, actors, scheduler, and garbage collector. [Pony papers](https://www.ponylang.io/learn/papers/)

**Jet disposition:** guarded. Keep the beginner ownership surface to read, write-borrow, take, and explicit copy. Infer richer authority facts and expose them at expert/concurrent boundaries. Never copy capability syntax without the matching runtime invariant.

### Roc

**What Jet takes:** friendly static functional programming, expected-type shorthand, explicit application platforms, and aggressive simplification.

- Roc rejects higher-kinded polymorphism because it predicts alternative monad-first standard libraries and ecosystem split. It rejects default currying because too-few-argument mistakes become remote type mismatches. [Roc FAQ](https://www.roc-lang.org/faq)
- The app/platform split can create coherent domain experiences, but multiple and composed platforms remain an unresolved product question. [Roc FAQ](https://www.roc-lang.org/faq)
- Roc's compiler rewrite and deprecated syntax demonstrate how pre-1 churn invalidates examples and learning assets. [Roc homepage](https://roc-lang.org/), [Roc examples](https://www.roc-lang.org/examples/)

**Jet disposition:** guarded. Fixed-arity calls, explicit lambdas, one Core abstraction family, and zero-config first run fit the lesson. Platform/provider selection layers over ordinary app semantics and remains revealable to experts. Examples, grammars, migrations, and compiler support update atomically.

## Configuration, proof, semantic-identity, and array languages

### Nix

**What Jet takes:** hermetic builds, content-addressed artifacts, reproducibility, declarative environments, and immutable activation.

- Practical Nix composition spans the language, builtins, library, builders, `override`, `overrideAttrs`, overlays, `callPackage`, and the NixOS module system. One deployment idea acquired many overlapping composition layers. [Nix language basics](https://nix.dev/tutorials/nix-language.html)
- Flakes attempted to solve identity, locking, schemas, CLI behavior, composition, and cross-compilation together. Experimental adoption made unresolved abstraction choices costly to change. [Nix developer manual and flake history](https://nix.dev/nix-dev.pdf), [NixOS Flakes wiki](https://wiki.nixos.org/wiki/Flakes)
- Flakes can copy large repositories into the store, producing avoidable time, disk, and confusing source-inclusion behavior. [Scaling Up Flakes](https://av.tib.eu/media/61027)
- Lazy recursive modules and overlays can fail late through library frames or infinite recursion. Nix's many composition forms also create shadowing and shallow-update traps. [Nix best practices](https://nix.dev/guides/best-practices.html)

**Jet disposition:** guarded. The ratified ecosystem shape has one typed graph, one merge law, one lock, explicit `OptionValue` precedence, eager graph validation, source inclusion explanations, immutable generations, and reclaimability reports. Content addressing must not imply unconditional whole-tree copies.

### Dhall

**What Jet takes:** typed configuration, total evaluation, imports with integrity, normalization, and safe data transformation.

- Dhall's totality and safety deliberately exclude general recursion and several ordinary operations. Those guarantees can make deep updates and common data algorithms verbose or impossible. [Dhall design choices](https://docs.dhall-lang.org/discussions/Design-choices.html), [Dhall language tour](https://docs.dhall-lang.org/tutorials/Language-Tour.html)
- Large normal forms and semantic-cache serialization have caused extreme compile time and memory use. [Dhall slow compile times](https://discourse.dhall-lang.org/t/slow-compile-times/634)
- Creator survey results identify performance, difficult errors, and semantic-integrity upgrade friction as recurring issues. [Dhall survey results](https://haskellforall.com/2019/02/dhall-survey-results-2019-2019)

**Jet disposition:** guarded. `pure` restricts effects and nondeterminism, not every finite data algorithm. Hash incrementally rather than materializing a full normal form. Semantic hash versions belong in lock contracts, and errors must name both producer and consumer.

### CUE

**What Jet takes:** order-independent unification, constraints as data, and conflict diagnostics that preserve both sources.

- CUE's lattice/unification model prevents last-file-wins behavior, but defaults, disjunctions, closedness, and empty-disjunction errors require a substantial logic engine. [CUE specification](https://cuelang.org/docs/reference/spec/), [CUE disjunctions](https://cuelang.org/docs/tour/types/disjunctions/)
- CUE explicitly incorporates lessons from earlier graph-constraint work: composition algebra has to be designed as one law rather than grown from overrides. [CUE introduction](https://cuelang.org/docs/introduction/)

**Jet disposition:** guarded. Use commutative compatible-fact merge with typed finite categories and full provenance. Do not turn ordinary Jet into a general logic/disjunction language. Conflicts show both origins and the exact path.

### Nickel

**What Jet takes:** typed configuration, mergeable records, contracts, priorities, and gradual schema enforcement.

- Lazy contracts may delay missing fields until later merges or export; union-like contracts can choose surprising branches. [Correctness in Nickel](https://nickel-lang.org/user-manual/correctness/), [Nickel contracts](https://nickel-lang.org/user-manual/contracts/)
- User contracts may transform values. The evaluator assumes idempotence but cannot enforce it, so deduplication and refactoring can change behavior for a non-idempotent contract. [Nickel contracts](https://nickel-lang.org/user-manual/contracts/)
- Field-attached and free-standing contracts look similar but interact with merge differently. [Nickel contracts](https://nickel-lang.org/user-manual/contracts/)

**Jet disposition:** guarded. Jet may evaluate lazily internally, but build, export, plan, and audit deeply validate all reachable output. Contracts are pure and observational; conversion is a separate named operation. Annotation placement cannot secretly change precedence.

### Unison

**What Jet takes:** semantic identity, dependency-aware refactoring, content-addressed definitions, and strong inspection.

- Unison stores code and history in a database. Its FAQ describes limited pruning and database-oriented sharing, while hashes act as true names behind user names. [Unison FAQ](https://www.unison-lang.org/docs/usage-topics/general-faqs/), [Unison hashes](https://www.unison-lang.org/docs/language-reference/hashes/)
- The model weakens ordinary text/Git tooling and can expose hashes during naming mistakes. This is a recurring community concern rather than a creator admission.

**Jet disposition:** guarded. Semantic hashes power caches, identity, impact analysis, and inspection behind ordinary files. They never become required source spelling or require a database editor. History and cache garbage collection must be defined before durable adoption.

### Eiffel

**What Jet takes:** contracts beside code, invariant-driven design, readable intent, and strong static guarantees.

- Parameter covariance combined with polymorphism produced “catcalls”; Eiffel's own community material acknowledges that the language was not completely type-safe until additional rules addressed the hole. [Catcall solution](https://www.eiffel.org/node/251), [Type Safe Eiffel](https://www.eiffel.org/node/187)
- Void safety arrived as a later retrofit rather than an original universal invariant. [Eiffel void safety](https://www.eiffel.org/doc/eiffel/Void-safety-_how_Eiffel_removes_null-pointer-dereferencing)
- Rescue/retry can turn contract failure into control flow, blurring broken program promises with recoverable domain errors. [Eiffel exception mechanism](https://www.eiffel.org/doc/solutions/Exception_Mechanism)

**Jet disposition:** guarded. No covariant parameter override, inheritance, universal nullability, or contract-retry path. Trait variance remains explicit and sound. Contract failure is an uncaught runtime contract stop, distinct from `T ? E`.

### APL and BQN

**What Jet takes:** whole-data operations, rank-aware thinking, compact algebra, and optimizer-friendly array semantics.

- BQN's creator identifies APL's nested/boxed array turns and BQN's empty-array type loss as persistent model problems; fill values only partly recover the missing facts. [Based array theory](https://mlochbaum.github.io/BQN/doc/based.html), [Problems with BQN](https://mlochbaum.github.io/BQN/problems.html)
- Glyph scarcity forces unrelated monadic and dyadic operations to share symbols, while high-rank notation remains hard to read. [Problems with BQN](https://mlochbaum.github.io/BQN/problems.html)
- Dynamic coarse typing moves rank, shape, and empty-edge behavior to runtime conventions. [BQN paradigms](https://mlochbaum.github.io/BQN/doc/paradigms.html)

**Jet disposition:** guarded. Empty collections require expected type when inference is not unique. Shape and element type remain static where promised. Core provides named whole-data operations; the optimizer supplies fusion. Dense valence/rank glyph overloading is not required public style.

## Cross-language design rules for future Jet work

These are research conclusions, not new syntax decisions.

### Memory, views, and resources

- A safe view carries owner lifetime, bounds, element type, and mutability through sema. It cannot be constructed from a temporary whose lifetime ends first, survive invalidating mutation, or decay to a raw pointer in safe code.
- Deterministic cleanup is the default. Early release is one ordinary operation, not a second lifetime system. Finalizers are not correctness mechanisms.
- Optimization, freestanding, embedded, kernel, and release-size profiles preserve the same safety contract.
- Foreign ownership and strings are translated at the boundary; no host sentinel, platform null, ARC convention, or unchecked annotation becomes a Jet invariant.

This evidence belongs on the existing memory/view/resource work, especially cards #557, #567, and #642–#649, rather than on duplicate ballots.

### Types, traits, inference, and errors

- Public generics, traits, error sets, effect sets, and wire types must be stable and explicit enough to survive separate compilation and package evolution.
- Private inference is permitted only when the answer is unique and nearby; public/audit boundaries can pin and inspection can reveal the inferred fact.
- Equality, ordering, conversion, serialization, and dispatch are trait/type-directed. There is no universal reflective fallback.
- Jet's existing orphan law—type or trait local—is the coherence baseline. Glue-package needs should be solved through explicit upstream/local wrappers, not global priority dispatch.
- Type-checking work receives deterministic complexity and diagnostic-quality budgets. “Could not type-check in reasonable time” is not an acceptable substitute for the actual user error.

### Tasks, effects, and live systems

- One task model owns creation, cancellation, deadlines, failure observation, cleanup, child lifetime, scheduling, and audit output.
- No `async void`, detached-by-default work, builder-specific failure law, or ambient synchronization-context capture.
- Race freedom comes from ownership/sendability and checked shared-state primitives, not coroutine syntax.
- Hot reload or rolling upgrade requires typed state versions and migrations. It is a separate proof from source compilation.
- Comptime, build scripts, hooks, macros, and foreign loaders declare effects and host/target identity; undeclared network, filesystem, process, environment, or credential access fails closed.

### Packages, tools, and evolution

- One package identity includes publisher/provenance, version, content, target facts, and dependency reason. Names alone are insufficient trust.
- Published versions are immutable; deletion cannot break locked builds; the registry is not the only availability root.
- One resolver, one lock model, one build graph, one formatter, one language server, one diagnostic registry, and one release compatibility matrix.
- Edition migration is executable: deprecation diagnostics, `jet fix`, package-graph readiness, an explicit edition bump, and removal gates exist before a break ships.
- Binary resilience, ABI stability, and source-package evolution are different contracts. Jet must choose binary-library rules before promising stable binary distribution, not infer them from source compatibility.

### Configuration, contracts, and proof

- Compatible contributions merge independent of file order. Conflict reports include both sources, the field path, and the reason no common value exists.
- Lazy evaluation is an implementation technique. A successful build, export, deployment plan, decode, or audit has validated every reachable result and contract required by that operation.
- Contract conditions are pure, observational, checked in every profile, and removed only by proof. They cannot normalize data, acquire authority, or catch themselves as ordinary domain failure.
- Proof output distinguishes universal proof, test observation, assumption, foreign trust, incomplete analysis, timeout, unsupported formula, and counterexample. “No issue found” is never reported as proof.
- Semantic hashes and indexes support source, but plain named text remains complete truth.

## Ballot assessment

**No new Tower ballot was raised.** Research surfaced four plausible decision areas, but the current repository audit found three already settled and one not yet concrete:

| Candidate | Audit result | Disposition |
|---|---|---|
| Trait coherence and retroactive implementations | `docs/spec/spec.md` and `syntax-decisions.md` already require an ordinary orphan/coherence law: the type or trait must be local; duplicates are rejected. | No ballot. Add hostile cross-package tests when trait work expands. |
| Contract purity, profile behavior, and failure meaning | D-PREPOST1 already requires pure conditions, checks in every build, and E3005 uncaught contract failure distinct from typed domain errors. | No ballot. Preserve in optimizer/proof verification. |
| Deep validation of lazy config/build output | The ratified ecosystem shape already requires eager typed graph checks, provenance, and source-local errors at the graph boundary. | No ballot. Turn the conclusion into acceptance coverage, not another choice. |
| Stable binary-library resilience/ABI | Swift and C++ show that this must be deliberate, but Jet has not yet reached a concrete binary-library distribution contract that requires an owner choice. | Watch. Ballot before promising binary compatibility, enum resilience, inlinable ABI, or cross-toolchain Jet layout. |

Two other apparent candidates were also duplicates: language evolution is already owned by the ratified editions/deprecation/`jet fix` contract, and effect omission is already owned by D-SHAPE-EFFECTOMIT1 and the effects roadmap.

The correct action is to attach evidence to existing work, not increase the owner's decision queue. A future ballot is appropriate only when implementation reaches an unsettled semantic fork that existing law cannot answer.

## Priority follow-through

1. **Memory/view cards:** add C array-decay, C++ dangling-view, Rust ergonomics, and Pony capability cases to the adversarial corpus for #557, #567, and #642–#649.
2. **Task/runtime acceptance:** require one failure/cancellation/cleanup law across every task constructor, callback, event bridge, FFI adapter, and blocking-looking API.
3. **Type-checker budgets:** add hostile overload/inference/diagnostic cases inspired by Swift, Zig error sets, Julia dispatch, and OCaml comparison. The compiler must explain the real error within a bounded budget.
4. **Build authority:** verify comptime, hooks, generators, native dependencies, and foreign probes cannot execute undeclared effects; include Cargo-build-script and npm-style supply-chain cases.
5. **Package identity and availability:** test scoped identity, immutable releases, registry deletion, offline locked builds, dependency feature isolation, provenance display, and lock/receipt agreement.
6. **Cross-target truthfulness:** for every marketed backend, run the same Core semantics, ownership, error, task, codec, and diagnostic corpus. Externals never count as parity evidence.
7. **Release integration:** test compiler, LSP, formatter, package manager, docs, and migrations as one versioned distribution rather than independently green components.
8. **Future ABI gate:** before binary Jet libraries become public, decide layout, generic reification, enum resilience, default arguments, inlining, toolchain skew, and migration together.

## Scope notes

This report focuses on language and ecosystem failures relevant to Jet's documented inspiration set. It does not rank languages, claim that a listed issue outweighs a language's successes, or treat every later redesign as proof that the original designers were careless. Many choices were rational under hardware, compatibility, schedule, or adoption constraints that Jet does not share.

That distinction matters. Jet should copy the successful invariant, understand the constraint that produced it, and decline the historical baggage that was necessary elsewhere. “Stand on the shoulders of giants” means learning from both the feature and the repair bill.
