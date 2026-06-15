# Epoch 2 development plan

**Status:** draft for owner concurrence, 2026-06-14.  
**Purpose:** one consolidated plan before agents split Epoch 2 into detailed
milestone files. Existing detailed plan: `m1-concurrency.md`.

Epoch 1 proved the language core. Epoch 2 should prove that Jet can be the
language a serious team chooses for production work: fast feedback, excellent
libraries, simple syntax, Rust-class memory safety, credible enterprise
operations, and a path into low-level/system domains without confusing
beginners.

## Product bar

Epoch 2 is done when Jet is no longer "a promising small-tools language" but a
production platform with these properties:

- **Best-in-class developer experience.** `jet check`, `jet test`, `jet dev`,
  LSP, docs, fix-its, and diagnostics feel instant, integrated, and teach as
  they go.
- **First-class libraries.** The core stdlib plus first-party `jet.*` packages
  cover CLI tools, data formats, streaming I/O, HTTP clients and services,
  logging, time, crypto primitives, archives, and database access.
- **Beginner-friendly syntax that experts still like.** The language keeps one
  obvious spelling for common code, while adding targeted post-v1 ergonomics:
  labels/defaults, error conversion, trait delegation, resource cleanup, and
  tier-2 references.
- **Memory safety remains the identity.** Ordinary Jet stays safe by default.
  Expert features are gated by imports plus `unsafe` blocks, and generated
  unsafe exists only inside user-audited regions or vetted std internals.
- **Enterprise adoption is credible.** Stability policy, edition/epoch policy,
  private registries, vendoring, SBOM/audit, source-level debugging,
  machine-readable diagnostics, coverage, CI artifacts, and cross-compilation
  are all planned and tested.
- **Single-file Jet remains sacred.** `jet run file.jet` never requires a
  manifest, package, workspace, config file, or project ceremony.

## Epoch versioning recommendation

Anthony Fu's [Epoch Semantic Versioning](https://antfu.me/posts/epoch-semver)
proposes separating large human-facing eras from smaller technical breaking
changes while remaining compatible with SemVer tooling. His encoded form is:

```
{EPOCH * 1000 + MAJOR}.MINOR.PATCH
```

For Jet, this idea is useful for **storytelling at launch**, but encoded numbers
like `1000.0.0` or `2000.0.0` are beginner-hostile and should not appear on the
toolchain until a deliberate initial public launch.

**Recommendation for concurrence:**

1. Use **Epoch 1, Epoch 2, ...** immediately for roadmap organization and
   internal release storytelling. The docs are already moving this way.
2. Keep **compiler/toolchain versions ordinary SemVer** until public launch:
   pre-1.0 `0.x`, then `1.x` for the first production era, `2.x` only when a
   real breaking toolchain release requires it — independent of epoch labels in
   docs.
3. **Defer encoded Epoch SemVer** (`1000.0.0`, `2000.0.0`, …) to the initial
   public launch milestone (E2-M17 or a separate launch decision). Until then,
   `jet --version` prints normal SemVer plus supported language epoch/edition,
   not an encoded epoch major.
4. Add a formal **compatibility policy** in E2-M2: what can change in a patch,
   minor, major, epoch, and edition.
5. Introduce an explicit project compatibility marker before breaking syntax:
   likely `[package].edition = "2026"` or `[package].epoch = 2`, decided in
   E2-M2. This is Rust-edition-style compatibility, not a license to churn.
6. Let the package registry enforce public API SemVer with sema-powered API
   diffs. Package authors may later opt into Epoch SemVer as a convention, but
   Jet should not require it for all packages.

**Owner decision E2-D1:** choose one external release policy before E2-M2
ships.

| Option | Shape | Recommendation |
|---|---|---|
| A | Compiler versions stay normal SemVer (`0.x` → `1.x`); docs use Epoch labels; encoded `1000+` only at public launch | **Recommended now** |
| B | Adopt Anthony-style encoded Epoch SemVer for the compiler now: Epoch 2 starts near `2000.0.0` | Powerful signal, but beginner-hostile — **not recommended pre-launch** |
| C | Use calendar/edition versioning for language compatibility, SemVer only for the binary | Viable, but needs more policy |

