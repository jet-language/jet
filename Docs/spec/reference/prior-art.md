# Prior art

This reference compares Jet's language and tooling contracts with ideas from
other languages and ecosystems. It is for readers evaluating a design choice,
not a compatibility promise. Executable truth is in the
[syntax decisions](../syntax-decisions.md), the
[metaprogramming reference](metaprogramming.md), the
[Jetpack reference](jetpack-epoch5.md), the
[foundation syntax ledger](../../../crates/jet-foundation/src/Syntax.rs), and
the checked examples under `Examples/`.

## How to read the comparison

Each comparison separates three questions:

- **Jet takes:** a useful mechanism or observation that fits Jet's typed,
  effect-aware, inspectable model.
- **Jet rejects:** a mechanism whose implicitness, unchecked authority, or
  second grammar conflicts with that model.
- **Why:** the boundary that keeps the same source fact visible to checking,
  interpretation, code generation, and tooling.

The links at the end are the reading record. They provide context and
counterexamples; they do not establish a language guarantee on their own.

## Systems and safety

### C and C++

**Jet takes:** C's direct data and ABI model, and C++'s zero-cost abstraction,
generic programming, deterministic resource ownership, and explicit layout
remain useful constraints for systems code. Jet exposes those concerns through
typed values, named unsafe boundaries, checked slices, explicit layout facts,
and a compiler-owned representation shared by its execution paths.

**Jet rejects:** unchecked pointer arithmetic, implicit narrowing, raw-array
decay, C-style casts, dangling views, and inheritance as the default modeling
tool. A low-level escape must remain a named, reviewable boundary; the source
language does not make an unchecked operation safe merely because it is
familiar in C or C++.

**Why:** a layout or ownership fact must survive semantic checking and remain
available to code generation and inspection. The contract is about the fact,
not about preserving a particular C spelling.

### Rust

**Jet takes:** ownership-oriented APIs, deterministic cleanup, exhaustive
sum-type handling, strong diagnostics, and an integrated package workflow.
Rust's experience also makes the cost of coherence, feature composition, build
scripts, and foreign authority visible.

**Jet rejects:** treating a token-stream macro or a build script as an
unbounded second compiler, or treating a provider's missing identity as a
guessable default. Jet's typed generation and package facts stay in one
language-owned graph; foreign tools remain explicit boundaries.

**Why:** ownership and provenance are useful only when they are carried by
the same checked facts. A source transform that cannot be inspected in the
typed graph is not an equivalent of typed metaprogramming.

### Zig

**Jet takes:** explicit allocation and error decisions, a small surface, and
version-matched local documentation as useful design pressures.

**Jet rejects:** making manual lifetime or unchecked pointer conventions the
ordinary burden of every caller. Jet's ordinary values, effects, and resource
contracts make the safe path the default while reserving named boundaries for
host interaction.

**Why:** explicitness is valuable when it names an invariant or authority.
Ceremony without a corresponding fact does not improve the contract.

## Compile-time and metaprogramming

### Jai-style compile-time execution

**Jet takes:** compile-time computation can make tables, constants, and
declarations from the same typed source as the program. Jai's emphasis on
staged language work is a useful comparison for keeping migration and
generation understandable.

**Jet rejects:** an unbounded compile-time escape that can silently change the
language grammar or acquire undeclared host authority. Jet's `@` form is a
compile-time value, block, or fact; build entries, derives, and generated names
remain typed operations.

**Why:** compile-time output must be attributable to an evaluation site and
its allowed effects. A generated declaration is checked as a declaration, not
re-lexed from arbitrary source text.

### Rust procedural macros

**Jet takes:** deriving repetitive declarations from a type and keeping
generated code close to its declaration.

**Jet rejects:** treating a token stream as a general-purpose syntax channel.
Jet's derive and generation bodies use ordinary typed item templates. They
cannot introduce a second parser or hide an arbitrary re-lexing pass.

**Why:** a typed template exposes names, types, effects, and source spans to
the same semantic machinery as hand-written code. This makes generated output
reviewable without requiring readers to reverse-engineer a macro protocol.

### Python decorators and generators

**Jet takes:** decorators show how a local declaration can receive reusable
behavior, and generators show the value of explicit suspend/resume state and
one-pass streams.

**Jet rejects:** an implicit runtime decorator mechanism that changes a
callable's meaning outside its type, and memory or performance claims that are
not part of a stream's contract. Jet uses typed derives and explicit stream
operations instead.

**Why:** transformations and suspension points belong in the callable's
checked contract. A runtime convention should not become an invisible second
type system.

### Verse and transactional state

Verse is a useful comparison for transactional rollback and failure-aware
state. Jet takes the discipline of making failure and rollback boundaries
explicit, but does not use a comparison language's runtime semantics as a
replacement for its own effects, ownership, or package facts.

## API and function design

### Function-design canon

A function should expose the facts its caller must reason about: meaningful
inputs, the result, effects, and the invariant that determines failure. Jet
keeps pure calculation separate from host interaction and lets types carry
stable identities and valid-state constraints.

```jet
fn parse(input: String) -> ParseResult { … }
fn write(path: Path, text: String) -[FS]> WriteResult { … }
```

The effect row is part of the signature. A result type names failure; a bare
`!` is not a complete return contract. Narrow functions compose because a
caller can see which authority they need without reading an implementation.
The sources below compare honest signatures, value-oriented APIs, non-member
operations, and single responsibility. Jet uses those observations as API
review criteria rather than as a second syntax.

## Packages, builds, and ecosystems

Cargo demonstrates the value of an integrated package graph, while Nix
demonstrates reproducible inputs and explicit host/platform facts. Jet takes
those goals through `package.jet`, `env.jet`, one `.jet/lock`, provider
identity, and a content-addressed Hangar. The [Jetpack reference](jetpack-epoch5.md)
defines the package, source, environment, service, and cache boundaries.

**Jet takes:** exact source selectors, typed package outputs, deterministic
member discovery, lock records that retain provider and content identity,
offline replay from verified local bytes, and host-owned credentials.

**Jet rejects:** a second manifest grammar, a mutable selector hidden behind
an unqualified name, network fallback during offline replay, guessed provider
metadata, and repository-controlled cache credentials. A foreign provider
either supplies the required fact or leaves an explicit loss/error.

**Why:** package identity, build inputs, outputs, and authority must be
inspectable before realization. A convenient shortcut that loses one of those
facts makes reproducibility and recovery depend on ambient machine state.

## Domain observations

The source record spans domains, but it is not a compatibility matrix. These
are design observations, not claims that Jet implements every framework named
by a source.

| Domain | Useful observation | Jet boundary |
| --- | --- | --- |
| Web | Route, data-loading, mutation, and UI state form different facts. | Keep them typed and explicit rather than hiding server calls in view syntax. |
| Live tooling | Inspectable state and time-travel debugging help explain a system. | Expose checked facts and deterministic projections; do not add a second runtime debugger language. |
| Games | Frame budgets, ownership, and data layout interact. | Make resource and layout choices explicit; measure rather than promise a performance tier. |
| Backend services | Readiness, cancellation, and dependency order are operational contracts. | Typed service records and effect rows carry those facts into Jetpack. |
| Mobile | Platform capabilities and credentials need a host authority. | Keep target and grant facts in the environment plan; never embed secret values. |
| Data and science | Columnar layout, streaming, and reproducible inputs affect results. | Preserve typed layout and input identity; do not infer numeric or memory guarantees from a library name. |
| CLI and devtools | Good tools preview mutation and explain provenance. | Use one checked plan for read-only inspection, realization, and recovery. |

