# Test-suite performance audit — 2026-08-08

## Executive summary

Jet's slow suites are compiler throughput benchmarks disguised as behavioral tests. The dominant work is not test logic. It is repeated native compilation of a large, mostly identical generated Rust program.

Static source counts show:

- `golden` discovers 479 example entries and starts about 476 native AOT compilations on a full supported host.
- `corelib` has 192 Rust tests and starts about 153 native AOT compilations in the default suite.
- `cli` currently has 186 `#[test]` functions and about 271 Jet process-launch sites. The earlier 162-test figure is stale. Many native CLI cases use optimized profiles with ThinLTO.
- Every ordinary generated program contains at least 331,699 bytes from `PRELUDE_PARTS`. Other always-emitted runtime parts bring the known base to about 445 KB. A program that uses Core can contain about another 1.54 MB of broadly included Core source before optional parts and user code.
- Direct test helpers and Jet's production AOT backend invoke `rustc` on that monolith. They have no Cargo target directory, reusable runtime object, or incremental dependency graph. Cargo's `sccache` wrapper does not cover these calls.

The largest safe fix is architectural: compile the canonical Prelude/Core runtime into content-addressed production `rlib` artifacts, then link each thin generated program against them. Each example must still pass through Jet, rustc, link, and execution. This preserves the real end-to-end proof required by I5 while removing repeated compilation of identical runtime code.

Two other large multipliers are independent of that architecture:

1. Full-corpus AOT oracle work is repeated in several `dev` differential batteries. A run-scoped exact artifact cache can remove the duplicate work immediately; the batteries can then be consolidated after their acceptance ratchets are mapped.
2. Many CLI behavior tests pay `-O` or opt-level 3 plus ThinLTO even when they test argument binding, diagnostics, or process behavior rather than optimization. Most should use a real debug-profile AOT build, with a small focused set retaining default and release profile proof.

All savings below are static estimates. No tests or compilers were run, per the audit directive. Ranges are non-additive and must be confirmed with per-phase timing after implementation.

## What each suite actually does

### Golden examples

`examples_compile_and_run` discovers the example corpus and runs a custom worker queue with at most 16 workers (`tests/golden.rs:75-212`). The current corpus shape gives 479 entries:

- 463 normal entries;
- one task-runner entry, which compiles and runs two tasks;
- four polyglot entries;
- 11 serde encoding entries.

The normal path calls Jet's frontend, writes the full generated Rust source, invokes standalone `rustc` once, and runs the binary (`tests/golden.rs:220-514`). The task-runner path does this twice (`tests/golden.rs:609-694`). Each serde encoding entry first calls the frontend and then calls `jet run --release`, which repeats frontend work and performs one native build (`tests/golden.rs:696-746`). This produces about 476 native AOT compilations, minus host-dependent GTK, raylib, or similar skips. Polyglot and FFI entries can add internal Cargo builds.

The worker queue itself is not the main problem. It already exposes 16-way concurrency. The problem is that each worker asks rustc to parse, type-check, and code-generate almost the same large runtime again.

### Corelib

`corelib.rs` currently contains 192 Rust tests. Static helper calls imply about 153 standalone rustc invocations in the default suite after excluding two ignored stress tests:

- about 142 calls to `build_and_run`;
- two calls to `build_and_run_multi`;
- about 11 other literal rustc launch sites.

The main helpers create a unique temporary source and output binary, call `jet::compile_with_path`, write the full generated source, invoke raw rustc, and run the result (`tests/corelib.rs:3681-3799`). They do not use Jet's native binary cache, Cargo incremental state, or a shared runtime object.

Libtest may run separate corelib tests concurrently. That means this suite can launch many large rustc processes at once. More concurrency is not automatically faster: after the CPU or memory limit is reached, extra rustc processes increase memory pressure and I/O contention.

### CLI

The current source has 186 tests and about 271 `Command::new(jet())` launch sites. It also has 28 literal `--release` tokens, plus conditional release and native `build` cases. Exact runtime process counts are higher or lower where loops and branches apply.

Default `jet run` normally uses the strict JIT and does not call rustc (`Source/CmdCompile.rs:354-390`). Native `build`, release runs, cross/FFI paths, and similar modes do call the production AOT backend. The default native profile uses optimization and ThinLTO; release uses opt-level 3 and ThinLTO (`Source/main.rs:240-363`). Those choices are correct product defaults, but they are unnecessarily expensive for many CLI tests whose assertion does not depend on optimization.

`isolated_cwd` deliberately gives compile/run/build tests separate working directories because Jet writes `build/<stem>` (`tests/cli.rs:295-319`). Isolation is good for correctness, but it also prevents accidental artifact sharing. Sharing must therefore be explicit and content-addressed, not a common mutable build directory.