**Owner decision E2-D2:** what event flips on encoded Epoch SemVer (if ever)?
Options: never (A only forever), at E2-M17 GA only, or at a separate marketing
launch after GA hardening.

## Owner vision — decide before detailed milestone plans

These are CEO-level choices. Agents should not write detailed `mN-*.md` files
until each item is approved or explicitly deferred. Tactical syntax ballots stay
in `docs/admin/06-decision-ballots.md`; this section is product direction.

### Strategic decisions (whole epoch)

| ID | Question | Why it blocks planning | Options (one line each) |
|---|---|---|---|
| E2-V1 | Who is Epoch 2 GA *for*? | Sets showcase projects, docs voice, and quality bar | A: beginners + small-tool authors (default) · B: small teams first · C: enterprise platform buyers first |
| E2-V2 | What does "production platform" mean at E2-M17? | Defines GA exit criteria and what can slip | A: credible for internal services + CLIs · B: also public-facing SaaS · C: also regulated/audit-heavy environments |
| E2-V3 | Who must we beat convincingly? | Prioritizes libraries, DX, and honest limits in docs | A: Python/Node scripts · B: Go services · C: Rust small tools · D: Zig/C systems (pick primary + secondary) |
| E2-V4 | How sacred is single-file `jet run`? | Gates package/registry ceremony and onboarding | A: forever default path (recommended) · B: package-first for new users after tutorial · C: workspace-first for teams |
| E2-V5 | Concurrency model lock for the epoch | Prevents async creep and duplicate runtime stories | A: tasks/channels only in Epoch 2 (S53) · B: reserve async syntax for Epoch 3 with public design note · C: promote async inside Epoch 2 (needs new ballot) |
| E2-V6 | Expert/low-level appetite in Epoch 2 | Sizes E2-M13–M15 and I1 amendment scope | A: smoke demos only, not a selling point · B: credible for systems programmers, still gated · C: defer C FFI + freestanding to Epoch 3 |
| E2-V7 | Networking/services ambition | Sets E2-M10 scope and performance honesty | A: internal HTTP/CLI services only · B: small public APIs with TLS · C: defer services to Epoch 3 |
| E2-V8 | Supply-chain minimum bar | Sizes E2-M8 before first-party libraries ship | A: pub.dev-class (semver + lockfile) · B: enterprise-class (vendor, audit, SBOM, private mirror) · C: air-gapped-first |
| E2-V9 | Editor ecosystem priority | Allocates extension work after VS Code/Cursor | A: VS Code/Cursor + Zed dev extension · B: VS Code/Cursor only until GA · C: also Neovim in Epoch 2 |
| E2-V10 | Public launch trigger | Decides when encoded Epoch SemVer and marketing align | A: E2-M17 technical GA · B: separate launch milestone after audits · C: no encoded epoch ever |
| E2-V11 | Governance at launch | Trademark, foundation, advisory board, LTS ownership | A: OSS project, owner-led LTS · B: foundation prep in Epoch 2 · C: defer all governance messaging |
| E2-V12 | JetOS / pure eval / layer 3 boundary | Prevents research scope from becoming a product promise | A: `pure fn` + `jet eval --pure` only (S60) · B: package recipes in Epoch 2 · C: JetOS remains research-only (recommended) |

### Milestone decision gates

Each detailed plan needs these owner calls in addition to any syntax ballots.