## Programming languages

### C

C supplies the low-level comparison for object representation, array bounds,
integer conversion, preprocessing, and the limits of retrofit safety. Jet keeps
the useful representation facts while requiring them to pass through typed
checking and explicit unsafe authority.

- WG14 N2659 (ordinary C safety) —
  https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2659.htm
- WG14 N1990, N2660, and N3360 (array bounds) —
  https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1990.htm
  https://www.open-std.org/jtc1/sc22/wg14/www/docs/n2660.pdf
  https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3360.htm
- WG14 N1967 (Annex K) —
  https://open-std.org/jtc1/sc22/wg14/www/docs/n1967.htm
- WG14 N1400 and N2896 (preprocessor and headers) —
  https://www.open-std.org/JTC1/SC22/wg14/www/docs/n1400.htm
  https://open-std.org/jtc1/sc22/wg14/www/docs/n2896.htm
- WG14 N1254 and N2885 (integer promotion and overflow) —
  https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1254.htm
  https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2885.pdf
- ACM Queue, “The Most Expensive One-byte Mistake” —
  https://queue.acm.org/detail.cfm?id=2010365

### C++

C++ supplies comparisons for zero-cost abstraction, native interoperation,
generic code, lifetimes, views, and dimensional types.

- Stroustrup interviews —
  https://www.stroustrup.com/slashdot_interview.html
  https://www.stroustrup.com/devXinterview.html
  https://www.stroustrup.com/italian_interview.html
- P2771R1, memory-safety views —
  https://isocpp.org/files/papers/P2771R1.html
- CppCoreGuidelines view-lifetime issue —
  https://github.com/CppCoreGuidelines/issues/2276
- mp-units, points and quantities —
  https://mpusz.github.io/mp-units/latest/tutorials/affine_space/points_and_quantities/
  https://mpusz.github.io/mp-units/latest/tutorials/affine_space/points_and_quantities

### Rust

Rust supplies comparisons for ownership, enums, patterns, API predictability,
coherence, asynchronous design, package resolution, and supply-chain
boundaries.

- Graydon Hoare, “The Rust I Wanted Had No Future” —
  https://graydon2.dreamwidth.org/307291.html
- Rust language roadmap, State of Rust, and compiler performance —
  https://blog.rust-lang.org/inside-rust/2022/04/04/lang-roadmap-2024/
  https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/
  https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results/
  https://blog.rust-lang.org/inside-rust/2022/04/04/lang-roadmap-2024
  https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results
  https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results
- Async project goals —
  https://rust-lang.github.io/rust-project-goals/2024h2/async.html
  https://rust-lang.github.io/rust-project-goals/2026/roadmap-just-add-async.html
- Relaxing the orphan rule and RFC 2451 —
  https://rust-lang.github.io/rust-project-goals/2024h2/Relaxing-the-Orphan-Rule.html
  https://github.com/rust-lang/rfcs/blob/master/text/2451-re-rebalancing-coherence.md
- Little Orphan Impls —
  https://smallcultfollowing.com/babysteps/blog/2015/01/14/little-orphan-impls/
  https://smallcultfollowing.com/babysteps/blog/2015/01/14/little-orphan-impls
- Cargo build scripts, features, workspaces, and resolver —
  https://doc.rust-lang.org/stable/cargo/reference/build-scripts.html
  https://doc.rust-lang.org/cargo/reference/features.html
  https://doc.rust-lang.org/cargo/reference/workspaces.html
  https://doc.rust-lang.org/nightly/cargo/reference/resolver.html
- crates.io malware postmortem —
  https://blog.rust-lang.org/inside-rust/2023/09/01/crates-io-malware-postmortem/
  https://blog.rust-lang.org/inside-rust/2023/09/01/crates-io-malware-postmortem
- Cargo issues —
  https://github.com/rust-lang/cargo/issues/14414
  https://github.com/rust-lang/cargo/issues/8088
- The Rust Book: ownership, enums, and patterns —
  https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html
  https://doc.rust-lang.org/book/ch06-00-enums.html
  https://doc.rust-lang.org/book/ch19-01-all-the-places-for-patterns.html
- API Guidelines, predictability —
  https://rust-lang.github.io/api-guidelines/predictability.html
- Inherent-impl and explicit-drop discussions —
  https://github.com/rust-lang/rust-clippy/blob/master/clippy_lints/src/inherent_impl.rs
  https://github.com/rust-lang/rust-clippy/issues/6446
  https://canonical.github.io/rust-best-practices/ordering-discipline.html
  https://pingcap.github.io/style-guide/rust/traits.html
  https://users.rust-lang.org/t/a-question-of-style-for-impl-of-structs/118029

## Source index

The following sources are retained as a compact reading index. Their arguments
inform the comparisons above; the linked Jet source and examples define Jet's
actual contracts.

### Systems, safety, and tooling

- https://www.youtube.com/watch?v=H1TSClkkHy0
- https://www.youtube.com/watch?v=BUX01em5eG4
- https://www.youtube.com/watch?v=_2xQog4EBzg
- https://www.youtube.com/watch?v=aoHMiCzqCNw
- https://www.youtube.com/watch?v=WXo6ial-vm4
- https://www.youtube.com/watch?v=3MP8D-mdheA
- https://www.youtube.com/watch?v=hpj6r6CjJf8
- https://www.youtube.com/watch?v=wo84LFzx5nI
- https://www.youtube.com/watch?v=N5bH6ALXX4U
- https://www.youtube.com/watch?v=V_qzqY1bb7I
- https://www.youtube.com/watch?v=9T0LVlVKjGo
- https://www.youtube.com/watch?v=Xz2f-PtQAdk
- https://www.youtube.com/watch?v=GnAwY55hux0
- https://www.youtube.com/watch?v=gDDvH0ZyM7E

### Ownership and compile-time

- https://www.youtube.com/watch?v=8j_FbjiowvE
- https://www.youtube.com/watch?v=KWB-gDVuy_I
- https://www.youtube.com/watch?v=Klq-sNxuP2g
- https://www.youtube.com/watch?v=SMCRQj9Hbx8
- https://www.youtube.com/watch?v=wU8hQvU8aKM
- https://www.youtube.com/watch?v=s5S2Ed5T-dc
- https://www.youtube.com/watch?v=A4cKi7PTJSs
- https://www.youtube.com/watch?v=SqT5YglW3qU
- https://www.youtube.com/watch?v=6c7pZYP_iIE
- https://www.youtube.com/watch?v=ebqKYLKjL6U
- https://www.youtube.com/watch?v=6lXZCOXCRME
- https://www.youtube.com/watch?v=2JgEKEd3tw8
- https://www.youtube.com/watch?v=vfrAX26cqtg
- https://www.youtube.com/watch?v=w50ofEmhRoc
- https://www.youtube.com/watch?v=buxopFR4VXQ
- https://www.youtube.com/watch?v=-DaVwuQeQD0
- https://www.youtube.com/watch?v=E3G2tl0GAb0
- https://www.youtube.com/watch?v=xn7nZLWXYSg
- https://www.youtube.com/watch?v=A6XI_0DWQOw
- https://www.youtube.com/watch?v=7QwqShxyHtc
- https://www.youtube.com/watch?v=l6tisoOzTuk