### Common test helpers

`Scratch` creates one unique directory and removes it on drop (`tests/common/mod.rs:38-100`). `build_and_run` follows the same frontend -> full Rust file -> raw rustc -> binary path as corelib (`tests/common/mod.rs:449-521`).

`have_rustc()` starts `rustc --version` on every call (`tests/common/mod.rs:23-35`). Corelib alone has roughly 60 static call sites. This is small beside native compilation, but a `OnceLock` result is free to add.

### Full verification and CI sharding

`verify-full.sh` exports `JET_TEST_JOBS=16` and `CARGO_BUILD_JOBS=16`, then runs `cargo test --workspace` locally (`scripts/agent/verify-full.sh:20-44,151-165`). It does not set `RUST_TEST_THREADS`.

These controls cover different work:

- `JET_TEST_JOBS` controls custom queues such as golden's.
- `CARGO_BUILD_JOBS` controls Cargo compilation, not libtest threads or raw rustc child processes.
- CLI and corelib use libtest's own thread selection.

CI shards whole Cargo test targets with a simple sorted round-robin assignment (`tools/ci/test-shards.sh:43-73`). A target is atomic. A 45–90 minute `golden` target therefore remains a 45–90 minute shard even when six CI jobs exist. The current algorithm also ignores historical or static test weight.

## Why rustc dominates wall time

Code generation always pushes `PRELUDE_PARTS`, then emits other base runtime modules such as environment setup, memory support, GC, and layout (`crates/jet-codegen/src/Codegen/mod.rs:54-92,1844-1872`). The checked-in source sizes are approximately:

- `PRELUDE_PARTS`: 331,699 bytes;
- full known always-emitted base: 445,463 bytes;
- broadly included Core closure for a Core-using program: 1,540,471 bytes;
- typical minimum for a Core program before optional parts and user code: about 1.99 MB.

The Core closure includes large sources such as Unicode tables, XML pull parsing, encoding streams, and HTTP support. This means a small fixture can make rustc process megabytes of common implementation.

Across golden and corelib alone, about 629 native compiles repeatedly process the same base. Even the narrower 331,699-byte `PRELUDE_PARTS` figure represents at least 208 MB of duplicate source input. The real amount is much larger because most Core programs include the wider closure and rustc also repeats type checking, monomorphization, LLVM setup, code generation, and linking.

There is no meaningful "shared target directory" for these standalone rustc calls. A target directory is a Cargo concept. The equivalent reusable units are:

- a stable precompiled `rlib` or, for a narrow C ABI kernel, a static library;
- rustc incremental state with a stable crate identity;
- a final-binary cache keyed by all semantic inputs.

An `rlib` is the right main unit because the emitted Rust uses Rust types and generics. A static library can split only cold, nongeneric ABI kernels without recreating a broad FFI surface.

## Current cache behavior

Jet has a native build cache keyed from canonical program and toolchain inputs (`Source/BuildCache.rs:1-4,155-210`; `Source/CmdCompile.rs:393-443,2569-2636`). It caches final program artifacts. It does not cache a reusable compiled Prelude.

The cache helps repeated public CLI builds of the same unchanged program. It does not help:

- direct `jet::compile_with_path` plus raw-rustc helpers in golden, corelib, and common;
- the first build of hundreds of distinct examples;
- FFI, C-link, cross-target, or other bypass paths;
- repeated compilation of identical runtime source inside different programs.

`scripts/agent/jet-env` enables `sccache` only as Cargo's `RUSTC_WRAPPER` for a cold Cargo target and disables Cargo incremental in that case (`scripts/agent/jet-env:42-58`). The standalone rustc processes launched by tests and by Jet do not pass through Cargo, so that wrapper does not cover the expensive fixture builds.

Honoring a configured rustc wrapper is still useful for exact repeats and warm CI caches. It will not solve the cold corpus because different monolithic source files have different cache keys. It is a supplement to runtime splitting, not a substitute.

## FFI builds and serialization

`FfiBridgeLock` serializes all FFI bridge tests across test binaries through one directory lock under `/tmp`, with 50 ms polling (`tests/common/mod.rs:390-447`). Its comment says the FFI cache has no synchronization and always rebuilds.

That comment no longer matches the product. FFI now has a verified artifact fast path and a per-cache-key `BuildLock` (`crates/jet-pkg-model/src/FFI.rs:1660-1770,2215-2266`). The global test lock is therefore broader than the product lock needed for correctness.

Cold FFI builds have a second problem. Each distinct bridge cache key gets its own Cargo target directory and runs `cargo build --release` (`crates/jet-pkg-model/src/FFI.rs:1787-1883`). Common dependencies can therefore be rebuilt cold for many bridge keys.