| Milestone | Owner must decide | Default if deferred |
|---|---|---|
| E2-M1 | Whether async/shared-state stay out for the whole epoch (E2-V5) | Keep tasks/channels only |
| E2-M2 | E2-D1 release policy; E2-D2 launch flip; edition vs epoch marker; LTS length; who may run migrations (`jet fix` vs edition upgrade) | Option A SemVer; edition field; owner-approved migrations only |
| E2-M3 | JSON diagnostic schema stability; `jet doctor` scope; **Zed extension** in or out of Epoch 2 (E2-V9); shell completions priority | VS Code/Cursor first; Zed as dev extension; stable `--json` by M3 exit |
| E2-M4 | Interpreter coverage boundary; whether JIT is even designed in Epoch 2; save-to-diagnostic latency budget | Interpret common programs; JIT design-only; <200ms target |
| E2-M5 | Tier-2 teaching order; whether arenas ship; inlay hint defaults beyond clone | Teach after ownership ch.; arenas if needed for parser example |
| E2-M6 | Ratify S61/S62 timing; associated types vs full trait inheritance; `?` error conversion shape | Labels/defaults + delegation; associated types; `From`-style conversion |
| E2-M7 | `Path` type vs `std.path` module; handle vs RAII type names; keep `fs.read`/`fs.write` as sugar | Module helpers; RAII handles; keep whole-file APIs |
| E2-M8 | Registry hosting model; `jet.*` namespace policy; signing required or optional; yank rules | Git append-only registry; owner-held `jet.*`; signed metadata optional v1 |
| E2-M9 | First-party ring order; sqlite in-ring vs via FFI; crypto surface (hashes/HMAC/RNG only?) | CSV/TOML/log/time first; sqlite if E2-M14 ready; vetted primitives only |
| E2-M10 | HTTP/TLS dependency choice; max concurrent request story; Postgres vs sqlite showcase | Blocking + tasks; rustls-class TLS; sqlite-first service example |
| E2-M11 | Property testing in or out; `todo` typed holes; `jet tour`/`jet learn` vs docs-only | Snapshots + coverage required; holes deferred; docs-first learning |
| E2-M12 | DAP before or with GA; panic local value privacy; metrics conventions (OpenTelemetry-aligned?) | DAP for VS Code/Cursor; safe locals only; simple structured logs first |
| E2-M13 | I1 amendment wording; `unsafe` audit story (comments, attributes, or tool); `std.mem` API breadth | Generated unsafe only in user gates; comment audit; narrow mem API |
| E2-M14 | Jet-export to C in scope; header discovery strategy; which C deps ship as examples | Import-only first; pkg-config/classic flags; one small C lib example |
| E2-M15 | First cross target triple; freestanding panic strategy; CI embedded smoke vs doc-only | One non-host CLI target; abort default; documented harness minimum |
| E2-M16 | Package recipe scope; sandbox guarantees; signed cache generation/rollback depth | Pure eval + recipes; no ambient I/O; design signed cache, ship later |
| E2-M17 | Showcase set (which 6 demos are mandatory); perf/size budgets; **launch versioning** (E2-D2); beta period | Four showcases + `jet dev` demo; record budgets; normal SemVer at GA |
| E2-M18 | D-REPL1…21 ballots (see m18-repl.md); whether REPL ships before or after GA | Separate milestone after E2-M4; terminal REPL recommended; playground deferred |

## Milestone map

Use `E2-MN` in plans and prompts to avoid colliding with Epoch 1 milestone
numbers. Suggested plan filenames are included for the detailed plans to write
after concurrence.

| Milestone | File | Goal |
|---|---|---|
| E2-M1 | `m1-concurrency.md` | Tasks, channels, sendability, structured thread ownership — verified 2026-06-14 |
| E2-M2 | `m2-release-policy.md` | Stability, editions/epochs, deprecation, generated-code license |
| E2-M3 | `m3-dx-cli.md` | CLI polish, JSON diagnostics, `jet explain`, `jet doctor`, fix engine |
| E2-M4 | `m4-jet-dev.md` | Watch server, incremental check/run, interpreter-backed dev loop |
| E2-M5 | `m5-references.md` | Tier-2 references, `view`/`ref` hardening, zero-copy patterns |
| E2-M6 | `m6-library-authoring.md` | Traits v1.5, error conversion, labels/defaults, trait delegation |
| E2-M7 | `m7-streaming-io.md` | File handles, readers/writers, paths, RAII cleanup |
| E2-M8 | `m8-packages-supply-chain.md` | Registry, resolver, publish, vendor, audit, SBOM, private mirrors |
| E2-M9 | `m9-first-party-libraries.md` | Regex, CSV/TOML, log, calendar time, crypto, archive, database base |
| E2-M10 | `m10-network-services.md` | TCP/UDP, HTTP client/server, TLS, service ergonomics |
| E2-M11 | `m11-testing-docs-bench.md` | Doctests, coverage, snapshot UX, property tests, `jet bench`, `jet doc` |
| E2-M12 | `m12-debug-observe.md` | DAP/source maps, panic locals, structured logging/tracing/metrics |
| E2-M13 | `m13-low-level-tier.md` | `std.mem`, allocators, layout, `Ptr<T>`, volatile, unsafe audit model |
| E2-M14 | `m14-c-ffi.md` | `extern c`, headers/libs, C ABI imports/exports, pointer boundary rules |
| E2-M15 | `m15-freestanding-cross.md` | Cross-compilation, `no_std`/freestanding profile, embedded smoke target |
| E2-M16 | `m16-pure-eval-layer3.md` | `pure fn`, `jet eval --pure`, package recipes, sandbox/cache foundations |
| E2-M17 | `m17-epoch2-ga.md` | Production showcase, audits, performance, docs, release checklist |
| E2-M18 | `m18-repl.md` | `jet repl` — interpreter-backed interactive session (blocked on D-REPL1…21) |