### Python, JavaScript, Go, and compiled scripting

- https://www.youtube.com/watch?v=3tyaO-OE0K0
- https://www.youtube.com/watch?v=GWZf_B129zs
- https://www.youtube.com/watch?v=FFpDsC6B2qw
- https://www.youtube.com/watch?v=4rmBOxn0PdI
- https://www.youtube.com/watch?v=aPL6y2oJjMw
- https://www.youtube.com/watch?v=sQ1Q96-Vhjk
- https://www.youtube.com/watch?v=DLT6n3wCkuc
- https://www.youtube.com/watch?v=rTgROnXIwnI
- https://www.youtube.com/watch?v=c7eLIsaDL7U
- https://www.youtube.com/watch?v=kHpEolpE3pU
- https://www.youtube.com/watch?v=wpqiH56ITZo
- https://www.youtube.com/watch?v=xUBIbhPC_rQ
- https://www.youtube.com/watch?v=uQV6hYwyjMY
- https://www.youtube.com/watch?v=QNPwKMOQIKM
- https://www.youtube.com/watch?v=L6r0HBz9sT4
- https://www.youtube.com/watch?v=ryfbBB3pHfI
- https://www.youtube.com/watch?v=lzhwEDkn6NY
- https://www.youtube.com/watch?v=2g63UXaynaA
- https://www.youtube.com/watch?v=Kr9ai5zxAcY
- https://www.youtube.com/watch?v=dYGSPyp41vY

### Functions and API design

- https://www.youtube.com/watch?v=2OMRWPOSw9s
- https://www.youtube.com/watch?v=b4p_tcLYDV0
- https://www.youtube.com/watch?v=W2tWOdzgXHA
- https://blog.cleancoder.com/uncle-bob/2014/05/08/SingleReponsibilityPrinciple.html
- https://podscripts.co/podcasts/the-standup-with-theprimeagen/legendary-game-dev-jonathan-blow
- https://youtu.be/OPuztQfM3Fg

### Owner playlist and sync-engine sources (mined 2026-09-28)

Report: [mine-for-jet-2026-09-28](../../research/mine-for-jet-2026-09-28.md).
Logan Smith's honest-function taxonomy (`2OMRWPOSw9s`, listed above) is the
source of the honest versus dishonest framing used by D-HONEST-SIG1.

- https://youtube.com/playlist?list=PLXgiJybdlzvk
- https://www.youtube.com/watch?v=gQwe1Fq_MQA
- https://www.youtube.com/watch?v=YrnAAp_Z16I
- https://www.youtube.com/watch?v=fbLfaFH_R_Q
- https://www.youtube.com/watch?v=5SntrSo8VMw
- https://www.youtube.com/watch?v=v6G_JJK01zU
- https://www.youtube.com/watch?v=HoPAlzaj3Tc
- https://www.youtube.com/watch?v=UUwDyPpQ3n4
- https://www.youtube.com/watch?v=-gPpUIUMrwg
- https://www.youtube.com/watch?v=i-h95QIGchY
- https://www.youtube.com/watch?v=YNtoDGS4uak
- https://www.youtube.com/watch?v=GIt0b-95Fr4
- https://www.youtube.com/watch?v=8-VZoXn8f9U
- https://www.youtube.com/watch?v=0nwZ2jdFrMg
- https://www.youtube.com/watch?v=z7wVUfnm7M0
- https://www.youtube.com/watch?v=ccV9aae8DIc
- https://www.youtube.com/watch?v=2jIQJ2GuCMU
- https://www.youtube.com/watch?v=xP6fIK5g9EQ
- https://www.youtube.com/watch?v=oitYvDe4nps
- https://www.youtube.com/watch?v=e6crOMC9WCE
- https://www.youtube.com/watch?v=29k3eay4Lr4
- https://www.youtube.com/watch?v=R6rH8IGtrTI
- https://www.youtube.com/watch?v=UzD_Ze6zFKA
- https://www.youtube.com/watch?v=YTe-cpDgyKs
- https://www.youtube.com/watch?v=hMvWIYmdxNQ
- https://www.youtube.com/watch?v=iPNEhKMIqNg
- https://www.youtube.com/watch?v=y1SGTYh1_Xs
- https://www.youtube.com/watch?v=X40rcpLfMdY
- https://www.youtube.com/watch?v=ApmD4IQP-Ac
- https://www.youtube.com/watch?v=lBDd26b0Gtw
- https://www.youtube.com/watch?v=jJJm2nQVolY
- https://www.youtube.com/watch?v=jHLbL1Eg4gM
- https://www.youtube.com/watch?v=Cqd4tMX3yxI
- https://www.youtube.com/watch?v=FdshdE-5D1U
- https://www.youtube.com/watch?v=N2ZTVAavNqY
- https://www.youtube.com/watch?v=KBny6MZJR64
- https://www.youtube.com/watch?v=pfWjbtTQwdo
- https://www.youtube.com/watch?v=Zmx0Ou5TNJs
- https://www.youtube.com/watch?v=XpvUiL0B42c
- https://www.youtube.com/watch?v=pRf8_40EDtM
- https://docs.convex.dev/functions/runtimes
- https://docs.deno.com/runtime/fundamentals/security/

### Roc and Skip/Hack talks (mined 2026-09-30)

Report: [mine-for-jet-2026-09-30-roc-skip](../../research/mine-for-jet-2026-09-30-roc-skip.md).

- https://www.youtube.com/watch?v=12yVcgQHAK0
- https://www.youtube.com/watch?v=J31LlAUtoos

### Simon Peyton Jones papers (mined 2026-09-30)

Report: [mine-for-jet-2026-09-30-spj-papers](../../research/mine-for-jet-2026-09-30-spj-papers.md).