The fix has two parts:

1. Share dependency artifacts by toolchain, target, profile, and dependency graph while keeping each final bridge product content-addressed.
2. Remove or key-scope the global test lock after concurrency tests prove that the product lock is sufficient. Bound concurrent cold Cargo builds by memory, not one lock for the whole suite.

## Repeated tier proof

Tier parity must remain. The waste is repeated compilation, not the requirement to compare AOT, JIT, and interpreter behavior.

Several corelib cases compile and run AOT, then invoke `Interpreter::dev_iteration` or `jet run` sequentially for the same source. Examples include codec, I/O, XML, derive, and typed-codec parity cases (`tests/corelib.rs:2702-3095,5056-5142,8056-8384,10516-10558,11007-11145`). These cases can parse, check, and lower once, then feed the same checked bundle to the separate execution seams. Independent execution legs can run concurrently where their fixtures do not contend.

The larger hidden duplication is in `tests/dev.rs`:

- `interpreter_matches_compiled_binary` builds an optimized AOT oracle across the corpus (`tests/dev.rs:1003-1028`).
- `dev_default_matches_compiled_binary` does another broad AOT pass (`tests/dev.rs:1033-1093`).
- `cranelift_three_way_differential_battery` repeats a resident-safe subset (`tests/dev.rs:7293-7353`).
- `example_corpus_strict_jit_aot_differential_gate` performs a combined AOT/JIT/interpreter corpus gate (`tests/dev.rs:7442-7665,8005 onward`).

The first two broad batteries can account for up to about 958 additional optimized AOT builds before exclusions. The exact accepted sets and ratchets differ, so they must not be deleted blindly. The safe first move is a run-scoped exact AOT artifact cache keyed by generated Rust bytes, rustc path/version, arguments, target, FFI inputs, and runtime fingerprint. The structural follow-up is to map every statistic and acceptance condition into one canonical combined corpus gate, then delete redundant batteries.

## Sleeps, probes, and sampling

These are not the main 30–90 minute cause, but they add fixed time and variance.

CLI development-tool tests exercise production sampling sizes:

- bench collection uses warmups and 20 measured samples;
- scene probes run 120 warmup frames and 600 measured frames;
- service probes perform 20 full down/start/readiness cycles, using a `sleep 30` service that is killed each cycle.

The relevant paths are `tests/cli.rs:938-1049,5281-5454` and `Source/CmdDevTools.rs:2480-2897`. One scene test invokes the probe three times; two are real misses because the third deliberately invalidates the evidence. These are real performance/lifecycle exercises, not ordinary functional assertions.

Keep one real default smoke for provider wiring and cache behavior. Move full sample counts, lifecycle repetition, and performance stability to a required opt-in or nightly `perf-real` lane. Keep deterministic unit tests that assert the production constants and evidence rules. This changes scheduling, not product behavior.

Corelib also contains fixed waits:

- two 250 ms TLS startup sleeps;
- a 500 ms stalled-TLS hold;
- an authentication expiry test that waits for a wall-clock second boundary, up to about two seconds;
- several 10–100 ms cancellation coordination sleeps;
- 1 ms polling loops.

Replace startup sleeps with readiness channels or connection retries, hold stalled peers with a channel, and give authentication tests an injectable clock. Keep the intentional 70 ms DNS timing assertion and actual timeout semantics. These changes save only seconds, but they reduce flakes.

Rare CLI `ETXTBSY` retry sleeps are defensive backoff and do not normally contribute wall time.

## Ranked plan

Estimates use the owner's reported 45–90 minute golden time and roughly 50 minute CLI time. They are source-derived, non-additive ranges. “Risk” means risk to proof quality or semantics, not implementation difficulty.

