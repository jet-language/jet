# What five years of language release notes teach Jet (2021–2026)

Card #6376. Author: MemDesign-5, taking over from MemDesign-4, on 2026-10-30.
Jet state: the published `jet` at dev 03c64ad0c414, Tower read on 2026-10-30.

## Plain answer

Over 2021–2026, the changes that worked had three things in common. They were safe by default, their cost was measured, and moving old code to them was mechanical.
Go's per-iteration loop variables, C++26's hardened library (0.3% cost at Google), Rust 2024 with `cargo fix`, and Kotlin's K2 compiler, tested on 10 million lines before release, are examples.
The changes that failed had different marks. Some forced annotations onto code that was already correct (Swift 6's first concurrency checks). Some blocked the runtime in ways users could not see (Java virtual threads stuck on `synchronized`). Some promised safety that a second path could skip (string interpolation into SQL). Others were dropped because their cost never came down (Dart macros, V autofree, Python 3.14's incremental GC).

Jet already matches most of the lessons. Probes found five real gaps, now cards N1–N5 below.

- Checked SQL and HTML can be built from any runtime string through their public fields. This makes SQL injection and script injection possible.
- A task blocked in an ordinary OS wait holds its worker thread, so a ready task waited 3 seconds.
- Each worker thread reserves 192 MiB of stack, so 32 trivial tasks crash under a 4 GB address-space limit.
- `jet fix` applies a deprecation rename that breaks the call it edits.
- Reassigning a loop variable gets an error that wrongly says it was made with `::`.