- The Verse Calculus (ICFP 2023) — https://simon.peytonjones.org/verse-calculus
- Lower Your Guards (ICFP 2020) — https://simon.peytonjones.org/lower-your-guards
- Diagnosing Type Errors with Class (PLDI 2015) — https://simon.peytonjones.org/diagnosing-type-errors
- SHErrLoc: A Static Holistic Error Locator (TOPLAS 2017) — https://simon.peytonjones.org/sherrloc
- Equality Proofs and Deferred Type Errors (ICFP 2012) — https://simon.peytonjones.org/equality-proofs
- Tackling the Awkward Squad (lecture notes) — https://simon.peytonjones.org/Tackling-the-awkward-squad
- Safe Haskell (Haskell 2012) — https://simon.peytonjones.org/safe-haskell
- Build Systems à la Carte: Theory and Practice (JFP 2020) — https://simon.peytonjones.org/build-systems-a-la-carte-theory-and-practice
- A History of Haskell: Being Lazy with Class (HOPL III) — https://simon.peytonjones.org/history-of-haskell
- Linear Haskell (POPL 2018) — https://simon.peytonjones.org/linear-haskell
- Composable Memory Transactions (PPoPP 2005) — https://www.microsoft.com/en-us/research/publication/composable-memory-transactions
- Beautiful Concurrency (Beautiful Code, 2007) — https://www.microsoft.com/en-us/research/publication/beautiful-concurrency
- Secrets of the Glasgow Haskell Compiler Inliner (JFP 2002; scanned, unread) — https://www.microsoft.com/en-us/research/publication/secrets-of-the-glasgow-haskell-compiler-inliner
- Inlining in GHC: Empirical Investigation and Improvement (Hollenbeck thesis, 2025) — https://simon.peytonjones.org/hollenbeck-inlining
- Join Points in Practice (Haskell Symposium 2025 keynote) — https://simon.peytonjones.org/join-points-hs
- Compiling without Continuations (PLDI 2017) — https://simon.peytonjones.org/compiling-without-continuations
- Type Inference as Constraint Solving (2019 talk) — https://simon.peytonjones.org/type-inference
- Secrets of the GHC Typechecker (2023 talk; slides only) — https://simon.peytonjones.org/secrets-of-typechecker
- OutsideIn(X): Modular Type Inference with Local Assumptions (JFP 2011) — https://simon.peytonjones.org/outsideinx
- Let Should Not Be Generalised (TLDI 2010) — https://simon.peytonjones.org/let-generalised
- Trees That Grow (JUCS 2017) — https://simon.peytonjones.org/trees-that-grow
- Template Meta-programming for Haskell (Haskell Workshop 2002) — https://www.microsoft.com/en-us/research/publication/template-meta-programming-for-haskell
- Kinds Are Calling Conventions (ICFP 2020) — https://simon.peytonjones.org/kinds-are-calling-conventions
- Levity Polymorphism (PLDI 2017) — https://simon.peytonjones.org/levity-polymorphism
- Elastic Sheet-Defined Functions (JFP 2020) — https://simon.peytonjones.org/elastic-sdfs
- Champagne Prototyping (VL/HCC 2004) — https://www.microsoft.com/en-us/research/publication/champagne-prototyping-research-technique-early-evaluation-complex-end-user-programming-systems
- A Monad for Deterministic Parallelism (Haskell 2011) — https://simon.peytonjones.org/deterministic-parallelism

### Systems-language talks and sources (mined 2026-10-01)

Report: [mine-for-jet-2026-10-01-systems-langs](../../research/mine-for-jet-2026-10-01-systems-langs.md).

- https://www.youtube.com/watch?v=5_oqWE9otaE
- https://www.youtube.com/watch?v=i9nFvSpcCzo
- https://www.youtube.com/watch?v=42y2Q9io3Xs
- https://www.youtube.com/watch?v=UBgam9XUHs0
- https://www.youtube.com/watch?v=aKYdj0f1iQI
- https://www.youtube.com/watch?v=UmL_CA-v3O8
- https://www.youtube.com/watch?v=JRcXUuQYR90
- https://www.youtube.com/watch?v=wuGx35UIKTk
- https://harelang.org
- https://github.com/modular/modular
- https://eyg.run
- https://roc-lang.org
- https://skiplang.com
- https://dev.epicgames.com/documentation/en-us/fortnite/verse-language-reference

## Developer experience (e14)

Sources from the nine e14 DX census manifests. IDs are manifest IDs; local paths remain provenance records.

### web-tooling (64 sources)

- VITE-GUIDE — https://vite.dev/guide/
- VITE-FEATURES — https://vite.dev/guide/features.html
- VITE-PLUGIN — https://vite.dev/guide/api-plugin.html
- VITE-CONFIG — https://vite.dev/config/
- BUN-QUICKSTART — https://bun.sh/docs/quickstart
- BUN-RUNTIME — https://bun.sh/docs/runtime
- BUN-WATCH — https://bun.sh/docs/runtime/watch-mode
- BUN-TEST — https://bun.sh/docs/test/
- BUN-BUILD — https://bun.sh/docs/bundler/
- BUN-BUNX — https://bun.sh/docs/pm/bunx
- BUN-INSTALL — https://bun.sh/docs/pm/cli/install
- DENO-INIT — https://docs.deno.com/runtime/reference/cli/init/
- DENO-TASK — https://docs.deno.com/runtime/reference/cli/task/
- DENO-WATCH — https://docs.deno.com/runtime/run/watch_mode/
- DENO-FMT — https://docs.deno.com/runtime/reference/cli/fmt/
- DENO-LINT — https://docs.deno.com/runtime/reference/cli/lint/
- DENO-TEST — https://docs.deno.com/runtime/reference/cli/test/
- DENO-BENCH — https://docs.deno.com/runtime/reference/cli/bench/
- DENO-COVERAGE — https://docs.deno.com/runtime/reference/cli/coverage/
- DENO-DOC — https://docs.deno.com/runtime/reference/cli/doc/
- DENO-SECURITY — https://docs.deno.com/runtime/fundamentals/security/
- TANSTACK-OVERVIEW — https://tanstack.com/devtools/latest/docs/overview
- TANSTACK-QUICKSTART — https://tanstack.com/devtools/latest/docs/quick-start
- TANSTACK-PLUGIN — https://tanstack.com/devtools/latest/docs/plugin-configuration
- TANSTACK-CUSTOM — https://tanstack.com/devtools/latest/docs/building-custom-plugins
- TANSTACK-PRODUCTION — https://tanstack.com/devtools/latest/docs/production
- TANSTACK-VITE — https://tanstack.com/devtools/latest/docs/vite-plugin
- TANSTACK-EVENTS — https://tanstack.com/devtools/latest/docs/event-system
- TANSTACK-CONFIG — https://tanstack.com/devtools/latest/docs/configuration
- TANSTACK-UTILS — https://tanstack.com/devtools/latest/docs/devtools-utils
- TANSTACK-QUERY — https://tanstack.com/query/latest/docs/framework/react/devtools
- TANSTACK-ROUTER — https://tanstack.com/router/latest/docs/devtools
- TANSTACK-FORM — https://tanstack.com/form/latest/docs/framework/react/guides/devtools
- TANSTACK-FORM-SOURCE — https://raw.githubusercontent.com/TanStack/form/main/packages/form-devtools/src/components/DetailsPanel.tsx
- TANSTACK-FORM-ACTIONS — https://raw.githubusercontent.com/TanStack/form/main/packages/form-devtools/src/components/ActionButtons.tsx
- TANSTACK-TABLE — https://tanstack.com/table/latest/docs/devtools
- ASTRO-APP-REF — https://docs.astro.build/en/reference/dev-toolbar-app-reference/
- ASTRO-GUIDE — https://docs.astro.build/en/guides/dev-toolbar/
- ASTRO-OLD-REF — https://docs.astro.build/en/reference/dev-toolbar/
- ASTRO-OLD-GUIDE — https://docs.astro.build/en/guides/dev-toolbar-apps/
- REACT-DEVTOOLS — https://react.dev/learn/react-developer-tools
- REACT-SETUP — https://react.dev/learn/setup
- REACT-PROFILER — https://react.dev/reference/react/Profiler
- REACT-PERF-TRACKS — https://react.dev/reference/dev-tools/react-performance-tracks
- CHROME-OVERVIEW — https://developer.chrome.com/docs/devtools/overview
- CHROME-WORKSPACES — https://developer.chrome.com/docs/devtools/workspaces
- CHROME-PERFORMANCE — https://developer.chrome.com/docs/devtools/performance/overview
- CHROME-RECORDER — https://developer.chrome.com/docs/devtools/recorder
- NEXT-ERROR — https://nextjs.org/docs/pages/building-your-application/configuring/error-handling
- NEXT-INDICATORS — https://nextjs.org/docs/app/api-reference/config/next-config-js/devIndicators
- NEXT-15-2 — https://nextjs.org/blog/next-15-2
- STORYBOOK-INSTALL — https://storybook.js.org/docs/get-started/install
- STORYBOOK-TESTS — https://storybook.js.org/docs/writing-tests
- STORYBOOK-CONTROLS — https://storybook.js.org/docs/essentials/controls
- PLAYWRIGHT-INTRO — https://playwright.dev/docs/intro
- PLAYWRIGHT-UI — https://playwright.dev/docs/test-ui-mode
- PLAYWRIGHT-TRACE — https://playwright.dev/docs/trace-viewer
- PLAYWRIGHT-CODEGEN — https://playwright.dev/docs/codegen
- PLAYWRIGHT-CONFIG — https://playwright.dev/docs/test-configuration
- NX-GRAPH — https://nx.dev/docs/features/explore-graph
- NX-CACHE — https://nx.dev/docs/features/cache-task-results
- NX-CONSOLE — https://nx.dev/docs/getting-started/editor-setup
- TURBO-RUN — https://turborepo.dev/docs/crafting-your-repository/running-tasks
- TURBO-CACHE — https://turborepo.dev/docs/crafting-your-repository/caching