## Dependency sketch

```
E2-M1 concurrency
  |-> E2-M7 streaming I/O -> E2-M10 network services
  |                            `-> E2-M12 debug/observe
  `-> E2-M5 references -------> E2-M13 low-level -> E2-M14 C FFI -> E2-M15 freestanding

E2-M2 release policy -> all public-breaking milestones
E2-M3 DX CLI --------> E2-M4 jet dev -----------> E2-M11 testing/docs/bench
                                              `-> E2-M18 repl (after M4 interpreter)
E2-M6 library authoring -> E2-M8 packages ------> E2-M9 first-party libraries
E2-M8 packages ---------> E2-M16 pure eval/layer 3

Everything -> E2-M17 Epoch 2 GA
```

Some milestones can run in parallel after detailed plans exist. The default
agent workflow should still implement one detailed plan at a time.

## E2-M1 - Concurrency (verified 2026-06-14)

**Existing plan:** `docs/plans/epoch-2/m1-concurrency.md`.

Goal: tasks and channels without data races, using ownership as the proof.
The surface is `tasks.spawn`, `Task<T>.join`, `tasks.channel<T>()`,
`Sender<T>.send`, and blocking `receive`.

Key exit criteria:

- Task closure capture rules are proven by sema, not rustc.
- Values crossing task/channel boundaries pass a structural sendability check.
- Dropped unjoined tasks lint.
- Channel examples are deterministic.
- Async/await, mutexes, atomics, thread pools, and shared-state concurrency stay
  out of scope unless evidence says tasks/channels cannot cover Jet's audience.

## E2-M2 - Release policy, editions, and epoch contract

Goal: write the promises enterprises adopt before they depend on Jet.

Scope:

- Adopt or reject external Epoch SemVer (E2-D1).
- Define compatibility levels: patch, minor, major, epoch, edition.
- Add a backward-compatibility guarantee for post-1.0 code.
- Add deprecation policy, migration window, and LTS intent.
- Decide project compatibility marker: `[package].edition`, `[package].epoch`,
  or toolchain-only constraints.
- State generated-code license policy explicitly: generated Rust carries no
  additional license obligation from the compiler.
- Define when `jet fix`, `jet fmt`, and LSP quick-fixes may perform migrations.

Exit criteria:

- `docs/admin` has a ratified compatibility/release policy.
- `jet --version` prints compiler version, language epoch/edition support, and
  std/package registry compatibility.
- Package manifests can reject unsupported future editions/epochs with a clear
  diagnostic.
- Every future breaking milestone lists the edition/epoch gate it needs.

## E2-M3 - Developer command UX

Goal: make the command-line experience feel as intentional as the language.

Scope:

- TTY-aware color/progress, `NO_COLOR`, `FORCE_COLOR`,
  `--color=auto|always|never`.
- Stable exit-code table.
- `--json` diagnostics for `check`, `build`, `test`, and package commands.
- `jet explain <E-code-or-L-code>` with offline examples.
- `jet doctor` for rustc, cache, PATH, LSP, package store, and registry health.
- Examples-first help text and friendly `jet` with no args.
- Shell completions and man-page generation.
- Unified fix engine shared by CLI `jet fix` and LSP code actions.
- OSC 8 terminal hyperlinks when supported.

Exit criteria:

- Golden tests pin human output and JSON output.
- CI mode is deterministic and ANSI-free.
- Every diagnostic points to `jet explain` without making error text noisy.
- `jet doctor` gives actionable fixes without network unless asked.

## E2-M4 - `jet dev`

Goal: instant-feeling development without changing release semantics.

Scope:

- Long-running watch server over the import/package graph.
- Reuse M13 LSP foundation: source overlays, incremental front end, crash
  policy, latency harness.
- Phase 1 execution: extend the comptime interpreter to whole programs where
  possible.
- Differential battery: interpreted output must match compiled output
  bit-for-bit for supported programs.
- Clear boundaries: FFI, tasks, native-only std modules, and low-level code may
  require a full build.
- Optional later phase: JIT plan, likely Cranelift, owner approval required.

Exit criteria:

- Save-to-diagnostic latency has a budget and test.
- `jet dev examples/31_cli.jet` watches, rechecks, reruns, and streams output.
- Unsupported programs fail with a plain explanation and a suggested full build.
- No release build ever uses the interpreter/JIT path.

## E2-M5 - Tier-2 references and zero-copy patterns

Goal: unlock Rust-territory programs without surfacing Rust lifetime syntax.

Current note: `view` returns and stored `ref` fields already exist in docs/code
as tier-2 machinery. This milestone should specify, harden, and test them as a
coherent post-v1 feature rather than inventing a new model.

Scope:

- Finalize stored/returned reference rules and labels (`ref[src]` etc.).
- Ensure references cannot cross task/channel boundaries unless explicitly
  proven safe.
- Add zero-copy string/list/map view APIs where they are worth the complexity.
- Add arena/owner patterns if needed to make graphs and parsers ergonomic.
- LSP inlay hints for borrowed returns and cleanup/borrow scopes.
- Guide chapter that teaches this after the beginner ownership chapter.

Exit criteria:

- Soundness matrix for returned views, ref fields, nested structs, generics,
  closures, tasks, and package boundaries.
- No user-written lifetime names.
- Diagnostics explain "what owns this?" and "how long can this view live?" in
  Jet words.
- Zero-copy parser example beats a clone-heavy version without becoming
  unreadable.

## E2-M6 - Library authoring ergonomics

Goal: make Jet excellent for authors of reusable libraries, not just users.

Scope:

- Generics v1.5: associated types and default method bodies.
- Re-evaluate trait inheritance and blanket impls only with evidence.
- Error conversion for `?` across different error types.
- Optional argument labels and trailing default parameter values (S61).
- Trait delegation `impl Trait using field;` (S62).
- API design lints for public package surfaces, kept advisory.
- Docs/examples for library API style.

Exit criteria:

- First-party packages can expose clean APIs without boilerplate explosions.
- `?` in multi-module programs works without same-error-type contortions.
- Argument labels catch transposed boolean/string arguments.
- Delegation removes real repeated forwarding code without invisible name
  injection.

## E2-M7 - Streaming I/O and resources

Goal: replace whole-file-only APIs with production I/O while keeping cleanup
automatic.

Scope:

- File handles, `Reader`, `Writer`, buffered readers/writers.
- `Path` type or path helper module, decided in the detailed plan.
- RAII cleanup contract from S63 as user-facing docs and tests.
- Streaming stdin/stdout/stderr.
- Line iteration, byte chunks, seek where available.
- Error conversion integrates with E2-M6.

Exit criteria:

- Large-file transform example runs with bounded memory.
- Resource cleanup happens on every exit path.
- Runtime errors name the resource and operation.
- Existing simple `fs.read`/`fs.write` APIs remain.

## E2-M8 - Packages and enterprise supply chain

Goal: finish the registry era and make dependency management acceptable to
teams with supply-chain requirements.

Scope:

- M12.2: append-only git registry, semver ranges, PubGrub resolver,
  `jet publish`, `jet vendor`, `jet audit`, local compile-once cache.
- Sema-powered public API diff, Elm-style publish enforcement.
- Private/internal registry and mirror configuration.
- Air-gapped builds via `vendor/` and `--locked`.
- SBOM emission from `jet.lock` in SPDX and/or CycloneDX.
- Advisory database format and audit command.
- Namespace ownership rules and immutable/yanked release policy.
- Signed registry metadata and optional signed binary/source cache design.

Exit criteria:

- Publish refuses breaking changes under a non-breaking version bump.
- `jet fetch --locked` and vendored builds work offline.
- Resolver conflict diagnostics are readable.
- Private mirror flow works without hard-coding public infrastructure.
- Single-file programs still bypass all package machinery.

## E2-M9 - First-party library ring

Goal: ship the batteries that make Jet feel complete without bloating core std.

First wave:

- `jet.regex`
- `jet.csv`
- `jet.toml`
- `jet.log`
- `jet.time` calendar/timezone package
- `jet.crypto` vetted hashes/HMAC/random primitives only
- `jet.archive` zip/tar/gzip
- `jet.db` base abstractions, sqlite first if FFI/runtime constraints allow

Rules:

- First-party packages use the same quality bar as the compiler: examples,
  docs, diagnostics, benchmarks where relevant.
- APIs prefer boring, unsurprising names.
- Fallible operations return `T ? E`.
- No hidden global mutable state.
- Pay for what you import/call remains a design constraint.

Exit criteria:

- Real examples replace common Python/Node/Go scripts: CSV cleanup, TOML
  config rewrite, log processor, archive unpacker, hash verifier.
- Package docs are generated and examples are tested.
- First-party package versions and API diffs are enforced by E2-M8.

## E2-M10 - Networking and services

Goal: enter Go's territory with blocking tasks/channels, not async syntax.

Scope:

- Blocking TCP/UDP sockets.
- HTTP client over streaming I/O.
- HTTP server for small services.
- TLS via vetted Rust library through the FFI tier; never hand-roll crypto.
- Service shutdown, timeouts, request limits, and channel-based worker patterns.
- Structured logging integration from `jet.log`.
- Basic config/env conventions without a framework.
- Postgres or sqlite-backed service example, depending on E2-M9/E2-M14 timing.

Exit criteria:

- HTTP client example calls a real API.
- HTTP server example handles concurrent requests with tasks/channels.
- TLS works through vetted dependencies.
- Docs state the scalability model honestly: blocking services are for the
  broad enterprise/internal-service case, not 100k-connection async workloads.

## E2-M11 - Testing, docs, and benchmarking

Goal: make quality workflows first-class.

Scope:

- `jet doc` from doc comments.
- Doctests: examples in docs run under `jet test`.
- Snapshot testing with one-command bless.
- Coverage output from `jet test`.
- Property testing with shrinking, if a small enough design exists.
- `jet bench` with warmups, repeated runs, variance, and comparison output.
- `todo` typed-hole expression, if owner approves the surface.
- `jet tour` or `jet learn` as guided compiler-coached exercises.
- Playground design if the interpreter is ready.

Exit criteria:

- Published packages get docs and tested examples automatically.
- Coverage works in CI-readable and human-readable modes.
- Bench output is statistically honest and scriptable.
- Beginner learning path uses real compiler feedback, not a separate tutorial
  language.

## E2-M12 - Debugging and observability

Goal: make production failures and local debugging understandable in Jet terms.

Scope:

- DAP/source maps so debuggers show Jet files and lines, not generated Rust.
- Panic reports in dev mode include relevant local values when safe.
- Error propagation traces for `?` where useful.
- Structured logging, trace context, and metrics conventions.
- `jet lsp` and `jet dev` integration for breakpoints/watch values where
  possible.
- Machine-readable runtime reports for CI/service logs.

Exit criteria:

- VS Code/Cursor can step through a Jet program at Jet source lines.
- Panic/error reports stay beginner-readable.
- Service examples emit structured logs and metrics without a framework.
- Generated Rust remains an implementation detail for normal users.

## E2-M13 - Expert low-level tier

Goal: provide C/C++/Rust/Zig-class control behind explicit gates.

Scope from S58:

- `import std.mem` discovery gate.
- `unsafe { ... }` audit gate and `unsafe fn` contract.
- `Ptr<T>`, pointer deref/math, transmute-class casts.
- Explicit allocators, including arenas and fixed allocators.
- Layout/repr controls.
- Volatile/MMIO wrappers.
- Clear amendment to I1: generated `unsafe` only inside user-gated regions or
  vetted std/mem internals.

Exit criteria:

- Beginner docs never require this tier.
- Every unsafe operation has a diagnostic if used outside the gates.
- Unsafe examples are small, audited, and tested against Rust output.
- Memory-safe Jet code pays no runtime cost for this tier.

## E2-M14 - C FFI

Goal: connect Jet to the non-Rust ecosystem without importing C's unsafety into
ordinary Jet.

Scope from S59:

- `extern c "header-or-lib" { ... }` blocks mirroring `extern rust`.
- By-value boundary first.
- Pointers only through E2-M13 rules.
- Linker flags/dependencies from `[dependencies:c]`.
- Header/library discovery diagnostics.
- Jet-export story for C callers, if scope allows.

Exit criteria:

- Example calls a small C library.
- Pointer misuse is rejected unless inside the low-level gates.
- C build/link failures become Jet diagnostics where possible.
- Rust FFI remains unchanged.

## E2-M15 - Cross-compilation and freestanding profile

Goal: avoid painting Jet out of embedded, kernel, and constrained targets.

Scope:

- `jet build --target <triple>`.
- Toolchain target detection and `jet doctor` support.
- `--freestanding` or `--no-std`-class profile using Rust `core` where
  possible.
- Allocator story tied to E2-M13.
- Panic strategy and binary-size profiles.
- Minimal embedded/freestanding smoke target in CI or documented local harness.

Exit criteria:

- Cross-compiled CLI artifact works for at least one non-host target.
- Freestanding smoke example avoids std-dependent APIs with clear diagnostics.
- Low-level tier enables the demo without leaking into normal Jet.

## E2-M16 - Pure evaluation and package layer 3

Goal: make purity a product feature and lay the groundwork for declarative
configuration/package recipes.

Scope from S60 and M12 layer 3:

- `pure fn` checked modifier.
- Purity in public signatures.
- `jet eval --pure`.
- Sandboxed package recipes on the existing store/lockfile.
- Signed binary/source caches and generations/rollback design.
- Integration path for `docs/plans/jetpack-jetos/README.md`: Phase 1 builds an
  independent `jetpack run/build/list/clean/add/remove` product track, while
  pure-eval/layer-3 work unlocks Phase 2 jetos system builds and installable
  ISOs.

Exit criteria:

- Pure evaluation is deterministic and has call-trace diagnostics.
- Impure calls fail with a path explaining why.
- Package recipes cannot perform ambient I/O or network access.
- A small declarative config example evaluates to stable JSON.

## E2-M18 - Interactive REPL (`jet repl`)

Goal: let users try Jet without creating a file — a teaching surface that
reuses the E2-M4 interpreter and the same diagnostics as batch compilation.

**Status:** plan only; **blocked on D-REPL1…D-REPL21** (Group 12). Full
decision tables and recommendations: docs/plans/epoch-2/m18-repl.md.

Scope (after ratification):

- `jet repl` with accumulating session (default) and optional cell mode.
- Interpreter-only execution with plain rejects for FFI, tasks, and native-only
  features.
- Meta-commands, multi-line input, transcript CI fixtures.
- Optional `--project` for manifest-aware imports (if owner ratifies).

Exit criteria:

- `tests/repl/` transcript suite green.
- Session bindings persist across inputs; ownership errors match batch E02xx.
- Unsupported features name `jet run` / `jet build` as the workaround.

## E2-M17 - Epoch 2 GA

Goal: prove the epoch with real projects, not feature checklists.

Showcases:

- A fast CLI tool with streaming I/O, regex/data-format libraries, tests,
  docs, and package publish.
- A small HTTP service with tasks/channels, logging, metrics, TLS, and a
  database or durable file store.
- A library package with public API diffing, docs, doctests, and semver
  enforcement.
- A C interop example.
- A low-level or freestanding smoke project.
- A `jet dev` demo showing instant feedback.

Audit gates:

- `cargo test` green through `nix develop`.
- Soundness fuzzing for ownership, refs, tasks, low-level gates, and FFI.
- Performance target recorded per showcase.
- Binary size and compile-time budgets recorded.
- Docs cover migration, compatibility, packages, services, debugging, and
  low-level gates.
- Every new diagnostic has `jet explain`.

## Deferred beyond Epoch 2 unless owner promotes

- Async/await.
- Shared-state concurrency primitives as the primary model.
- Exceptions.
- Safety-critical certification.
- A large web framework before the lower-level service story is proven.
- JetOS as a shipped OS product. Epoch 2 can build `jet eval --pure` and layer
  3 foundations, but JetOS should remain research until those foundations are
  real.

## Concurrence checklist

Before writing the rest of the detailed plan files, owner should explicitly
approve or edit:

### Versioning and policy

- [ ] E2-D1 external versioning policy (recommend A: normal SemVer until launch).
- [ ] E2-D2 when encoded Epoch SemVer (`1000+`) may appear, if ever.
- [ ] Whether E2-M2 should add `edition`, `epoch`, or both to `jet.toml`.

### Strategic vision (E2-V1…V12)

- [ ] E2-V1 primary GA audience.
- [ ] E2-V2 production-platform definition.
- [ ] E2-V3 competitive set (primary + secondary).
- [ ] E2-V4 single-file vs package-first onboarding.
- [ ] E2-V5 concurrency model lock.
- [ ] E2-V6 expert/low-level appetite (M13–M15 in or out).
- [ ] E2-V7 networking/services ambition.
- [ ] E2-V8 supply-chain minimum bar.
- [ ] E2-V9 editor priority (include Zed dev extension?).
- [ ] E2-V10 public launch trigger.
- [ ] E2-V11 governance at launch.
- [ ] E2-V12 JetOS / pure eval / layer 3 boundary.

### Milestone order and scope

- [ ] Milestone order and scope (table above).
- [ ] Whether `jet dev` is early enough at E2-M4.
- [ ] Whether package registry/supply-chain work should move before library
  authoring.
- [ ] Whether low-level tier and C FFI are both required inside Epoch 2.
- [ ] Whether pure evaluation/layer 3 belongs in Epoch 2 or should start Epoch 3.

### REPL (E2-M18 / D-REPL1…21)

- [ ] D-REPL1 — ship terminal REPL in Epoch 2 (recommend A).
- [ ] D-REPL2 — web playground scope for this milestone (recommend A: terminal only).
- [ ] D-REPL3 — command entry (`jet repl` vs bare `jet` in TTY).
- [ ] D-REPL4 — execution backend (interpreter vs compile vs hybrid).
- [ ] D-REPL5 — input unit (expressions vs statements vs full decls).
- [ ] D-REPL6 — hard rejects (FFI, tasks, packages).
- [ ] D-REPL7 — session model (accumulating vs cells).
- [ ] D-REPL8 — ownership across inputs (real moves vs auto-clone).
- [ ] D-REPL9 — multi-line input strategy.
- [ ] D-REPL10 — project/`jet.toml` context.
- [ ] D-REPL11 — line editor tier (std vs rustyline vs completions).
- [ ] D-REPL12 — relation to `jet eval --pure`.
- [ ] D-REPL13 — relation to `jet dev`.
- [ ] D-REPL14 — native-code fallback (reject vs temp compile).
- [ ] D-REPL15 — meta-commands set.
- [ ] D-REPL16 — result display (implicit echo vs explicit print).
- [ ] D-REPL17 — diagnostic voice in REPL.
- [ ] D-REPL18 — line-editing crate choice (if D-REPL11 ≠ A).
- [ ] D-REPL19 — web playground architecture (if D-REPL2 ≠ A).
- [ ] D-REPL20 — CI testing strategy.
- [ ] D-REPL21 — milestone timing vs E2-M4.

After concurrence, create one detailed plan file per milestone using
`docs/plans/README.md` protocol, starting with E2-M2 unless E2-M1 concurrency is
not yet verified.