There is one owner choice: when a deprecated function's replacement takes different arguments, how should `jet fix` move callers? The recommendation inlines a one-line wrapper as Go 1.26 does, uses an author's pattern otherwise, and else prints the exact edit. See ballot D-DEPRECATE-INLINE1 (card #6405).

## Method and sources

- I read the official release notes, JEPs, PEPs and team blog posts for 25 languages. Canonical addresses are given per row.
- Jet facts come from Tower decisions and cards, the dev worktree `.agent-worktrees/Main-dev-collectall` at 59213de00f, and probes.
- Every probe is in the card's scratch probe set (rel/, not kept in the repository) and ran on the published `jet` 03c64ad0c4, under `nice -n 19 taskset -c 4-15,20-31`.
  The shell's soft address-space limit is 4,096,000,000 bytes (`ulimit -v`), with the hard limit unlimited. Two runs raised the soft limit to 16 GB to isolate the cause; they are marked.
- A claim I did not observe directly is marked [INFERENCE].

### Probe results

| Probe | What it asks | Result on 03c64ad0c4 |
|---|---|---|
| r01b_string_plus | `a + b` on two Strings | E0109 "Arithmetic needs matching number types"; Fix says "Make both operands the same number type" though both are String. Already card #6374. |
| r02_sql_plain_literal | plain interpolated string where SQL is expected | E0149 "A runtime `String` can't be used as `SQL`". Good (D-UNIFYLIT1=A, typed head only). |
| r01e_sql_runtime_ok | `SQL{text: <runtime String>, values: []}` | Compiles and runs: `1`, `1`. |
| r01f_sql_inject_db | same, against SQLite, input `x' OR '1'='1` | Prints `rows:2` (both rows). Injection works. |
| r10_html_fields | `HTML{markup: user}` | Prints `<script>alert(1)</script>` unescaped; the checked literal beside it escapes. |
| r03b_defer_return | `return` inside `defer { }` | E0003 "Expected a value, found `return`", plus a follow-on E0003 at the closing `}`. |
| r03c_defer_block | `defer { print(..) }` | E0003 "`defer` only schedules a consuming resource close": D-JAI-DEFER1=A is not built yet (#5026). |
| r04_loop_capture | closure made in a loop | Prints `1`: each iteration has its own variable (Go 1.22's fix). Good. |
| r05_module_write | function writes a module `:=` | Prints `2`, no diagnostic. This is open ballot D-MODULE-WRITE1 (#6332). |
| r06_loop_var_assign | `i = i + 10` on a loop variable | E0111 "`i` was made with `::`, so it can't change", Fix "Declare it with `i := ...`". The rule matches Lua 5.5; the text is wrong. |
| r08_child_wait_block | 96 tasks each wait for `sleep 1`, then one quick task | With the 16 GB limit: "quick task done after 3023780905ns". With the default 4 GB limit: `panic: could not start scheduler worker`. |
| r09_many_quick_tasks | 32 tasks that never block | 4 GB limit: `panic: could not start scheduler worker` (E3001, exit 70). 16 GB limit: `done`. |
| r11_deprecated + `jet fix` | deprecated `double(a)` whose `use:` replacement is `scaled(a, by)` | Warning promises "run `jet fix` to migrate automatically". `jet fix` "applied 2 fixes", and the fixed file fails with E0766 (`scaled(4)` is missing `by`). |

## Lessons across languages

1. **Safe defaults win when the cost is measured.**
   - Go 1.22 made each loop iteration get its own variable. Google reported no production problems; only buggy tests broke, and the change was gated per module (go.dev/blog/loopvar-preview).
   - Google's hardened libc++ cost 0.30% on average, found over 1,000 bugs and cut baseline segfaults by 30% (security.googleblog.com, 2024-11). C++26 standardizes that hardening.
   - Jet already has per-iteration capture (r04), bounds checks and zero telemetry (D-TELEMETRY1=A).
2. **Breaking changes survive only when the fix is mechanical.**
   - Rust 2024 shipped with `cargo fix --edition`.
   - Go 1.26 rebuilt `go fix` around modernizers and a source-level inliner (`//go:fix inline`).
   - Mojo 1.0 shipped deprecated aliases and fix-its.
   - Zig 0.15's "Writergate" broke every I/O caller with no automatic fix, and users had to migrate by hand.
   - Jet has the rule (#6068: every break ships a `jet fix` rewrite), but r11 shows `jet fix` breaking code. That is card N4 and ballot D-DEPRECATE-INLINE1.
3. **Withdrawn features were withdrawn for cost or for leaks, not for lack of interest.**
   - Java string templates went to a third preview and were then withdrawn (JEP 465, Closed/Withdrawn).
   - Dart cancelled macros over compile-time and hot-reload cost.
   - Go stopped all error-syntax proposals in 2025 after three rounds.
   - V still tells users to avoid autofree.
   - Python 3.14.5 reverted the incremental GC after "significant memory pressure in production".
   - Zig 0.15 removed `async`/`await` and `usingnamespace`.
   - Jet already avoided most of these: metaprograms cannot rewrite user code (D-METAMUTATE1=A), it has no glob imports, and it uses M:N tasks instead of `async` (D-ASYNCRT1=A).
4. **Concurrency checks must not punish correct single-threaded code, and the runtime must not silently block.**
   - Swift 6 reported false positives on globals. Swift 6.2 made the main actor the default.
   - Java virtual threads pinned their carrier thread inside `synchronized` until JDK 24 (JEP 491), so libraries rewrote their locks.
   - Jet has the same blocking hole for OS waits (r08, card N2). Its module-write question is still open (#6332).
5. **Faster tools are a feature.**
   - TypeScript 7's Go port is about 10x faster.
   - Zig's own x86 backend made debug builds 5x faster.
   - Kotlin K2 was validated on 10 million lines before shipping.
   - Jet's self-hosted pipeline already targets this. No new card.
6. **Small "you dropped this" checks are cheap wins.**
   - Kotlin 2.3 added an unused-return-value checker.
   - PHP 8.5 added `#[\NoDiscard]` with a `(void)` escape.
   - C23 added `[[nodiscard]]`.
   - Jet's D-DISCARD1=A already rejects lost results.
7. **A safety type is only as strong as its only door.**
   - Java's JEP 465 refused plain interpolation because "it would be tempting to construct SQL queries using interpolation".
   - Python 3.14's t-strings return a `Template`, not a `str`, so libraries can refuse plain strings.
   - Jet's checked literal is stronger, because it is checked when compiling. But r01f and r10 show a second, unchecked door through public fields (card N1).

## Per-language catalogue

Each row gives the version, the change, how it went in practice (with its source), what Jet does today, and what Jet should do and why.

### C (C23, C2y work)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| C23 | Removed K&R definitions and non-two's-complement integers; `realloc(p, 0)` became undefined | Old code needed edits; compilers kept the old modes behind flags (en.cppreference.com/w/c/23) | Jet's C importer reads headers with pinned Clang (D-CIMPORT1=A) | Keep. Clang gives C23 parsing for free. |
| C23 | `nullptr`, `typeof`, `constexpr`, `[[nodiscard]]`, `_BitInt(N)`, `#embed` | Adopted in GCC 13+ and Clang 16+ (cppreference compiler table) | The header importer is Clang, so it parses these; whether `_BitInt` maps to a Jet type is unverified | No card yet. My header-import probe could not resolve names in this environment (E0102 for `use "blocker.h"`), so I could not test `_BitInt`. |
| C2y defer TS 25755 | Block-scoped `defer`; `return`, `break` and `goto` out of a defer block are banned; `longjmp`/`exit` unspecified | GCC patch and slimcc implement it (thephd.dev, 2025-03-15) | D-JAI-DEFER1=A ratified the same ban with registered codes; not built (r03c) | Criterion on #5026: the ban must give its registered code, not today's E0003 "Expected a value" (r03b). |

### C++ (C++23, C++26)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| C++23 | `std::expected`, `std::print`, `import std` | `import std` depends on modules, which compilers support only partly (cppreference compiler support) | Jet has error unions and `print` built in | Nothing to do. |
| C++26 | Standard library hardening (P3471) | Google: 0.30% average cost, over 1,000 bugs found, 30% fewer segfaults (security.googleblog.com 2024-11) | Jet collections are bounds-checked by default | Keep checks on in release builds. This is the strongest measured evidence for safe defaults. |
| C++26 | Erroneous behaviour for uninitialized reads (P2795), `[[indeterminate]]` opt-out | Turns a whole class of undefined behaviour into defined-but-wrong values | Jet has no uninitialized locals | Nothing to do. Useful evidence for open ballot D-UB-BOUNDARY1. |
| C++26 | Static reflection (P2996) and contracts (P2900) | Herb Sutter: "more transformational than any 10 other major features" (herbsutter.com, 2025-06) | Jet has `$` facts and `prep { }` compile-time code | Nothing to do. Jet's reflection already reads facts without letting metaprograms mutate code (D-METAMUTATE1=A). |

### Rust (1.56–1.9x, editions 2021 and 2024)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 1.85 | Edition 2024, with `cargo fix --edition` | Migration was mostly mechanical; the fixer is conservative and leaves a few hand edits (blog.rust-lang.org, 2025-02-20) | One edition, "2026"; E2001 rejects others | Keep one edition until 1.0. When a second exists, it needs a full `jet fix` (#6068). |
| 1.75 | `async fn` in traits | Shipped with a `Send`-bound caveat and the `trait_variant` helper crate (blog.rust-lang.org, 2023-12-21) | No `async`; tasks are M:N (D-ASYNCRT1=A) | Nothing to do. Jet avoids function colouring entirely. |
| 1.88 | `let` chains (edition 2024 only) | Took years because of drop-order questions | Jet's `if ... == .Val(x)` matching covers this | Nothing to do. |

### Go (1.18–1.26)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 1.21 | GODEBUG settings and toolchain auto-download | Behaviour changes gated per module; old behaviour selectable | D-JPK-TOOLCHAIN1 pins toolchains | Nothing to do. |
| 1.22 | Per-iteration loop variables | No production problems at Google; buggy tests exposed (go.dev/blog/loopvar-preview) | Per-iteration capture (r04 prints `1`) | Nothing to do. |
| 1.23 | Range-over-func iterators; opt-in telemetry | Iterators adopted widely; telemetry kept opt-in after pushback | Zero telemetry (D-TELEMETRY1=A) | Keep. |
| 1.25 | Container-aware GOMAXPROCS | Fixes CPU throttling in Kubernetes | The scheduler reads cgroup quotas and affinity (`HostParallelism.jet`) | Nothing to do. |
| 1.26 | `go fix` rebuilt: modernizers and `//go:fix inline` | Library authors migrate their own users (go.dev/doc/go1.26) | `#Deprecated(use:)` handles renames only, and misapplies them (r11) | Card N4, and ballot D-DEPRECATE-INLINE1. |
| 1.26 | Green Tea GC default; goroutine leak profile | 10–40% less GC overhead | Jet's memory model differs | Nothing to do. |
| 2025 blog | Stopped pursuing error-handling syntax after check/handle, `try` (~900 comments) and `?` (go.dev/blog/error-syntax) | Community could not agree | Jet has error unions and `??` | Nothing to do. Evidence against more error syntax. |
| all | Syscalls and cgo calls release their P, so other goroutines keep running | Basis of Go's "write blocking code" model | Jet compensates only its own runtime waits (r08: a ready task waited 3 s) | Card N2. D-ASYNCRT1=A promises Go's model. |

### Zig (0.11–0.16)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 0.15 | `usingnamespace` removed | Hid where names came from; broke Autodoc (ziglang.org/download/0.15.1/release-notes.html) | No glob imports | Keep. |
| 0.15 | `async`/`await` keywords removed; moving into the `Io` interface | Async had been broken since the self-hosted compiler | No `async`; M:N tasks | Nothing to do. |
| 0.15 | "Writergate": new non-generic `std.Io.Writer`/`Reader` | "tidal waves of breaking" changes, no automatic fix | `jet fix` rule (#6068) | Evidence for #6068 and D-DEPRECATE-INLINE1. |
| 0.15 | x86 backend default for debug builds | Debug compile 5x faster | Native backend | Nothing to do. |
| 0.16 | All I/O takes an `Io` value; lazy field analysis | A namespace use pulled in vtables and bloated binaries until fields became lazy | Effects in signatures (`-[FS]>`), no passed value | Nothing to do. Jet gets the same visibility without a parameter. |

### Swift (5.5–6.2)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 5.5 | async/await, actors, structured concurrency | `async` must be marked at every caller, so it spreads through APIs | M:N tasks, no colouring | Nothing to do. |
| 5.9 | Macros (SwiftSyntax), parameter packs, noncopyable types | Macros are separate programs built on swift-syntax; build-time cost complaints followed [INFERENCE: not in the release notes] | Checked macros (D-JAI-CODE1=A), in-compiler | Nothing to do. |
| 6.0 | Strict concurrency checks on by default | False positives on globals; reflexive `@MainActor` everywhere (swift.org/blog/announcing-swift-6) | Module `:=` writes compile silently (r05); open ballot D-MODULE-WRITE1 | Evidence for #6332: whatever wins must not make single-task programs add annotations. |
| 6.2 | "Approachable concurrency": main actor by default, `@concurrent`; `InlineArray`, `Span`, strict memory safety | Reverses 6.0's friction with defaults (swift.org/blog/swift-6.2-released) | — | Same evidence for #6332. |

### Kotlin (1.5–2.3)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 2.0 | K2 compiler | Validated on 10M lines and 80k projects before release (kotlinlang.org/docs/whatsnew20.html) | Self-hosted compiler with a corpus guard | Evidence for #6068's corpus guard. |
| 2.2 | Context parameters replace context receivers | Receivers were withdrawn after experience | Jai-style `context` (D-JAI-CONTEXT1) | Nothing to do. |
| 2.3 | Unused return value checker | Catches dropped results | D-DISCARD1=A | Nothing to do. |

### Java (17–25)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 21 (JEP 444) | Virtual threads | Pinned inside `synchronized` and native calls; libraries rewrote to j.u.c locks, and pinning could starve or deadlock | Same hole for OS waits (r08) | Card N2. |
| 24 (JEP 491) | Synchronize virtual threads without pinning | Removed nearly all pinning (openjdk.org/jeps/491) | — | Same card. |
| 21–23 (JEP 430/459/465) | String templates | Withdrawn after a third preview; the JEP refused plain interpolation for safety | Checked SQL/HTML literal, but a field bypass (r01f, r10) | Card N1. |
| 25 (JEP 512) | Compact source files, instance `main` | Lowers the cost of the first program | `fn run()` scripts | Nothing to do. |
| JEP 401 | Value classes (preview) | Still preview after years | Value structs by default | Nothing to do. |

### C# (10–14)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 12 | Collection expressions, primary constructors; interceptors experimental | Interceptors stayed experimental (learn.microsoft.com/dotnet/csharp/whats-new/csharp-12) | Metaprograms cannot rewrite calls (D-METAMUTATE1=A) | Keep. |
| 13 → 14 | `field` keyword: preview in 13 because existing identifiers named `field` would break; shipped in 14 | A breaking keyword waited a release for a fix-it | New keywords go through `jet fix` (#6068) | Evidence for #6068. |
| 14 | Extension members (properties, statics) | Long-requested | Methods are ordinary functions (D-METHODMACRO1=A) | Nothing to do. |

### Python (3.10–3.14)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 3.11 | 1.25x faster; PEP 657 error carets | Error locations widely praised | Located diagnostics | Nothing to do. |
| 3.12 | PEP 695 type parameter syntax; PEP 701 f-strings in the grammar | Tokenizing 64% faster | — | Nothing to do. |
| 3.13 | Free-threaded build; experimental JIT | Both opt-in | — | Nothing to do. |
| 3.14 | PEP 750 t-strings return a `Template`, not `str` | Lets SQL/HTML libraries refuse plain strings (docs.python.org/3.14/whatsnew) | Checked literal plus field bypass | Card N1. |
| 3.14 | PEP 765 warns on `return`/`break`/`continue` leaving `finally` | Same class as C's defer ban | D-JAI-DEFER1=A bans it | Criterion on #5026. |
| 3.14 | PEP 649 deferred annotations; PEP 768 safe debugger attach; JIT "10% slower to 20% faster" | — | — | Nothing to do. |
| 3.14.5 | Incremental GC reverted to generational | "significant memory pressure in production" | — | Evidence: ship GC changes behind a measured gate. |

### JavaScript and TypeScript

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| TS 5.8 | `--erasableSyntaxOnly` | Follows Node type stripping (unflagged in 23.6; no flag from 22.18), which rejects enums, runtime namespaces and parameter properties | — | Nothing to do. |
| TS 7 | Native port to Go | About 10x faster, lower memory (devblogs.microsoft.com/typescript, 2025-03) | — | Evidence: tool speed is a feature. |

### Ruby (3.0–4.0)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 3.3 | Prism parser, YJIT gains, M:N threads (opt-in) | M:N stayed opt-in | M:N tasks | Nothing to do. |
| 3.4 | `it` block parameter; Prism default; modular GC | — | — | Nothing to do. |
| 4.0 | `Ruby::Box` isolation (experimental), ZJIT "not yet as fast as YJIT", `Ractor::Port` | New JIT shipped slower than the old one | — | Evidence: keep old paths until the new one is measured faster. |

### PHP (8.1–8.5)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 8.1 | Enums, readonly, fibers, `never` | Fibers enabled async libraries without colouring | — | Nothing to do. |
| 8.4 | Property hooks, asymmetric visibility, `#[\Deprecated]` | Deprecation for user code | `#Deprecated` (D-STRUCT-LIFE1=A) | Nothing to do. |
| 8.5 | Pipe `|>`, `#[\NoDiscard]` with `(void)`, clone-with | — | D-DISCARD1=A | Nothing to do. |

### Dart (2.12–3.x)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 3.0 | 100% sound null safety | Took two years of opt-in migration with a tool | Non-null by default | Nothing to do. |
| 2025-01-29 | Macros cancelled | Compile-time and hot-reload cost (dart.dev blog) | Checked macros in the compiler | Watch macro build cost; no card. |

### Elixir and Erlang

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| Elixir 1.17 | Gradual set-theoretic types, warnings only | No code changes required (elixir-lang.org/blog, 2024-06-12) | Static types | Nothing to do. |
| OTP 24 | BeamAsm JIT; better error messages | Shipped default-on | — | Nothing to do. |
| OTP 28 | Priority messages, nominal types, old-style catch warnings | Priority is opt-in per alias | — | Nothing to do. |

### Haskell (GHC 9.0–9.12)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 9.0 | Simplified subsumption | Rejected working programs, because the old rule eta-expanded and changed meaning; `DeepSubsumption` came back as an extension | Open ballot D-INFER-RULE1 | Evidence for D-INFER-RULE1: an inference rule that rejects working code needs a migration path. |
| GHC2021/2024 | Default extension sets | The docs warn about instability | One edition | Nothing to do. |

### OCaml (4.14–5.3)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 5.0 | Multicore and effect handlers, "fully compatible with OCaml 4" | Some ports (MSVC) dropped, restored in 5.3 | — | Evidence: a runtime rewrite can keep source compatibility. |
| 5.3 | Deep effect handler syntax; statistical memory profiling back | — | — | Nothing to do. |

### Julia (1.7–1.12)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 1.9 | Native code caching; package extensions | Fixed "time to first plot" | — | Nothing to do. |
| 1.12 | `--trim` small binaries (needs no dynamic dispatch); struct redefinition | — | `!Dyn` denial (D-DENY-COST1=A) | Nothing to do. |

### Odin

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| monthly tags | No 1.0; breaking `core:os/os2` → `core:os` (2026-03); `core:sys/info` moved from globals to calls ("as little work in @(init)") | Users track monthly breaks by hand | D-CORE-GLOBAL-STATE1=A | Evidence: keep start-up work out of module init. |
| 2024-06 | `core:odin/printer` removed in favour of OLS | Tools consolidated | One formatter | Nothing to do. |

### Nim (2.0–2.2)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 2.0 | ARC/ORC default; `strictDefs` because default init was "one major source of bugs"; `strictEffects` | Definite init made default (nim-lang.org/blog, 2023-08-01) | No uninitialized locals; effects in signatures | Nothing to do. |

### V

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 0.4.x | Autofree "still WIP… avoid using it"; GC default | Promised feature never became default | — | Evidence: do not advertise a memory mode before it is default. |

### Lua (5.4–5.5)

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 5.5 | Global declarations; read-only `for` loop variables; incremental major GC | Accidental globals were Lua's oldest bug class | Loop variables are read-only (r06); module writes open (#6332) | Card N5 for r06's wrong text. Evidence for #6332. |

### Gleam

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| v1 (2024-03) | Semver promise: breaking changes only for security or soundness | Stable since | D-TOOL-STABLE2=A, D-ADOPT-LTS1=A | Nothing to do. |

### Mojo

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 1.0 (2026-08-11) | Per-API stability marks; breaking changes ship with deprecated aliases and fix-its | Betas in May and June 2026 (mojolang.org/releases/v1.0.0) | D-MATURITY1=B (doc-only markers) | Evidence for #6068 and D-DEPRECATE-INLINE1. |
| 1.0 | Removed negative indexing and `StringLiteral[i]`, which allowed out-of-bounds reads; `unsafe_` prefixes; fixed Optional/Variant corrupting Bool payloads | Safety fixes made late were breaking | Bounds checks, `#Unsafe` blocks | Nothing to do. |

### Carbon

| Version | Change | In practice (source) | Jet today | Jet action and why |
|---|---|---|---|---|
| 2025 goals → 2026 | C++ interop and memory-safety design; working 0.1 in 2026 | Still pre-0.1 | Polyglot binders | Nothing to do. |

## Gaps found (cards)

The card texts are in `answers/6376-cards.json`.

- **N1 — Checked SQL and HTML values can be built from any runtime string through their public fields.**
  `SQL{text: s, values: []}` and `HTML{markup: s}` compile from a runtime String.
  r01f returns both rows for input `x' OR '1'='1`, and r10 prints a raw `<script>`.
  E0149's own text says only a checked literal may build one; `SQL.raw` / `HTML.raw` are meant to be the single audited door.
  Jet already has `priv` fields (Regex, Files, Zstd use them).
- **N2 — A task blocked in an ordinary OS wait holds its worker.**
  In r08 the quick task waited 3.02 s behind 96 child waits.
  `jet_sys_waitpid_raw` issues `wait4` with no scheduler call (`HostProcess.jet:166-177`).
  Only runtime park points call `scheduler_blocking_wait_enter` (Condition, Shared, Task, TaskGroup, Realtime, Generator, AsyncEvents, SharedTransactions, Scheduler).
  D-ASYNCRT1=A promises "blocking calls yield automatically; no call-site annotation".
- **N3 — Every worker reserves at least 192 MiB of stack, and failing to start one stops the program.**
  `thread_stack_bytes` uses at least 201,326,592 bytes (`Thread.jet:358-370`).
  Under a 4 GB address-space limit, 32 trivial tasks stop with E3001 "could not start scheduler worker", exit 70, which blames the program (`Scheduler.jet:1389-1397`).
- **N4 — `jet fix` applies a deprecation rename that breaks the call.** See r11.
- **N5 — Reassigning a loop variable says it "was made with `::`" and suggests `i := ...`**, which cannot apply to a loop binding (r06).

Criteria for existing cards:

- **#5026**: `return`, `break`, `next` and `?` inside a defer body each give their registered code (r03b). Evidence: C TS 25755 and PEP 765.
- **#6332** (open D-MODULE-WRITE1): this is evidence, not a criterion. Swift 6 → 6.2 and Lua 5.5 both show that the module-write rule must not make correct single-task code carry concurrency annotations.

## Owner choice

- **D-DEPRECATE-INLINE1** (card #6405, `answers/ballots/D-DEPRECATE-INLINE1.json`, PLAIN OK and READY). The question: when a deprecated function's replacement takes different arguments, how should `jet fix` move callers?
  - A (recommended): inline the old body when it is a one-line wrapper (Go 1.26), else use the author's call pattern from the marker, else leave the call and print the exact edit.
  - B: rename only.
  - C: a call pattern for every migration.

## What Jet should not do (from the record)

- Do not ship a safety type with a second constructor that skips the check (Java JEP 465's reasoning; card N1).
- Do not make concurrency rules flag correct single-task code (Swift 6).
- Do not let any blocking call hold a scheduler worker silently (Java JEP 444 → 491; card N2).
- Do not promise automatic migration that a tool cannot perform (r11; Zig 0.15).
- Do not default to a new GC, JIT or memory mode before it is measured better in production (Python 3.14.5, Ruby 4.0 ZJIT, V autofree).
- Do not add metaprogramming whose compile-time cost is unbounded (Dart macros).