### web-frameworks (48 sources)

- W01 — https://tanstack.com/router/latest/docs/overview
- W02 — https://tanstack.com/router/latest/docs/routing/file-based-routing
- W03 — https://tanstack.com/router/latest/docs/guide/data-loading
- W04 — https://tanstack.com/query/latest/docs/framework/react/overview
- W05 — https://tanstack.com/query/latest/docs/framework/react/guides/mutations
- W06 — https://tanstack.com/query/latest/docs/framework/react/guides/query-invalidation
- W07 — https://tanstack.com/query/latest/docs/framework/react/guides/suspense
- W08 — https://tanstack.com/query/latest/docs/framework/react/guides/network-mode
- W09 — https://tanstack.com/query/latest/docs/framework/react/devtools
- W10 — https://tanstack.com/form/latest/docs/overview
- W11 — https://tanstack.com/form/latest/docs/framework/react/guides/validation
- W12 — https://tanstack.com/table/v8/docs/introduction
- W13 — https://tanstack.com/table/v8/docs/guide/data
- W14 — https://tanstack.com/table/v8/docs/guide/sorting
- W15 — https://tanstack.com/table/v8/docs/guide/pagination
- W16 — https://tanstack.com/virtual/latest/docs/introduction
- W17 — https://tanstack.com/store/latest/docs/overview
- W18 — https://tanstack.com/store/latest/docs/quick-start
- W19 — https://tanstack.com/store/latest/docs/reference/functions/createStore
- W20 — https://tanstack.com/start/latest/docs/framework/react/overview
- W21 — https://tanstack.com/start/latest/docs/framework/react/guide/server-functions
- W22 — https://tanstack.com/start/latest/docs/framework/react/guide/middleware
- W23 — https://nextjs.org/docs/app/getting-started/server-and-client-components
- W24 — https://nextjs.org/docs/app/getting-started/mutating-data
- W25 — https://nextjs.org/docs/app/guides/caching-without-cache-components
- W26 — https://nextjs.org/docs/app/api-reference/file-conventions/loading
- W27 — https://nextjs.org/docs/app/getting-started/caching
- W28 — https://github.com/vercel/next.js/discussions/54075
- W29 — https://github.com/vercel/next.js/issues/58723
- W30 — https://github.com/vercel/next.js/discussions/86538
- W31 — https://github.com/vercel/next.js/discussions/59167
- W32 — https://github.com/vercel/next.js/discussions/73537
- W33 — https://github.com/vercel/next.js/issues/54514
- W34 — https://docs.astro.build/en/concepts/islands/
- W35 — https://docs.astro.build/en/reference/directives-reference/
- W36 — https://docs.astro.build/en/guides/content-collections/
- W37 — https://docs.astro.build/en/guides/view-transitions/
- W38 — https://qwik.dev/docs/concepts/resumable/
- W39 — https://qwik.dev/docs/core/overview/
- W40 — https://svelte.dev/docs/kit/load
- W41 — https://svelte.dev/docs/kit/form-actions
- W42 — https://svelte.dev/docs/svelte/what-are-runes
- W43 — https://docs.solidjs.com/solid-start/v1
- W44 — https://docs.solidjs.com/solid-start/v2
- W45 — https://github.com/solidjs/solid-start
- W46 — https://reactrouter.com/start/framework/data-loading
- W47 — https://reactrouter.com/start/framework/actions
- W48 — https://usefresh.dev/docs/concepts/islands

### live (37 sources)

- elm-debugger — https://guide.elm-lang.org/architecture/debugger.html
- elm-reactor — https://github.com/elm-lang/elm-reactor
- elm-debug-site — https://github.com/elm-lang/debug.elm-lang.org
- elm-architecture-buttons — https://guide.elm-lang.org/architecture/buttons
- redux-devtools-guide — https://redux.js.org/usage/configuring-your-store#redux-devtools
- redux-devtools — https://github.com/reduxjs/redux-devtools
- redux-devtools-api — https://github.com/reduxjs/redux-devtools/blob/main/extension/docs/API/Arguments.md
- redux-devtools-methods — https://github.com/reduxjs/redux-devtools/blob/main/extension/docs/API/Methods.md
- redux-devtools-trace — https://github.com/reduxjs/redux-devtools/blob/main/extension/docs/Features/Trace.md
- replay-docs — https://docs.replay.io/
- replay-time-travel — https://docs.replay.io/getting-started/what-is-replay-io
- replay-live-logs — https://docs.replay.io/reference-guide/debugging/print-statements
- replay-react — https://docs.replay.io/reference-guide/dev-tools/react
- replay-recording — https://docs.replay.io/quickstart
- replay-runtime — https://docs.replay.io/reference/replay-runtimes/replay-chrome
- pharo-by-example — https://books.pharo.org/pharo-by-example/
- pharo-by-example9 — https://raw.githubusercontent.com/SquareBracketAssociates/PharoByExample9/master/Chapters/FirstApplication/FirstApplication.md
- clojure-repl — https://clojure.org/guides/repl/introduction
- cider-interactive — https://docs.cider.mx/cider-nrepl/usage/interactive_programming.html
- cider-interactive-current — https://docs.cider.mx/cider/usage/interactive_programming.html
- portal — https://raw.githubusercontent.com/djblue/portal/master/README.md
- erlang-code-loading — https://www.erlang.org/doc/system_principles/code_loading.html
- erlang-release-handling — https://www.erlang.org/doc/system_principles/release_handling.html
- erlang-observer — https://www.erlang.org/doc/apps/observer/observer_ug.html
- elixir-reloading — https://hexdocs.pm/elixir/code-reloading.html
- elixir-mix-compiler — https://hexdocs.pm/mix/Mix.Task.Compiler.html
- swift-playgrounds — https://developer.apple.com/documentation/swift-playgrounds
- xcode-previews — https://developer.apple.com/documentation/xcode/previewing-your-apps-interface-in-xcode
- revise — https://timholy.github.io/Revise.jl/stable/
- vscode-live-share — https://code.visualstudio.com/learn/collaboration/live-share
- vscode-live-share-current — https://learn.microsoft.com/en-us/visualstudio/liveshare/
- vscode-live-share-features — https://learn.microsoft.com/en-us/visualstudio/liveshare/overview/features
- rr-site — https://rr-project.org/
- rr-repo — https://github.com/rr-debugger/rr
- jupyter — https://docs.jupyter.org/en/latest/
- jupyter-notebook — https://jupyter-notebook.readthedocs.io/en/latest/notebook.html
- marimo — https://docs.marimo.io/