| Rank | Change | Estimated wall-time saving | Effort | Semantic/proof risk |
|---:|---|---|:---:|---|
| 1 | Build canonical production Prelude/Core `rlib` artifacts, content-keyed by exact runtime closure, rustc/toolchain, target, and profile. Emit a thin user crate and link it with `--extern`. | Remove about 60–85% of native compile time. Golden: roughly 25–75 min from the stated 45–90 min. Corelib and native CLI improve proportionally. | L | Medium-high. Rust visibility, generics, panic/runtime settings, targets, and I9 ownership must stay exact. Goldens still compile, link, and run each example end to end. |
| 2 | Add a run-scoped exact AOT artifact cache, then consolidate duplicate `dev` corpus batteries into the strict combined gate after mapping all ratchets. | Avoid up to about 958 duplicate optimized corpus builds. Likely tens of minutes to more than an hour from full verification, depending exclusions and cache overlap. | M for cache; L for consolidation | Low for an integrity-checked run-local cache; medium for consolidation. Preserve every tier, manifest, count, and failure classification. |
| 3 | Reclassify CLI native behavior tests. Use a real debug-profile AOT build unless the assertion concerns default/release optimization, packaging, or profile flags. Keep focused profile matrix tests. | Likely 10–30 min of the reported 50 min by avoiding `-O`/opt3 plus ThinLTO on dozens of behavior cases. | S–M | Low if classified case by case. Do not replace AOT with JIT; keep representative default and release end-to-end proof. |
| 4 | Combine related corelib fixtures into multi-case Jet executables with tagged results, targeting about 25–40 rustc launches instead of about 153. | Roughly 50–80% of corelib wall time if compilation dominates. | M–L | Medium. Preserve case isolation where process state, panic, timeout, or crash behavior matters. Every semantic assertion still executes compiled Jet. |
| 5 | Shard golden and combined dev corpora by deterministic entry, not only by Cargo target. Balance shards using static feature weights first and measured timings later. Verify exact union and no overlap. | Six balanced CI jobs could reduce a 45–90 min corpus target to about 8–20 min of CI critical path. Total compute is unchanged. | M | Low. Every example still runs. The shard-union test and stable failure reporting are mandatory. |
| 6 | Share FFI Cargo dependency artifacts across bridge keys, rely on the product's per-key lock, and replace the global test lock with bounded keyed concurrency. | About 50–80% of the cold FFI slice; plausibly 2–10 min per full suite, more on a cold cache. | M | Medium. Cache keys must include toolchain, target, profile, build inputs, and generated bridge content. Add corruption and concurrent-builder tests. |
| 7 | As an interim step, give each long-lived test worker a stable crate identity and private rustc incremental directory; centralize rustc launch and honor an explicit wrapper such as `sccache`. | Incremental prototype target: 20–50% of the AOT compile portion on a corpus worker. Wrapper: 70–95% on exact warm repeats, but little on a cold unique corpus. | M | Medium for incremental invalidation; low for wrapper plumbing. Never share one mutable incremental directory between concurrent workers. |
| 8 | Reuse one checked frontend bundle for AOT/JIT/interpreter parity cases and run independent tier legs concurrently under a resource budget. | About 5–15% on affected corelib/dev parity cases; larger if frontend work is repeated by subprocess APIs. | M | Medium. Engines remain separate and call the same Prelude semantics; this removes repeated frontend work only. |
| 9 | Move exhaustive bench, scene, and service sampling to an enforced `perf-real` lane. Keep one real provider/cache smoke in the default suite plus deterministic policy tests. | Seconds to a few minutes from CLI, with much lower variance and process churn. | S–M | Low-medium. The perf lane must remain required on an appropriate cadence; default tests must still prove real integration once. |
| 10 | Make readiness and time tests event-driven; inject the auth clock; cache `have_rustc`; promptly remove per-entry golden source/binaries from the RAM-backed verification temp root. | At least a few fixed seconds, likely under one minute on a healthy host; potentially much more if retained binaries push tmpfs into swap pressure. | S–M | Low. Keep actual timeout tests and diagnostic behavior unchanged. |

## Recommended delivery order

Start with changes that preserve the current proof graph exactly:

1. Add phase timing and counters around frontend, rustc, link, run, cache hit, and FFI Cargo build. Do not use timing assertions.
2. Add the run-scoped exact AOT cache for duplicate dev batteries.
3. Reclassify CLI profile use.
4. Key-scope the FFI lock and share its dependency artifacts.
5. Add deterministic entry-level CI sharding.

In parallel, prototype one runtime `rlib` boundary against a representative matrix: no-Core, Core-heavy, generics, panic, FFI, default/release, JIT parity, interpreter deopt, and web where applicable. Measure rustc frontend, codegen, and link separately. If the prototype confirms the expected reduction, make it the canonical production AOT path before changing golden helpers. Tests should consume the product path, not a test-only fast path.

After the runtime split lands, reassess incremental compilation and corelib batching. Their savings overlap with the `rlib` win and may no longer justify their complexity.

## Non-options

The following shortcuts would make the suite fast by weakening it and should be rejected:

- Replacing golden native builds with parser, sema, or codegen-only checks.
- Compiling one synthetic mega-program as the only proof that all examples independently build and run.
- Marking examples AOT-only, JIT-only, or parking parity in `tests/jit_gaps.txt`.
- Trusting a persistent developer cache without a forced-cold correctness lane.
- Sharing one writable build or incremental directory across concurrent tests without content keys and locking.
- Moving all real probes out of required verification. At least one real default smoke and a required exhaustive lane must remain.

The goal is not fewer semantics. It is fewer recompilations of identical semantics.
