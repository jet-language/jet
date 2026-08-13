# Prelude scope across languages

Research date: 2026-08-06. Question: what is available with zero imports in each major language, what each community praises or regrets, and what criteria and mitigations follow for Jet's zero-import prelude.

Vocabulary: [Jet vocabulary](../spec/vocabulary.md).

## 1. Comparison table

| Language | Zero-import surface | Approx. size | Notes |
|---|---|---|---|
| Python | `builtins` module: functions, types, exceptions | 71 built-in functions, plus types and ~60 exceptions ([docs](https://docs.python.org/3/library/functions.html)) | `print`, `len`, `open`, `input` all zero-import. `reduce` was demoted out. |
| Rust | `std::prelude` v1 + per-edition additions | ~40 traits/types/functions + macros ([docs](https://doc.rust-lang.org/std/prelude/index.html)) | "Kept as small as possible... focused on things, particularly traits, which are used in almost every single Rust program." |
| Go | Universe block: predeclared identifiers only | ~40: basic types, `true/false/iota/nil`, ~15 builtins (`len`, `append`, `make`, `panic`, ...) ([spec](https://go.dev/ref/spec)) | No zero-import printing for real programs. `print`/`println` exist but are for bootstrapping, not use. |
| Kotlin | Default imports: `kotlin.*`, `kotlin.collections.*`, `kotlin.io.*`, `kotlin.text.*`, `kotlin.math.*`, `kotlin.ranges.*`, `kotlin.sequences.*`, `kotlin.comparisons.*`, `kotlin.annotation.*`, plus platform (`java.lang.*`, `kotlin.jvm.*`) ([docs](https://kotlinlang.org/docs/packages.html)) | Hundreds of names | `println` is in `kotlin.io`, so hello world needs zero imports. |
| Swift | The standard library module is implicitly imported into every file ([Modules.md](https://github.com/swiftlang/swift/blob/main/docs/Modules.md)) | Full stdlib: `String`, `Array`, `Int`, `print`, protocols | Foundation is NOT implicit by design, but Xcode templates and transitive imports made it near-ubiquitous, which the community criticizes ([forums](https://forums.swift.org/t/how-to-disable-implicit-foundation-imports/59678)). |
| Haskell | `Prelude` implicitly imported | ~250 functions, types, classes | Widely criticized; large alternative-prelude ecosystem ([yesodweb](https://www.yesodweb.com/blog/2013/01/so-many-preludes)). |
| Elixir | `Kernel` and `Kernel.SpecialForms` auto-imported ([Kernel docs](https://hexdocs.pm/elixir/Kernel.html)) | ~200 functions/macros/operators/guards | Opt-out per function: `import Kernel, except: [...]`. Special forms cannot be overridden ([SpecialForms](https://hexdocs.pm/elixir/Kernel.SpecialForms.html)). |
| Java | `java.lang.*` implicitly imported in every compilation unit ([JLS §7](https://docs.oracle.com/javase/specs/jls/se7/html/jls-7.html)) | ~100+ public types (`String`, `System`, `Math`, boxed types, core exceptions) | JDK 25 compact source files additionally auto-import all of `java.base` on demand ([JEP 512](https://openjdk.org/jeps/512)). |
| Scala | `Predef`, `scala.*`, `java.lang.*` auto-imported ([Baeldung](https://www.baeldung.com/scala/implicit-imports)) | Predef: dozens of aliases, implicit conversions, `println`, assertions | Scala 2.13 deprecated and Scala 3 dropped the worst Predef implicits (`any2stringadd`) ([migration guide](https://docs.scala-lang.org/scala3/guides/migration/incompat-dropped-features.html)). |

Two clusters. Small-universe languages (Go, Rust) predeclare only what the language itself needs and make everything else an import. Big-prelude languages (Python, Kotlin, Swift, Elixir, Haskell, Scala) put a working vocabulary in scope, including printing and collections.

## 2. Praised vs regretted, by language

### Python

Praised: 71 built-ins cover the whole first year of programming with zero imports — `print`, `input`, `len`, `range`, `open`, `sorted`, `sum`, `min`, `max`, `enumerate`, `zip` ([docs](https://docs.python.org/3/library/functions.html)). Beginner materials lean on exactly these; corpus write-ups of common-function usage put `print`, `len`, `range` at the top ([Medium corpus analysis](https://medium.com/@robertbracco1/most-common-python-functions-aafdc01b71ef)).

Regretted:

- `reduce` was demoted from builtin to `functools`. Guido: "I ended up hating reduce() because it was almost exclusively used (a) to implement sum(), or (b) to write unreadable code. So we added built-in sum() at the same time we demoted reduce()" ([Artima, "The fate of reduce() in Python 3000"](https://www.artima.com/weblogs/viewpost.jsp?thread=98196)). He also proposed cutting `lambda`, `map`, `filter`; they survived, changed.
- Guido's 2002 "Python Regrets" talk lists builtin-level relics removed in Python 3: `apply()`, `coerce()`, `input()` (the old eval-ing form), string exceptions, `int/int` ([slides PDF](https://legacy.python.org/doc/essays/ppt/regrets/PythonRegrets.pdf)).
- Shadowing is the standing cost. Builtins are ordinary names, so `list`, `id`, `type`, `max`, `str` get shadowed by beginners and professionals alike. The ecosystem grew dedicated linters (`flake8-builtins` codes A001–A003) just to police this ([flake8-builtins](https://github.com/gforcada/flake8-builtins); [LSST coding standard DM-831](https://jira.lsstcorp.org/browse/DM-831)).

### Rust

Praised: the prelude is deliberately minimal and trait-focused: "It's kept as small as possible, and is focused on things, particularly traits, which are used in almost every single Rust program" ([std::prelude docs](https://doc.rust-lang.org/std/prelude/index.html)). `Option`/`Result` variants (`Some`, `None`, `Ok`, `Err`) being bare names is broadly liked; `Vec` and `String` are in, `HashMap` is not.

Edition mechanics: the 2021 edition added only three traits — `TryFrom`, `TryInto`, `FromIterator` — and the 2024 edition added `Future`/`IntoFuture` ([edition guide](https://doc.rust-lang.org/edition-guide/rust-2021/prelude.html); [prelude docs](https://doc.rust-lang.org/std/prelude/index.html)).

Actual libs-team criteria, from [RFC 3114](https://rust-lang.github.io/rfcs/3114-prelude-2021.html):

- Adding a trait can break code: `x.try_into()` becomes ambiguous if a local `MyTryInto` is also in scope. So trait additions ride edition boundaries, with the `rust_2021_prelude_collisions` migration lint auto-disambiguating old code ([lint](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_lint/builtin/static.RUST_2021_PRELUDE_COLLISIONS.html)).
- Accepted items fixed a structural incentive problem: `TryFrom`/`TryInto` were "less accessible than the infallible From/Into", a disincentive to fallible conversion.
- Rejected candidates and reasons: `Display`/`Debug` (users reach them via `ToString`, format macros, `dbg!`, so direct scope adds nothing), `Future` in 2021 (`poll` is rarely called directly; async/await covers it — later added in 2024 when `IntoFuture` mattered for `.await`), `Error` (commonly implemented, rarely called directly), `Not` (better served by an inherent method on `bool`).
- The implicit rubric: a name earns a spot when it is called directly and frequently, its absence distorts API choices ecosystem-wide, and the collision risk is manageable at an edition boundary.

### Go

Praised: the universe block is tiny and fixed — types, `true`/`false`/`iota`, `nil`, and ~15 builtin functions ([spec](https://go.dev/ref/spec); [Go 101 blocks/scopes](https://go101.org/article/blocks-and-scopes.html)). Everything else is an explicit import, and an unused import is a compile error ([Go 101 packages](https://go101.org/article/packages-and-imports.html)). This buys total explicitness about provenance: every non-universe name traces to an import line.

Regretted/criticized: hello world needs `package main`, `import "fmt"`, `func main`, and `fmt.Println` — three lines of ceremony around one line of intent, which every beginner tutorial must explain away step by step ([GOSAMPLES hello world](https://gosamples.dev/hello-world/); [Learn Go with tests](https://quii.gitbook.io/learn-go-with-tests/go-fundamentals/hello-world)). Go even has zero-import `print`/`println` builtins, but they are documented as bootstrapping tools not to be relied on — so the beginner-visible path still requires the import ([Go 101 FAQ](https://go101.org/article/unofficial-faq.html)). Universe identifiers are also shadowable (`nil`, `len` can be redeclared), a known confusion source ([Boldly Go](https://boldlygo.tech/archive/2023-06-22-the-universe-and-package-blocks/)).

### Kotlin

Praised: nine default-imported packages give a batteries-included surface — `println`, collections, ranges, text, math — with zero imports ([docs](https://kotlinlang.org/docs/packages.html)). Hello world is one function and one call. The default set is fixed by the compiler; users asking to extend it are told no, which keeps every codebase's ambient vocabulary identical ([discussion](https://discuss.kotlinlang.org/t/how-to-import-additional-packages-by-default-in-kotlin/11414)).

Regret signal is mild: the main complaint is JVM-platform leakage (`java.lang.*` also default-imported), not the Kotlin set itself.

### Swift

Praised: the standard library is implicitly imported into every file, so `print`, `String`, `Array` need nothing ([Modules.md](https://github.com/swiftlang/swift/blob/main/docs/Modules.md)).

Regretted — the Foundation split is the case study:

- Foundation is not implicit, but in practice everything drags it in. Developers on non-Darwin platforms report it as "a frequent source of unexpected bugs and unexpectedly large binary size"; one WASI bundle grew from 3 MB to 26 MB via an implicit Foundation import ([Swift forums](https://forums.swift.org/t/how-to-disable-implicit-foundation-imports/59678)).
- Importing Foundation silently changes semantics of stdlib-looking code: `"abc".contains("")` is `true` without Foundation and `false` with it, because overload resolution picks NSString ([Livsy Code](https://livsycode.com/swift/when-importing-foundation-changes-swifts-behavior/)).
- Apple's response was a full pure-Swift rewrite that breaks Foundation into smaller opt-in modules (FoundationEssentials, FoundationNetworking, FoundationXML) ([InfoQ](https://www.infoq.com/news/2022/12/apple-swift-foundation-rewrite/); [corelibs release notes](https://github.com/apple/swift-corelibs-foundation/blob/release/5.10/Docs/ReleaseNotes_Swift5.md)).

Lesson: a two-tier split where tier two is monolithic and semi-mandatory produces the worst of both worlds — import ceremony and namespace surprise.

### Haskell

The strongest regret signal of any language. The standard Prelude "encourages bad practices such as the partial head function" and promotes `String` (a linked list of Char, "crushingly inefficient") over `Text` ([relude](https://hackage.haskell.org/package/relude); [universum](https://github.com/serokell/universum/blob/2dbf7003143feedabe3fe797831e9a333504b556/README.md)). Partial functions in the default vocabulary mean the types "lie about their behaviour" and cause runtime crashes in pure code ([relude/Kowainik](https://kowainik.github.io/projects/relude)).

The community response — BasicPrelude, ClassyPrelude, safe-prelude, relude, universum, intro, and more ([Yesod: "So many preludes!"](https://www.yesodweb.com/blog/2013/01/so-many-preludes); [Aelve guide](https://guide.aelve.com/haskell/alternative-preludes-zr69k1hc)) — is itself the failure signal: when a language's default vocabulary is wrong, the ecosystem fragments into competing replacements and every project starts with a prelude decision. None achieved consensus. The takeaway for Jet: unsafe or partial defaults in the prelude are near-impossible to retract and get routed around instead of fixed.

### Elixir

Praised: `Kernel` auto-imports ~200 functions/macros (`length`, `hd`, `elem`, `div`, `inspect`, guards, operators) and `Kernel.SpecialForms` supplies the syntax core; both work with zero ceremony ([Kernel](https://hexdocs.pm/elixir/Kernel.html); [SpecialForms](https://hexdocs.pm/elixir/Kernel.SpecialForms.html)). Two mitigations are built in: per-name opt-out (`import Kernel, except: [length: 1]`) so user code can rebind ambient names deliberately, and a hard floor of special forms that can never be overridden. This gives a big prelude with an explicit, local escape hatch — the cleanest large-prelude design in the survey.

### Java

Classic tier: `java.lang.*` is implicitly imported into every compilation unit by spec ([JLS §7.3](https://docs.oracle.com/javase/specs/jls/se7/html/jls-7.html)), but that tier never included printing without ceremony — `System.out.println` inside `public static void main(String[] args)` is the canonical teaching complaint; instructors report telling students to "just ignore this part for now," and the JEP itself calls the concepts premature for a first lesson ([JEP 512](https://openjdk.org/jeps/512); [BigGo summary](https://biggo.com/news/202509161323_Java_Eliminates_public_static_void_main_Boilerplate)).

The correction, 30 years in: JDK 25 compact source files drop the class and `static` ceremony, and implicitly import (on demand) every public type exported by `java.base`, plus a new `IO.println` for console output ([JEP 512](https://openjdk.org/jeps/512); [Oracle docs](https://docs.oracle.com/en/java/javase/25/language/compact-source-files-and-instance-main-methods.html)). Two design details matter for Jet: the enlarged implicit import applies only to the beginner file form ("compact" units), and the expanded form "merges gracefully onto the highway" — the same names work when the student later writes full modules ([JEP 512](https://openjdk.org/jeps/512)). Java effectively adopted a mode-gated prelude: big for the first hour, conventional for production files. `IO`'s static methods were deliberately kept qualified (`IO.println`) rather than bare, showing restraint even inside the beginner mode ([JEP 512](https://openjdk.org/jeps/512)).

### Scala

Scala auto-imports `java.lang.*`, `scala.*`, and `Predef` ([Baeldung](https://www.baeldung.com/scala/implicit-imports)). Predef packed in implicit conversions along with utilities, and the conversions became the regret:

- `any2stringadd` let `anyObject + "s"` compile by implicitly converting anything to String — called "a dreadfully type-unsafe nuisance" in the bug tracker; deprecated in 2.13 and dropped in Scala 3 ([scala/bug#7327](https://github.com/scala/bug/issues/7327); [scala/scala#6315](https://github.com/scala/scala/pull/6315); [Scala 3 migration guide](https://docs.scala-lang.org/scala3/guides/migration/incompat-dropped-features.html)).
- Scala 3's broader cleanup dropped further ambient magic (auto-application, `DelayedInit`) "to make the language simpler and safer," with migration tooling handling most rewrites ([dropped features](https://docs.scala-lang.org/scala3/reference/dropped-features/auto-apply.html); [migration guide](https://docs.scala-lang.org/scala3/guides/migration/incompat-dropped-features.html)).

Lesson: utilities in a prelude age fine; implicit conversions in a prelude become language-wide semantics that take a major version to remove.

## 3. Per-segment view: who feels what

Owner directive: weigh every experience level, not just expert opinion.

### First lesson / intro curricula

- Purpose-built teaching languages have no import concept at all in the first hours. DrRacket's Beginning Student language ships a predefined function vocabulary in a deliberately restricted syntax ([Racket docs](https://docs.racket-lang.org/drracket/htdp-langs.html)). Hedy starts at five ambient commands (`print`, `ask`, `echo`, `forward`, `turn`) and grows the language level by level ([Hedy design paper, Software Impacts](https://www.sciencedirect.com/science/article/pii/S2590118422000557)). The pedagogy consensus: lesson one teaches sequencing and output, and any name-resolution ceremony before that is pure overhead.
- Empirical support that surface friction matters for novices: Stefik & Siebert's four-study investigation found novices using C-style languages (Java, Perl) were no more accurate than with a language using randomly generated keywords, while Python, Ruby, and Quorum did significantly better — "syntax remains a significant barrier" ([ACM TOCE 2013](https://dl.acm.org/doi/10.1145/2534973); [summary](https://neverworkintheory.org/2014/01/29/stefik-siebert-syntax.html)).
- The concrete hello-world gradient: Python `print("hi")` (zero ceremony, zero imports); Kotlin `fun main() { println("hi") }` (zero imports); Go requires package + import + func main ([GOSAMPLES](https://gosamples.dev/hello-world/)); pre-25 Java required class + `public static void main` + `System.out.println`, which instructors uniformly flagged as harmful hand-waving and which Oracle's own JEP now removes for exactly that reason ([JEP 512](https://openjdk.org/jeps/512)).

### Hobbyists and one-off scripters

This segment writes short files where import lines are a large fraction of the program. Python wins this segment because the 71 builtins plus `open`/`input`/`print` cover most one-screen scripts with zero imports ([docs](https://docs.python.org/3/library/functions.html)). Java's compact source files were justified explicitly for "scripts and command-line utilities," auto-importing `java.base` so small programs need no import block at all ([JEP 512](https://openjdk.org/jeps/512)). Go's unused-import compile error is at its most annoying here: iterating on a script means repeatedly adding and deleting import lines (tooling like goimports exists precisely to automate this) ([Go 101 packages](https://go101.org/article/packages-and-imports.html)).

### Professional / enterprise teams

Large codebases feel the opposite costs:

- Namespace pollution and shadowing become review and lint burden: Python organizations codify "do not shadow builtins" rules and run `flake8-builtins` in CI ([LSST DM-831](https://jira.lsstcorp.org/browse/DM-831); [kolibri #5700](https://github.com/learningequality/kolibri/issues/5700)).
- Ambient names that change resolution are worse than shadowing: Swift's Foundation behavior shifts ([Livsy Code](https://livsycode.com/swift/when-importing-foundation-changes-swifts-behavior/)) and Scala's `any2stringadd` ([#7327](https://github.com/scala/bug/issues/7327)) both created bugs a reader could not see in the file.
- Explicit provenance is why enterprises praise Go: every name is greppable to an import. The cost is pure ceremony, and it is paid once per file, mostly by tooling.
- Evolution risk lands here too: adding a prelude name can break existing large codebases (Rust's `try_into` ambiguity), which is why Rust gates additions on editions with migration lints ([RFC 3114](https://rust-lang.github.io/rfcs/3114-prelude-2021.html)).

Net: beginners and scripters pay per-use for a small prelude; enterprises pay rarely-but-expensively for a big one. The mitigations in section 5 exist precisely because the second cost is manageable with mechanism, while the first cost is unfixable except by enlarging the prelude.

## 4. Derived criteria for prelude membership

Synthesized from the evidence above:

1. **Frequency, measured, and direct.** The name is called directly in a large share of real programs (Rust: "used in almost every single Rust program" ([prelude docs](https://doc.rust-lang.org/std/prelude/index.html)); RFC 3114 rejected `Error`/`Debug` because implementation-frequency and macro-mediated use don't count ([RFC 3114](https://rust-lang.github.io/rfcs/3114-prelude-2021.html))). Guido's `sum`-for-`reduce` swap shows the test is what people do with a name, not category membership ([Artima](https://www.artima.com/weblogs/viewpost.jsp?thread=98196)).
2. **Safe as a default.** Never put a partial, panicking, or type-unsafe operation in the ambient vocabulary. Haskell's `head` and `String` are the cautionary tale — the fix became a fragmented alternative-prelude ecosystem, not a repair ([relude](https://hackage.haskell.org/package/relude); [yesodweb](https://www.yesodweb.com/blog/2013/01/so-many-preludes)). Scala's `any2stringadd` shows the same for type safety ([#7327](https://github.com/scala/bug/issues/7327)).
3. **Names only, never semantics.** A prelude entry must add a name, not change how existing code resolves or converts. Implicit conversions (Scala) and cross-module overload capture (Swift/Foundation) are the two documented ways preludes caused invisible behavior change ([migration guide](https://docs.scala-lang.org/scala3/guides/migration/incompat-dropped-features.html); [Livsy Code](https://livsycode.com/swift/when-importing-foundation-changes-swifts-behavior/)).
4. **No better home.** If the natural spelling routes through something else, keep it out (RFC 3114: `Display` via `ToString`, `Not` via an inherent method) ([RFC 3114](https://rust-lang.github.io/rfcs/3114-prelude-2021.html)). Conversely, absence must not distort the ecosystem: `TryFrom` got in because leaving it out pushed people to infallible conversions.
5. **First-hour coverage.** The prelude must cover lesson one through the first real script without an import: output, input, length, ranges, basic math, basic collections. Python and Kotlin meet this; Go and pre-25 Java fail it, and Java's own JEP documents the pedagogical cost ([JEP 512](https://openjdk.org/jeps/512); [Stefik & Siebert](https://dl.acm.org/doi/10.1145/2534973)).
6. **Uniform everywhere.** One fixed set, not user-extensible, so every file in every codebase reads the same (Kotlin refuses user additions ([discussion](https://discuss.kotlinlang.org/t/how-to-import-additional-packages-by-default-in-kotlin/11414)); Haskell's per-project preludes are the counterexample).
7. **Collision-conscious naming.** Prefer prelude names unlikely to be natural variable names. Python's `list`, `id`, `type`, `max` collide with the words programmers reach for first, generating a permanent lint industry ([flake8-builtins](https://github.com/gforcada/flake8-builtins)).

## 5. Mitigations for prelude evolution and pollution

- **Edition/epoch-gated additions.** Rust adds prelude names only at edition boundaries, with an automatic migration lint (`rust_2021_prelude_collisions`) that rewrites ambiguous call sites in old code ([edition guide](https://doc.rust-lang.org/edition-guide/rust-2021/prelude.html); [lint](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_lint/builtin/static.RUST_2021_PRELUDE_COLLISIONS.html)). This is the only proven way to grow a prelude without breaking the world.
- **Mode-gated prelude size.** Java's compact source files give beginners an enlarged implicit import set that full modules don't get, with identical name meaning in both, so graduating is additive ([JEP 512](https://openjdk.org/jeps/512)). Maps directly onto Jet's dual-facet design: the beginner facet can see a wider ambient surface as long as every name means the same thing in expert code.
- **Local shadowing rules with a hard floor.** Elixir lets a file exclude specific Kernel imports to rebind them, while special forms are un-overridable ([Kernel](https://hexdocs.pm/elixir/Kernel.html); [SpecialForms](https://hexdocs.pm/elixir/Kernel.SpecialForms.html)). Python's silently-shadowable builtins are the anti-pattern; a prelude either warns on shadowing by default or requires an explicit opt-out.
- **User names win, loudly.** Precedence should favor user definitions over prelude names (so additions can't break code), paired with a compiler warning when shadowing happens (so it can't hide bugs). Go gives the precedence without the warning ([Boldly Go](https://boldlygo.tech/archive/2023-06-22-the-universe-and-package-blocks/)); Python gives neither and outsources the warning to linters ([flake8-builtins](https://github.com/gforcada/flake8-builtins)).
- **Demote, don't delete silently — and demote early.** Python moved `reduce` to `functools` at a major version with years of stated rationale ([Artima](https://www.artima.com/weblogs/viewpost.jsp?thread=98196)); Scala deprecated `any2stringadd` a full major version before removal, with migration tooling ([scala/scala#6315](https://github.com/scala/scala/pull/6315)). Both worked. Haskell never demoted `head`, and got forks instead.
- **Keep tier two modular.** If anything sits between "prelude" and "third-party," split it small and opt-in. Swift's monolithic Foundation caused binary bloat and semantic surprise, and Apple's remedy was modularization ([forums](https://forums.swift.org/t/how-to-disable-implicit-foundation-imports/59678); [InfoQ](https://www.infoq.com/news/2022/12/apple-swift-foundation-rewrite/)).

## Implications for Jet (summary)

Jet's zero-import prelude should look like Python's coverage with Rust's discipline: full first-hour vocabulary (print, input, len, ranges, math, core collections) with every entry total/safe, no ambient conversions or resolution magic, a fixed uniform set, user-wins-with-warning shadowing, and additions gated on Jet's epoch mechanism with automatic migration. The beginner facet may widen the ambient surface Java-25-style provided names mean the same thing in expert files. Nothing partial, nothing panicking, nothing implicit-converting ever enters — Haskell shows that mistake is permanent.