### games (47 sources)

- UE-PIE — https://dev.epicgames.com/documentation/en-us/unreal-engine/playing-and-simulating-in-unreal-engine
- UE-LIVE — https://dev.epicgames.com/documentation/en-us/unreal-engine/using-live-coding-to-recompile-unreal-engine-applications-at-runtime
- UE-BP — https://dev.epicgames.com/documentation/en-us/unreal-engine/blueprints-visual-scripting-in-unreal-engine
- UE-STAT — https://dev.epicgames.com/documentation/en-us/unreal-engine/stat-commands-in-unreal-engine
- UE-INSIGHTS — https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-insights-in-unreal-engine
- UE-GDT — https://dev.epicgames.com/documentation/en-us/unreal-engine/using-the-gameplay-debugger-in-unreal-engine
- UE-CRASH — https://dev.epicgames.com/documentation/en-us/unreal-engine/crash-reporting-in-unreal-engine
- UE-PACKAGE — https://dev.epicgames.com/documentation/en-us/unreal-engine/packaging-your-project
- UE-AUTOREIMPORT — https://dev.epicgames.com/documentation/en-us/unreal-engine/reimporting-assets-automatically-in-unreal-engine
- U-MANUAL — https://docs.unity3d.com/6000.0/Documentation/Manual/UnityManual.html
- U-PLAY — https://docs.unity3d.com/6000.0/Documentation/Manual/configurable-enter-play-mode.html
- U-CODELOAD — https://docs.unity3d.com/6000.0/Documentation/Manual/code-reloading-editor.html
- U-DOMAIN — https://docs.unity3d.com/6000.0/Documentation/Manual/domain-reloading.html
- U-INSPECTOR — https://docs.unity3d.com/6000.0/Documentation/Manual/UsingTheInspector.html
- U-INSPECTOR-OPTIONS — https://docs.unity3d.com/6000.0/Documentation/Manual/InspectorOptions.html
- U-EDITOR-API — https://docs.unity3d.com/6000.0/Documentation/ScriptReference/Editor.html
- U-PROFILER — https://docs.unity3d.com/6000.0/Documentation/Manual/Profiler.html
- U-PROFILER-CUSTOM — https://docs.unity3d.com/6000.0/Documentation/Manual/profiler-create-modules.html
- U-FRAME — https://docs.unity3d.com/6000.0/Documentation/Manual/FrameDebugger.html
- U-FRAME-CONTROLS — https://docs.unity3d.com/6000.0/Documentation/Manual/FrameDebugger-debug.html
- U-PACKAGES — https://docs.unity3d.com/6000.0/Documentation/Manual/Packages.html
- U-IL2CPP — https://docs.unity3d.com/6000.0/Documentation/Manual/il2cpp-introduction.html
- U-CONSOLE — https://docs.unity3d.com/6000.0/Documentation/Manual/Console.html
- U-DEBUGBUILD — https://docs.unity3d.com/6000.0/Documentation/ScriptReference/Debug-isDebugBuild.html
- U-BUILDPROFILES — https://docs.unity3d.com/6000.0/Documentation/Manual/build-profiles-reference.html
- U-ASSETREFRESH — https://docs.unity3d.com/6000.0/Documentation/Manual/AssetDatabaseRefreshing.html
- GODOT-PROJECT — https://docs.godotengine.org/en/stable/tutorials/editor/project_manager.html
- GODOT-SCENE — https://docs.godotengine.org/en/stable/getting_started/step_by_step/nodes_and_scenes.html
- GODOT-EMBED — https://docs.godotengine.org/en/stable/tutorials/editor/game_embedding.html
- GODOT-DEBUG — https://docs.godotengine.org/en/stable/tutorials/scripting/debug/overview_of_debugging_tools.html
- GODOT-DEBUGGER — https://docs.godotengine.org/en/stable/tutorials/scripting/debug/debugger_panel.html
- GODOT-PROFILER — https://docs.godotengine.org/en/stable/tutorials/scripting/debug/the_profiler.html
- GODOT-OUTPUT — https://docs.godotengine.org/en/stable/tutorials/scripting/debug/output_panel.html
- GODOT-INSPECTOR — https://docs.godotengine.org/en/stable/tutorials/editor/inspector_dock.html
- GODOT-IMPORT — https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/import_process.html
- GODOT-EXPORT — https://docs.godotengine.org/en/stable/tutorials/export/exporting_projects.html
- GODOT-CLI — https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html
- BEVY-START — https://bevy.org/learn/quick-start/getting-started/
- BEVY-APPS — https://bevy.org/learn/quick-start/getting-started/apps/
- BEVY-ECS — https://bevy.org/learn/quick-start/getting-started/ecs/
- BEVY-FEATURES — https://github.com/bevyengine/bevy/blob/latest/docs/cargo_features.md
- BEVY-REMOTE — https://docs.rs/bevy/latest/bevy/remote/index.html
- BEVY-INSPECTOR — https://github.com/jakobhellermann/bevy-inspector-egui
- BEVY-DIAGNOSTIC — https://docs.rs/bevy/latest/bevy/diagnostic/index.html
- BEVY-DEVTOOLS — https://docs.rs/bevy/latest/bevy/dev_tools/index.html
- BEVY-EXAMPLES — https://github.com/bevyengine/bevy/blob/latest/Examples/README.md
- CARGO-BUILD — https://doc.rust-lang.org/cargo/commands/cargo-build.html

### backend (43 sources)

- PHX-DASH — https://phoenix-live-dashboard.hexdocs.pm/Phoenix.LiveDashboard.html
- PHX-PAGE — https://phoenix-live-dashboard.hexdocs.pm/Phoenix.LiveDashboard.PageBuilder.html
- PHX-METRICS — https://phoenix-live-dashboard.hexdocs.pm/metrics.html
- PHX-ECTO — https://phoenix-live-dashboard.hexdocs.pm/ecto_stats.html
- PHX-MIGRATE — https://hexdocs.pm/ecto_sql/3.14.0/Mix.Tasks.Ecto.Migrate.html
- PHX-RELOAD — https://phoenix-live-reload.hexdocs.pm/Phoenix.LiveReloader.html
- PHX-HTML — https://phoenix.hexdocs.pm/1.8.11/Mix.Tasks.Phx.Gen.Html.html
- PHX-SCHEMA — https://phoenix.hexdocs.pm/1.8.11/Mix.Tasks.Phx.Gen.Schema.html
- PHX-SERVER — https://phoenix.hexdocs.pm/1.8.11/Mix.Tasks.Phx.Server.html
- LARAVEL-TELESCOPE — https://laravel.com/docs/12.x/telescope
- LARAVEL-HORIZON — https://laravel.com/docs/12.x/horizon
- HORIZON-ROUTES — https://github.com/laravel/horizon/blob/5.x/resources/js/routes.js
- HORIZON-DASH — https://github.com/laravel/horizon/blob/5.x/resources/js/screens/dashboard.vue
- TELESCOPE-REG — https://raw.githubusercontent.com/laravel/telescope/5.x/src/RegistersWatchers.php
- TELESCOPE-WATCHER — https://raw.githubusercontent.com/laravel/telescope/5.x/src/Watchers/Watcher.php
- LARAVEL-PULSE — https://laravel.com/docs/12.x/pulse
- LARAVEL-ARTISAN — https://laravel.com/docs/12.x/artisan
- LARAVEL-SAIL — https://laravel.com/docs/12.x/sail
- LARAVEL-ERRORS — https://laravel.com/docs/12.x/errors
- RAILS-CLI — https://guides.rubyonrails.org/command_line.html
- RAILS-JOBS — https://guides.rubyonrails.org/active_job_basics.html
- RAILS-DEBUG — https://guides.rubyonrails.org/debugging_rails_applications.html
- RAILS-JS — https://guides.rubyonrails.org/working_with_javascript_in_rails.html
- RAILS-WEBCONSOLE — https://github.com/rails/web-console
- SPRING-INITIAL — https://spring.io/quickstart
- SPRING-DEVTOOLS — https://docs.spring.io/spring-boot/reference/using/devtools.html
- SPRING-ACTUATOR — https://docs.spring.io/spring-boot/reference/actuator/endpoints.html
- SPRING-MONITOR — https://docs.spring.io/spring-boot/reference/actuator/monitoring.html
- SPRING-WEB — https://docs.spring.io/spring-boot/reference/web/servlet.html
- SBA-README — https://github.com/codecentric/spring-boot-admin
- SBA-UI — https://docs.spring-boot-admin.com/4.1.2/docs/samples/sample-custom-ui.md
- SBA-ENDPOINTS — https://docs.spring-boot-admin.com/4.1.2/docs/reference/actuator-endpoints.md
- DOTNET-WATCH — https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-watch
- ASPIRE-OVERVIEW — https://aspire.dev/dashboard/overview
- ASPIRE-EXPLORE — https://aspire.dev/dashboard/explore
- ASPIRE-COMMANDS — https://aspire.dev/fundamentals/custom-resource-commands
- DJANGO-ADMIN — https://docs.djangoproject.com/en/5.2/ref/django-admin/
- DJANGO-MIGRATIONS — https://docs.djangoproject.com/en/5.2/topics/migrations/
- DJANGO-ADMIN-SITE — https://docs.djangoproject.com/en/5.2/intro/tutorial02/
- DDT-PANELS — https://django-debug-toolbar.readthedocs.io/en/latest/panels.html
- DDT-INSTALL — https://django-debug-toolbar.readthedocs.io/en/latest/installation.html
- DDT-PANEL-SOURCE — https://raw.githubusercontent.com/django-commons/django-debug-toolbar/main/debug_toolbar/panels/__init__.py
- ERLANG-OBSERVER — https://www.erlang.org/doc/apps/observer/observer_ug.html

### systems (42 sources)

- CARGO-NEW — https://doc.rust-lang.org/cargo/commands/cargo-new.html
- CARGO-RUN — https://doc.rust-lang.org/cargo/commands/cargo-run.html
- CARGO-TEST — https://doc.rust-lang.org/cargo/commands/cargo-test.html
- CARGO-BENCH — https://doc.rust-lang.org/cargo/commands/cargo-bench.html
- CARGO-DOC — https://doc.rust-lang.org/cargo/commands/cargo-doc.html
- CARGO-ADD — https://doc.rust-lang.org/cargo/commands/cargo-add.html
- CARGO-WORKSPACE — https://doc.rust-lang.org/cargo/reference/workspaces.html
- CARGO-FEATURES — https://doc.rust-lang.org/cargo/reference/features.html
- CARGO-PROFILES — https://doc.rust-lang.org/cargo/reference/profiles.html
- CARGO-EXT — https://doc.rust-lang.org/cargo/reference/external-tools.html
- RUSTC-EXPLAIN — https://doc.rust-lang.org/rustc/command-line-arguments.html
- RUSTC-JSON — https://doc.rust-lang.org/rustc/json.html
- RA-FEATURES — https://rust-analyzer.github.io/book/features.html
- RA-ASSISTS — https://rust-analyzer.github.io/book/assists.html
- CARGO-WATCH — https://github.com/watchexec/cargo-watch
- BACON — https://dystroy.org/bacon/
- BACON-COOKBOOK — https://dystroy.org/bacon/cookbook/
- NEXTEST — https://nexte.st/docs/filtersets/
- ZIG-START — https://ziglang.org/learn/getting-started/
- ZIG-BUILD — https://ziglang.org/learn/build-system/
- ZIG-LANGREF — https://ziglang.org/documentation/master/
- GO-CMD — https://go.dev/cmd/go/
- GO-MOD — https://go.dev/ref/mod
- GO-FUZZ — https://go.dev/doc/tutorial/fuzz
- GO-RACE — https://go.dev/doc/articles/race_detector
- GO-WORK — https://go.dev/doc/tutorial/workspaces
- GO-GOPLS — https://go.dev/gopls/
- GO-DELVE — https://github.com/go-delve/delve
- GO-PPROF — https://go.dev/blog/pprof
- CRITERION — https://bheisler.github.io/criterion.rs/book/
- CRITERION-CLI — https://bheisler.github.io/criterion.rs/book/user_guide/command_line_options.html
- CARGO-EXPAND — https://github.com/dtolnay/cargo-expand
- FLAMEGRAPH — https://github.com/flamegraph-rs/flamegraph
- DOCSRS — https://docs.rs/about
- MIETTE — https://github.com/zkat/miette
- ARIADNE — https://github.com/zesterer/ariadne
- JUST — https://just.systems/man/en/
- JUST-PARAMS — https://just.systems/man/en/recipe-parameters.html
- MISE-TASKS — https://mise.jdx.dev/tasks/
- MISE-RUN — https://mise.jdx.dev/tasks/running-tasks.html
- DIRENV — https://direnv.net/
- DEVENV — https://devenv.sh/getting-started/

### mobile (43 sources)

- flutter-install — https://docs.flutter.dev/install
- flutter-cli — https://docs.flutter.dev/reference/flutter-cli
- flutter-hot-reload — https://docs.flutter.dev/tools/hot-reload
- flutter-devtools — https://docs.flutter.dev/tools/devtools
- flutter-inspector — https://docs.flutter.dev/tools/devtools/inspector
- flutter-layout-explorer — https://docs.flutter.dev/tools/devtools/layout-explorer
- flutter-performance — https://docs.flutter.dev/tools/devtools/performance
- flutter-cpu-profiler — https://docs.flutter.dev/tools/devtools/cpu-profiler
- flutter-memory — https://docs.flutter.dev/tools/devtools/memory
- flutter-network — https://docs.flutter.dev/tools/devtools/network
- flutter-extensions — https://docs.flutter.dev/tools/devtools/extensions
- flutter-custom-tool — https://docs.flutter.dev/tools/devtools/custom-tool
- expo-environment — https://docs.expo.dev/get-started/set-up-your-environment
- expo-start — https://docs.expo.dev/get-started/start-developing
- expo-dev-builds — https://docs.expo.dev/develop/development-builds/introduction
- expo-build — https://docs.expo.dev/build/introduction
- expo-update — https://docs.expo.dev/eas-update/introduction
- expo-router — https://docs.expo.dev/router/introduction
- expo-devtools-plugins — https://docs.expo.dev/debugging/devtools-plugins
- expo-create-plugin — https://docs.expo.dev/debugging/create-devtools-plugins
- rn-fast-refresh — https://reactnative.dev/docs/fast-refresh
- rn-hermes — https://reactnative.dev/docs/hermes
- rn-debugging — https://reactnative.dev/docs/debugging
- rn-devtools — https://reactnative.dev/docs/react-native-devtools
- swiftui-previews — https://developer.apple.com/documentation/swiftui/previews-in-xcode
- xcode-preview-interface — https://developer.apple.com/documentation/xcode/adding-previews-to-your-interface-files
- xcode-preview-canvas — https://developer.apple.com/documentation/xcode/interacting-with-previews-in-the-canvas
- xcode-run-device — https://developer.apple.com/documentation/xcode/running-your-app-on-simulated-or-physical-devices
- wwdc-previews — https://developer.apple.com/videos/play/wwdc2023/10252/
- xcode-diagnostics — https://developer.apple.com/documentation/xcode/diagnosing-memory-thread-and-crash-issues-early
- instruments-archive — https://developer.apple.com/library/archive/documentation/AnalysisTools/Conceptual/instruments_help-collection/Chapter/Chapter.html
- xcode-cloud — https://developer.apple.com/xcode-cloud/
- swiftpm-cli — https://www.swift.org/getting-started/cli-swiftpm/
- android-studio — https://developer.android.com/studio/intro
- compose-tooling — https://developer.android.com/develop/ui/compose/tooling
- compose-live-edit — https://developer.android.com/develop/ui/compose/tooling/iterative-development
- compose-previews — https://developer.android.com/develop/ui/compose/tooling/previews
- android-layout-inspector — https://developer.android.com/studio/debug/layout-inspector
- android-profiler — https://developer.android.com/studio/profile
- android-logcat — https://developer.android.com/studio/debug/logcat
- android-gradle-cli — https://developer.android.com/build/building-cmdline
- kmp-first-app — https://kotlinlang.org/docs/multiplatform/multiplatform-create-first-app.html
- kmp-hot-reload — https://kotlinlang.org/docs/multiplatform/compose-hot-reload.html

### data (40 sources)

- jupyter-architecture — https://docs.jupyter.org/en/latest/projects/architecture/content-architecture.html
- jupyter-notebook — https://jupyter-notebook.readthedocs.io/en/stable/notebook.html
- jupyterlab-extensions — https://jupyterlab.readthedocs.io/en/stable/user/extensions.html
- ipywidgets-interact — https://ipywidgets.readthedocs.io/en/stable/examples/Using%20Interact.html
- ipython-magics — https://ipython.readthedocs.io/en/stable/interactive/magics.html
- marimo-reactivity — https://docs.marimo.io/guides/reactivity/
- marimo-apps — https://docs.marimo.io/guides/apps/
- marimo-export — https://docs.marimo.io/guides/exporting/
- marimo-old-notebooks — https://docs.marimo.io/guides/understanding_notebooks/
- observable-framework — https://raw.githubusercontent.com/observablehq/framework/main/docs/what-is-framework.md
- observable-loaders — https://raw.githubusercontent.com/observablehq/framework/main/docs/data-loaders.md
- observable-plot — https://raw.githubusercontent.com/observablehq/plot/main/README.md
- pluto-reactivity — https://plutojl.org/en/docs/reactivity/
- pluto-packages — https://plutojl.org/en/docs/packages/
- livebook — https://livebook.dev/
- kino — https://kino.hexdocs.pm/
- kino-datatable — https://kino.hexdocs.pm/Kino.DataTable.html
- polars-lazy — https://docs.pola.rs/user-guide/lazy/using/
- polars-optimizations — https://docs.pola.rs/user-guide/lazy/optimizations/
- polars-streaming — https://docs.pola.rs/user-guide/concepts/streaming/
- polars-plugins — https://docs.pola.rs/user-guide/plugins/
- duckdb-cli — https://duckdb.org/docs/current/clients/cli/overview.html
- duckdb-dot — https://duckdb.org/docs/current/clients/cli/dot_commands.html
- duckdb-ui — https://duckdb.org/docs/current/core_extensions/ui
- pandas-copy-on-write — https://pandas.pydata.org/docs/user_guide/copy_on_write.html
- streamlit-run — https://docs.streamlit.io/develop/concepts/architecture/run-your-app
- streamlit-architecture — https://docs.streamlit.io/develop/concepts/architecture/architecture
- streamlit-caching — https://docs.streamlit.io/develop/concepts/architecture/caching
- streamlit-session — https://docs.streamlit.io/develop/concepts/architecture/session-state
- gradio-quickstart — https://gradio.app/guides/quickstart
- gradio-blocks — https://www.gradio.app/guides/blocks-and-event-listeners
- quarto-kernel — https://quarto.org/docs/advanced/jupyter/kernel-execution.html
- quarto-widgets — https://quarto.org/docs/interactive/widgets/jupyter.html
- quarto-publishing — https://quarto.org/docs/publishing/
- jupyter-source-registry — https://jupyter.org/
- jet-data-stats — crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs
- jet-data-tests — tests/dataflow_stream.rs
- jet-data-evaluator — crates/jet-codegen/src/Codegen/TIR/eval/data_calls.rs
- jet-notebook — crates/jet-repl/src/Notebook/document.rs
- jet-notebook-trust — crates/jet-repl/src/Notebook/kernel.rs

### cli (31 sources)

- BT — https://github.com/charmbracelet/bubbletea
- LG — https://github.com/charmbracelet/lipgloss
- BB — https://github.com/charmbracelet/bubbles
- HUH — https://github.com/charmbracelet/huh
- GUM — https://github.com/charmbracelet/gum
- VHS — https://github.com/charmbracelet/vhs
- WISH — https://github.com/charmbracelet/wish
- RATATUI-SITE — https://ratatui.rs
- RATATUI-WIDGET — https://ratatui.rs/concepts/widgets/
- RATATUI-LAYOUT — https://ratatui.rs/concepts/layout/
- RATATUI — https://github.com/ratatui/ratatui
- RATATUI-TEMPLATES — https://github.com/ratatui/templates
- INK — https://github.com/vadimdemedes/ink
- TEXTUAL — https://textual.textualize.io
- TEXTUAL-DEV — https://textual.textualize.io/guide/devtools/
- TEXTUAL-TEST — https://textual.textualize.io/guide/testing/
- CLAP — https://docs.rs/clap/latest/clap/
- CLAP-COMPLETE — https://docs.rs/clap_complete/latest/clap_complete/
- CLAP-MAN — https://docs.rs/clap_mangen/latest/clap_mangen/
- TYPER — https://typer.tiangolo.com
- TYPER-CMD — https://typer.tiangolo.com/tutorial/typer-command/
- OCLIF — https://oclif.io/docs/introduction/
- OCLIF-UX — https://oclif.io/docs/user_experience/
- OCLIF-TEST — https://oclif.io/docs/testing/
- COBRA — https://github.com/spf13/cobra
- RICH — https://rich.readthedocs.io/en/stable/
- RICH-CONSOLE — https://rich.readthedocs.io/en/stable/console.html
- RICH-TABLE — https://rich.readthedocs.io/en/stable/tables.html
- RICH-PROGRESS — https://rich.readthedocs.io/en/stable/progress.html
- INDICATIF — https://docs.rs/indicatif/latest/indicatif/
- NO-COLOR — https://no-color.org/
