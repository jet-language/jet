# Automatic build optimization

**Status:** proposal, 2026-09-01. Replaces the prior draft in full. The prior draft was treated as untrusted input; every claim below was re-derived from the tree at commit `8b9933668`, the live Tower board, and first-hand measurements taken on 2026-09-01. Nothing in this document is implemented.

**Scope:** compiler, package manager, dependency graph, build actions, caches, code generation, and linking, for every lens (`jet build`, `jet build --profile=debug`, `jet run`, `jet dev`, web).

**Goal:** beat Rust and Cargo on matched clean and incremental build workloads while Jet still transpiles to Rust, without giving up safety (I1), hidden rustc (I2), sema as the only checker (I3), diagnostics as products (I4), determinism, or execution-tier parity (I9).

**Hard rule:** optimization never changes a user-defined package, subpackage, or workspace boundary. Declared authority, policy, visibility, outputs, and dependency edges are preserved exactly. The optimizer works only inside those boundaries and across their declared graph. A large package gains fine-grained reuse without reorganization.

**Owner decisions taken on 2026-09-01 (chat, verbatim in Appendix A):** cards and ballots go to Tower now; the claim is a strict win against out-of-box *and* tuned Cargo unless impossibility is proven; hidden units inside a package are allowed; memoized module checks count as checking; the resident build session is opt-in; the store cap is adaptive (`min(20 GiB, 10% of disk)`, least-recently-used pruning); the project designs the public prebuilt-object seam now and operates it when infrastructure exists; the terminal shows a rich live board and `jet explain-build --html` ships; real programs gate the benchmark and synthetic twins are scaling curves only.

How to read this document: Part I is what exists today, with citations. Part II is the design. Part III is the proof still owed, the benchmark method, the hostile cases, the owner gates, and the card slate. A term is defined the first time it is used.

---

## Summary

A build is a graph of small jobs: check a module, emit Rust for a unit, run rustc on a unit, link an output. Today Jet runs that graph as one monolithic pass with eleven separate caches bolted on. Cargo runs it as one crate per package with mtime fingerprints. This proposal makes Jet run it as **one content-addressed action graph in one machine-wide store**, and gives the graph three things Cargo cannot have:

1. **Hidden units.** A package is compiled as several rustc crates that the user never names. A body edit recompiles one unit and relinks. Cargo must recompile the whole crate, and in release mode it must re-run the entire optimizer.
2. **Optimization that remembers.** Release builds emit LLVM bitcode per unit and let `rust-lld` run ThinLTO with a persistent cache in the store. Unchanged units skip optimization even though the whole program is link-time optimized. Cargo release builds have no such cache.
3. **A front end that never repeats itself.** Module checks are memoized on disk and shared by every lens. A no-change build verifies digests and replays its recorded diagnostics in tens of milliseconds. Dependencies are restored as sealed unit sets from local, team, and public store tiers instead of being recompiled per project.

The fast profile (`jet build --profile=debug`) does not use rustc at all: the ratified Cranelift path (D-AOT-CRANELIFT1=B) emits objects from checked TIR and links them against a prebuilt optimized runtime. That is the peer of `cargo build`, and it is where the largest margins are structural.

The hardest cell is a truly cold, single-package, optimized build of a tiny program, where fixed costs dominate and both sides run one rustc. The design attacks that cell with prebuilt runtime objects, cheaper generated Rust, parallel unit front ends, and a version-matched fast linker. It does not claim the cell is won until the benchmark says so.

---

## Part I — Current facts (verified 2026-09-01, commit `8b9933668`)

Documentation under `docs/` is stale by default; every fact here is backed by code, the Tower CLI, or a command run on this machine. Evidence files from the fact lanes live under `~/.cache/jet-luna/abo/` (session evidence, not repository content).

### I.1 What `jet build` does today

1. `jet build` enters `run_native_execution`, prepares the programmable-build front end, computes a native cache key, and calls the builder (`Source/CmdCompile.rs:2244-2314`, `:2490-2534`).
2. The front end loads, lexes, parses, resolves modules, runs sema, and classifies diagnostics into a reusable `PreparedBuildFrontEnd` (`crates/jet-driver/src/Driver/mod.rs:2990-3260`, `:3263-3295`). Lex and parse use a bounded eight-worker fan-out consumed serially in stable module order; the loader appends modules depth-first in import order and rejects import cycles with `E0604` (`crates/jet-driver/src/Loader.rs:3685-3705`, `:3774-3916`).
3. The native cache probe runs only after the front end has completed. Cache lookup is authorized only when parse, sema, policy, and diagnostics flags are all set (`crates/jet-comptime/src/Comptime/Build/cache_cas.rs:177-220`; `Source/CmdCompile.rs:2290-2314`, `:2371-2395`). A warm build can skip codegen, rustc, and link; it never skips the front end.
4. Codegen emits **one Rust source string**: the runtime prelude and Core closure marked by begin/end comments, one `mod __jet_<mangled>` per non-entry module with `super::` as its root prefix, then entry items at crate root and a `main` wrapper when needed (`crates/jet-codegen/src/Codegen/mod.rs:5291-5509`, `:335-341`, `:549-588`). Every generated crate starts with `#![allow(warnings)]` (`mod.rs:5319-5323`). Dependency packages are flattened into the same program carrier (`crates/jet-driver/src/Loader.rs:3440-3585`, `:3660-3925`; `crates/jet-pkg-model/src/Package/mod.rs:646-719`).
5. `RuntimeCache` splits the marked runtime and Core blocks into `jet_runtime` and `jet_runtime_core` rlibs under `~/.cache/jet/runtime` (or `JET_RUNTIME_CACHE_DIR`), keyed on source, `rustc -vV`, flags, and environment, verified by SHA-256 sidecars, bounded to 512 MiB (`Source/RuntimeCache.rs:33-37`, `:185-270`, `:489-547`, `:637-721`). The Core rlib content depends on the program's used-Core set (`crates/jet-codegen/src/Codegen/mod.rs:1214-1259`), so it is per program shape, not per toolchain.
6. One `rustc --edition 2021` invocation compiles the thin user crate against those rlibs and links it, in a private per-process work directory; the binary is stored in `BuildCache` and published atomically (`Source/CmdCompile.rs:7962-7975`, `:8167-8242`, `:8331-8347`). If the thin crate is rejected by rustc, the exact inline monolith is retried (`:8252-8309`). No `-C incremental` is passed; the source states the compiler recompiles from scratch each invocation (`Source/main.rs:3485-3488`).
7. Profiles: `jet build` default is opt-level 2 with thin LTO and strip; `--release` is opt-level 3; `run`/`dev` fast is opt-level 0, 256 codegen units, no LTO; host native builds add `target-cpu=native` (`Source/main.rs:306-326`, `:432-491`, `:495-604`).
8. Linker: `RUSTC_LINKER`, then `CC`, then mold, then lld through the C driver, else the target's system linker (`Source/NativeLinker.rs:96-165`, precedence at `:108-117`). A missing explicit linker is tool error `L2101`, exit 1, never an ICE (`Source/CmdCompile.rs:8306-8315`; `tests/cli_compiler_speed.rs:833-869`).
9. Toolchain: rustc is taken from `PATH`; `jet self doctor` requires `rustc 1.97.1` (`Source/Doctor.rs:17-18`, `:125-159`). No `rust-toolchain*` file exists. The environment receipt records rustc 1.97.1, LLVM 21.1.8, and a 32-thread Ryzen 9 7950X3D (`docs/reference/compiler-speed-environment-2026-08-25.json:1-55`).
10. Cranelift: `crates/jet-jit` pins `cranelift-jit`, `-module`, `-frontend`, `-codegen`, `-native`, and `-object` at 0.112 (`crates/jet-jit/Cargo.toml:17-31`). `try_compile_debug_aot` emits relocatable object bytes, but no CLI path calls it (`crates/jet-jit/src/jit/api_debug.rs:30-118`; `crates/jet-jit/src/lib.rs:504-510`). D-AOT-CRANELIFT1=B is ratified and unwired. Host support is x86_64 only (`api_debug.rs:24-28`).
11. `jet dev` is a foreground process: `WatchSession` samples existence, mtime, and length with a 30 ms debounce and a 120 ms tick, and hot-swaps a resident Cranelift program (`crates/jet-devserver/src/WatchService.rs:49-82`, `:713-879`; `Source/CmdDevTools.rs:462-596`, `:807-970`). It spawns a stdin reader thread and, for Canvas or static web hosts, binds TCP listeners for the browser (`Source/CmdDevTools.rs:348-361`, `:441-474`; `crates/jet-devserver/src/WebHost.rs:595-601`). Nothing survives the process, and no socket accepts build requests.
12. Generics: generic modules are expanded in sema (`crates/jet-sema/src/Sema/Bundle/GenericModules.rs:18-70`). Generic functions and methods are emitted as Rust generics (`crates/jet-codegen/src/Codegen/TIR/emit/functions.rs:849-878`, `:1247-1260`) and monomorphized by rustc; the JIT path instead asks sema to specialize demanded generic methods under a stable instance key (`crates/jet-codegen/src/Codegen/TIR/mod.rs:994-1012`, `:1135-1310`) and specializes a generic free function only when one concrete shape is demanded, skipping functions with several shapes (`:1387-1393`). Jet enforces an orphan rule: a trait impl needs a local target type or a local/builtin trait (`crates/jet-foundation/src/Traits.rs:947-1030`); an imported-provider/imported-target derive pair is `E2711` (`crates/jet-sema/src/Sema/Bundle/Pipeline.rs:1207-1218`).
13. Cross-module references render as `{root}{rust_mod}::{rust_fn}` or an inline-mangled name; user items are `pub` (`crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs:1761-1829`; `Items.rs:323-445`; `emit/functions.rs:756-805`). Single-crate globals in the program crate are the `#[used] #[no_mangle] #[link_section]` static `__JET_COMMAND_SCHEMA` and, for the counting allocator policy, one `#[global_allocator]` (`mod.rs:619-636`, `:1921-1935`). Runtime state lives in the runtime rlibs.
14. Interpolated strings emit Rust `format!` (`emit/helpers.rs:297-348`); scalar functions emit `#[inline(never)]`, `#Inline` markers emit `#[inline]`/`#[inline(always)]` (`emit/functions.rs:801-825`).
15. Web builds emit one `wasm_rust` string and run rustc once with `--target wasm32-unknown-unknown --crate-type cdylib`; web and cross builds never use the native cache (`crates/jet-codegen/src/Codegen/Web.rs:250-288`; `Source/CmdCompile.rs:6972-6997`, `:2119-2123`).
16. Inspection: `jet graph`, `jet query build`, and `jet explain-build` exist and read the declared `BuildPlan` (targets, actions, files, toolchains, probes, generated modules); compiler work is not in that plan (`crates/jet-cli/src/CLI.rs:388-393`; `Source/CmdCompile.rs:137-307`; `crates/jet-driver/src/Driver/mod.rs:2465-2578`).
17. Progress: `jet build` prints `Reading`, `Checking program and build plan`, `Generating native code`, `Building backend artifacts`, `Verifying build budgets`, and `Built <path> in <elapsed> ✓` to stderr without testing for a TTY (`Source/CmdCompile.rs:18-76`, labels at `:1829-1831`, `:2255`, `:2408`, `:2771`, `:2864`, `:2898-2902`). Phase timing is opt-in via `JET_TIMING=1` and writes `jet-timing.json` (`crates/jet-driver/src/PhaseTiming.rs:80-116`, `:244-329`).

### I.2 The eleven reuse mechanisms

| # | Mechanism | Store | Key | Integrity | Bound | Used by |
|---|---|---|---|---|---|---|
| 1 | `BuildCache` (final binary) | `~/.cache/jet/build` or `JET_CACHE_DIR`, `bin` + `bin.sha256` | canonical AST, target, profile, toolchain/compiler/linker identity, manifest and dependency interfaces, runtime/Core digests, instances, bridge identity, comptime inputs (`Source/CmdCompile.rs:6476-6592`) | SHA-256 on read (`Source/BuildCache.rs:109-166`) | none (`BuildCache.rs:1-347`) | `jet build` (not web, cross, C-linked, embed, or library outputs: `CmdCompile.rs:6453-6473`, `:8079-8086`) |
| 2 | `RuntimeCache` (runtime/Core rlibs) | `~/.cache/jet/runtime` | source, `rustc -vV`, flags, env (`RuntimeCache.rs:489-547`) | SHA-256 sidecar | 512 MiB FIFO | all native AOT |
| 3 | `RunCache` (tier-1 JIT module) | `~/.cache/jet/run` | source and dependency bytes and stamps, compiler identity, args (`Source/RunCache.rs:92-185`) | format-5 header only (`crates/jet-jit/src/jit/tier_cache.rs:31-41`, `:562-580`); no digest sidecar (`RunCache.rs:192-231`) | none | default `jet run`; all-or-nothing |
| 4 | `ReceiptStore` (whole invocation) | `<package>/.jet/receipts` | verb, cwd, argv, full env, tool identities, terminal mode, input digests (`Source/ReceiptStore.rs:88-125`, `:1054-1166`) | authenticated body digest, `jet-receipt-v2` | 64 MiB fields, 100k inputs | `check`, `build`, `test`, `prove`, `budget check`; disabled by `JET_RECEIPT_BYPASS` or `JET_TIMING` (`:502-557`) |
| 5 | Declared-action CAS and records | `.jet/build-cache/cas`, `.jet/build-cache/actions` | `act-sha256` over kind, argv, allowlisted env, inputs, outputs, caps, tool digests, target, toolchain, probes, labels (`crates/jet-comptime/src/Comptime/Build/errors_keys.rs:253-423`) | SHA-256 on read; atomic restore (`cache_cas.rs:3094-3258`) | remote 4 MiB/64 MiB/100k limits | `b.action`, compiler-owned package actions |
| 6 | Compiler package actions (#1422) | `.jet/build-cache/package-artifacts/<pkg>.sealed` | package, source digest, compiler, target, profile, dependency snapshots (`plan_impl.rs:263-295`, `:384-434`) | as #5 | as #5 | metadata receipt only; no Rust, TIR, or rlib payload (`Driver/mod.rs:3568-3594`) |
| 7 | `IncrementalSemaCache` / `CompilerQueries` | process memory only | module interfaces, dependencies, per-function bodies (`crates/jet-sema/src/Sema/Bundle.rs:748-977`) | n/a | n/a | `CompileMode::Check` only (`Pipeline/Completion.rs:242-247`); never `build`, `run`, or `dev` |
| 8 | Rust FFI bridge | `~/.cache/jet/ffi/` | toolchain, target, profile, rustflags, foreign descriptors (`crates/jet-pkg-model/src/FFI.rs:1-12`) | Cargo project cache | none | FFI builds |
| 9 | C bindings cache | see `GapFacts2.md §8` | binding inputs | digest | none | C imports |
| 10 | `BuildPlanReplay` | in memory | `jet-build-plan-replay-v1` codec | version check | n/a | jetpack provider and store replay (`crates/jetpack/src/Provider.rs:269-275`, `:1685-1692`; `crates/jetpack/src/Store.rs:1480-1493`); not used by the compiler's build path (`Comptime/Build/replay.rs:4-250`) |
| 11 | `PhaseTiming` JSON | `jet-timing.json` | n/a | n/a | n/a | perf dashboard; disables receipts when set |

Each has its own key derivation, storage, integrity rule, and failure behavior. Two of them (4 and 11) fight: measuring a build disables its replay.

### I.3 Law that already applies

Ratified on the Tower board (quoted from `decision show`):

- **D-INCR-UNIT1=A** — three-layer dirty model: item/query reuse (layer 1), module interface fingerprint invalidating importers only (layer 2), sealed package artifacts (layer 3). "No path skips sema or diagnostics on a cache hit that still needs checking."
- **D-LIB-REUSE1=B** — sealed package objects keyed on exact identity; generic bodies travel as typed IR and instantiate at the use site; a compiler upgrade empties the cache and rebuilds once; pinned Jet dynamic libraries with a checked compiler identity.
- **D-AOT-CRANELIFT1=B** — `jet build --profile=debug` lowers checked TIR through Cranelift to objects and links without rustc; unsupported targets fall back to rustc opt-level 0 and name the fallback in `jet explain build`. Owner comment: `jet check 0.007s vs ~12s rustc path`.
- **D-BUILD-DEFAULT1=B** — `run`/`dev` fast, `build` optimized; explicit `--profile` overrides.
- **D-BUILDACTION1=A, D-BUILDCACHE1=A, D-BUILDSCHED1=A, D-BUILDQUERY1=A, D-BUILDNORM1=A, D-BUILDREMOTE1=A, D-BUILDTOOLCHAIN1=A, D-BUILDPROBE1=A** — typed actions with declared inputs/outputs/argv/env/caps; default-on local action cache with a full explainable key, `--no-cache`, `jet explain-build`; deterministic scheduler with named pools (`cpu`, `memory`, `linker`, `console`, `gpu`, project pools); `jet graph`, `jet explain-build`, `jet query build` sharing one provenance API with the LSP; AST-level rename-sensitive cache normalization; local by default with remote cache and remote execution as separate policy grants; lock-recorded toolchains; reproducibility-class probes.
- **D-JPK-CACHEAUTH1=D, D-JPK-CACHECONFIG1=D, D-JPK-REPROCACHE1=D, D-JPK-REMOTE1=D, D-JPK-CACHE1=A, D-JPK-SELECTOR1, D-CASTORE1=A** — signed provenance from allowlisted writers with verify-on-read; host-bound mirror bindings (`jet cache bind`), never repo flags; quarantine of unreproducible outputs; offline-first ordered mirrors; `-p` and `--affected` derived from action-cache input hashes.
- **D-JPK-NODAEMON1=A** — "No daemon, no root ... no resident process ever — supervision parents services from the jet dev session ... violations need a new ballot."
- **D-JPK-SANDBOX2** — fetched or transitive executable steps require a strong sandbox; copy and prebuilt verification proceed.
- **D-PERFBUDGET-COMPILE1=C** — typed Clean, NoChange, and named-Edit compile workloads, one warmup, twenty samples, exact patch on a copied tree.
- **D-COSTLAW1=A (+C by owner comment)** — transparency surfaces plus a first-class optimizer.
- **D-VERDICT-687-1** — `jet run` is the JIT lens, `jet build` the AOT lens; missing JIT coverage is a compiler defect.

Open records that touch this scope: **D-BUILDPROFILE1** (spec-only import: blessed profiles `release`, `debug`, `ci`; `E1219`), **D-BUILD1**, **D-WORKSPACE2**, **D-JPK-NIXCACHE1**, **D-JPK-NIXINDEX1**. This proposal does not depend on any of them.

Competitive gate (`AGENTS.md`): per cell and metric; Rust parity only at Jet/Rust ≤ 1.05; the owner has tightened build cells to a strict win (Appendix A).

### I.4 Measured baseline

Measured on 2026-09-01 with `target/debug/jet` (a debug build of the compiler; **front-end phases are inflated by that, rustc and link phases are not**), rustc 1.97.1, LLVM 21.1.8, 32 threads, load average ≈ 9 before the runs, three trials per state, `JET_TIMING=1` (which disables receipt replay), all caches redirected into scratch so the machine's real caches were untouched. Wall seconds, median of three. Full tables: `~/.cache/jet-luna/abo/MeasureNow.md`.

| Program | LOC | State | Wall | Front end (parse+sema) | Backend (rustc of user crate) | Link (includes thin LTO) |
|---|---|---|---|---|---|---|
| `examples/features/devloop/job_runner.jet` | 15 | cold | 19.8 s | 0.10 s | 17.7 s (compiles runtime rlib) | 1.30 s |
| same | | no-change | 2.52 s | 0.10 s | 0.44 s | 1.28 s |
| same | | edit | 2.54 s | 0.10 s | 0.44 s | 1.29 s |
| `examples/features/time/datetime_accuracy_civil_arithmetic.jet` | 3349 | cold | 108.7 s | 26.6 s | 15.5 s | 28.7 s |
| same | | no-change | 103.3 s | 30.0 s | 0.43 s | 30.5 s |
| same | | edit | 99.5 s | 33.0 s | 0.43 s | 28.7 s |
| `gauntlet/entries/nbody/jet-expert/run.jet` | 144 | cold | 24.1 s | 0.20 s | 20.8 s (runtime rlib) | 1.75 s |
| same | | no-change | 0.97 s | 0.22 s | 0 (BuildCache hit) | 0 |
| same | | edit | 2.95 s | 0.20 s | 0.49 s | 1.48 s |
| `dogfood/jetpack` copy (package build) | 4358 | all three | ≈ 15 s to failure | 4.4 s | — | — (exit 101, see I.5) |

Three findings the numbers make plain:

- **The runtime rlib is the cold tax.** 17–21 s of every cold build on this machine is rustc compiling `jet_runtime` once per (rustc, flags, used-Core) identity. Cargo users pay nothing comparable because `std` ships prebuilt.
- **Thin LTO over the runtime is the warm tax.** The 3349-line program spends ≈ 29 s in link on *every* build, including no-change, because the default optimized profile runs thin LTO across the program and the full runtime bitcode each time. A per-module ThinLTO cache removes this for unchanged modules.
- **Reuse is inconsistent.** `nbody` hits the binary cache on no-change (0.97 s); `job_runner` and the 3349-line program do not (backend and link run again). The graph below has one rule for all of them.

Cargo peer rows for the `nbody` pair (`gauntlet/entries/nbody/rust-expert/main.rs`, 240 LOC): see §I.4a, filled from `~/.cache/jet-luna/abo/CargoPeer.md`.

### I.4a Cargo peer baseline

Same machine, same day, `cargo 1.97.0`, rustc 1.97.1, `RUSTC_WRAPPER` unset, default linker, three samples, wall seconds median. The Rust twin of `nbody` is `gauntlet/entries/nbody/rust-expert/main.rs` (240 LOC, no dependencies). The 1113-line row is `tools/ci/compiled-workload-peer-launcher.rs`, a Rust-only program with no Jet twin, included to show how Cargo scales. Full tables: `~/.cache/jet-luna/abo/CargoPeer.md`.

| Program | Command | cold | no-change | edit |
|---|---|---|---|---|
| nbody (Rust, 240 LOC) | `cargo build --release` | 0.62 s | 0.28 s | 0.55 s |
| nbody | `cargo build` (dev) | 0.55 s | 0.28 s | 0.46 s |
| peer-launcher (Rust, 1113 LOC) | `cargo build --release` | 1.05 s | 0.27 s | 1.05–1.58 s |
| peer-launcher | `cargo build` (dev) | 0.82 s | 0.27 s | 0.58 s |
| nbody (Jet, 144 LOC) | `jet build` (opt-level 2, thin LTO) | 24.1 s | 0.97 s | 2.95 s |
| nbody (Jet) | `jet build --profile=debug` (falls back to rustc opt-level 0 today) | 13.7 s | — | 2.86 s |
| nbody (Jet) | `jet run` (JIT) | 1.19 s | 1.20 s (warm) | — |

Today Jet loses every cell of this small program by 2× to 40×. The losses decompose exactly onto the levers in Part II: the cold cell is the runtime rlib compile (17–21 s; prebuilt runtime objects, II.7), the edit cell is thin LTO over the runtime at link (≈ 1.5 s here, ≈ 29 s on the 3349-line program; ThinLTO cache, II.6) plus a whole-crate rustc (hidden units, II.4), the no-change cell is a front end that always runs plus an unreliable binary-cache hit (memoized checks and receipts, II.3), and the debug cell is rustc at opt-level 0 where the ratified Cranelift path is unwired (II.6). The warm `jet run` shows no reuse at all on this program (`JET_RUN_TRACE=1` printed nothing), which the per-unit run artifacts replace.

### I.5 Defects found during measurement

- **Jetpack package build ICEs on master.** `jet build` at the root of a copy of `dogfood/jetpack` exits 101 with `internal compiler error: the generated Rust did not compile.` in all nine runs. Re-running rustc on the preserved generated file yields 49 errors: `E0308` ×33 (nominal module mismatch between `manifest`/`ref` `ParseError` types), `E0382` ×9 (`DataTree` value reused after move), `E0507` ×7 (moves out of optional `String`). Card #2350 fixed a different exit-101 cause at `77df06cc6`; this is a new one. Evidence: `~/.cache/jet-luna/abo/JetpackIce.md`, preserved generated source `~/.cache/jet-luna/abo/jetpack-ice-main.rs` (6,075,674 bytes including the embedded runtime and Core blocks). This is an I2 P0 and blocks the only large real program in the corpus.
- **Whole-invocation receipts never replay for `jet build`.** Two consecutive builds of the same unchanged program with the same copied `jet` binary (SHA-256 `cf196767…`) and receipts enabled produced two different receipt contexts (`35cbc60e…` and `086a4036…`); the second run rebuilt (68.8 s) instead of printing `ok: build current (receipt …)`. Something outside the inputs enters the receipt claim (`Source/ReceiptStore.rs:88-125`, `:1054-1166` hash verb, cwd, argv, the full environment, tool identities, and terminal mode). Evidence: `~/.cache/jet-luna/abo/CargoPeer.md`. Under the design, the receipt key is the invocation closure digest and nothing else; the hostile matrix (III.3, case 1) makes this a permanent test.
- **No-change builds under measurement re-run backend and link** for two of three programs (I.4). The receipt finding above explains the receipt half; whether the binary-cache misses share the cause is settled by the same test.
- **`RunCache` artifacts have no digest sidecar and no bound** (`Source/RunCache.rs:192-231`); `BuildCache` has no bound (`Source/BuildCache.rs:1-347`).

### I.6 Where the prior draft was wrong

- "Jet already wins cold builds by 6×": no source for that ratio exists; the Jetpack ledger marks every build metric *not measured* (`dogfood/jetpack/METRICS.md:1-14`, `:102-149`). Cold builds today are dominated by the runtime rlib compile.
- "Sealed package objects exist via #1422": the per-package action produces a `jet.sealed-package.v1` text receipt, not a compiled payload (I.2 row 6).
- "Incremental sema serves `jet build`/`jet run`/`jet dev`": it serves `CompileMode::Check` only, in memory only (I.2 row 7).
- It rejected hidden units and a resident session as out of scope for the transpile era. The owner has overruled both (Appendix A); Part II designs them.

---

## Part II — Proposed design

### II.1 Why Jet can beat Cargo while transpiling to Rust

Cargo's unit of compilation, dependency, caching, and parallelism is the crate the user wrote. Everything below follows from Jet owning the semantics (I3) and the whole program graph, so Jet can choose different units than the user's files without changing what the user declared.

| Lever | What Cargo does | What Jet does | Kind of win |
|---|---|---|---|
| Front end | rustc re-parses, re-resolves, type-checks, and borrow-checks the whole crate on every build | Jet checks per module, memoizes on disk, and emits Rust that rustc accepts by construction; rustc still parses and type-checks that Rust, but it is explicit, macro-free, and lint-free | structural on no-change and edits; fought on cold |
| Incremental unit | crate; `cargo build --release` has incremental off | hidden units: an edit recompiles one unit's implementation crate | structural |
| Optimizer | release re-runs LLVM on every codegen unit of the crate | per-unit bitcode plus ThinLTO with a persistent cache in the store; unchanged modules skip optimization | structural |
| Dependencies | compiled from source per project; sccache is opt-in | sealed unit sets restored from local, team, and public tiers by exact identity | structural |
| Standard library | `std` ships prebuilt | runtime and Core ship as prebuilt per-module objects for the pinned toolchain; today they compile once per machine | parity restored, then structural once the public tier exists |
| Parallelism | one serial front end per crate; LLVM parallel across codegen units | many unit front ends in parallel; ThinLTO parallel across modules | structural for large packages |
| Dev builds | rustc + LLVM at opt-level 0 | Cranelift objects from checked TIR, no rustc (D-AOT-CRANELIFT1) | structural |
| No-change | fingerprint walk with mtimes | digest verify with stamps as a first filter, replayed diagnostics | fought; small absolute numbers |
| Link | `rust-lld` by default on x86_64 Linux since 1.90 | `rust-lld` for release (version-matched ThinLTO plugin and cache); mold for the fast profile when present | parity to small win |

Honest physics. The irreducible work of a truly cold, single-package optimized build is rustc's front end on the generated Rust, LLVM optimization, and the link. Jet cannot skip any of the three while transpiling. It can shrink the first (cheaper Rust, parallel unit front ends), reorder the second (bitcode per unit, optimization once at link, cached), and match the third. For programs above a few hundred lines that arithmetic favors Jet. For hello-world-sized programs the two sides run about one rustc each and the result depends on fixed costs; that cell is measured, not asserted (III.1, A4).

### II.2 The one graph

Every command (`jet build`, `jet build --profile=debug`, `jet run`, `jet dev`, `jet test`, web) lowers to one `BuildPlan`, whether or not the package declares `fn build`. Today's plan holds only declared actions; the compiler's own work joins it as nodes of the same kind, keyed by the same `jet.action-key.v2` discipline (`errors_keys.rs:253-423`), stored in the same store (II.7), scheduled by the same scheduler (II.8), and explained by the same commands (II.11). No second graph, no second key format, no second cache.

| Node | Inputs (all by content digest) | Output | Executor |
|---|---|---|---|
| `Source` | file bytes; manifest; lock; workspace index; toolchain identity; settings; allowlisted environment | leaf digests | stamp-then-hash reader |
| `Check(module)` | module source; interfaces of imported modules; package policy and authority facts; comptime inputs; compiler identity | module **interface record** (signatures, types, layouts, exported generics), module **body record** (checked TIR incl. generic templates), diagnostics record, item digests | the existing `jet-queries` / `IncrementalSemaCache` engine, persisted (II.3) |
| `Partition(package)` | module import graph; impl-placement edges; entry module | unit manifest: unit id → sorted member modules; unit dependency DAG | pure function (II.4) |
| `Emit(unit, lens, profile, target)` | body records of members; interface records of imported units; emission policy version | generated Rust for `unit.iface` and `unit.impl` (release lens) or nothing (fast lens uses TIR directly) | codegen |
| `Compile(unit.iface)` / `Compile(unit.impl)` | generated Rust; `rmeta` of dependency `iface` crates; runtime object identities; rustc identity; flags; target | `.rlib` containing bitcode (release) | rustc |
| `Object(unit)` | body records; runtime symbol table; Cranelift version; target | `.o` | Cranelift (fast lens) |
| `Action(name)` | as today (D-BUILDACTION1) | declared outputs | sandboxed runner |
| `Link(output)` | all unit artifacts in dependency order; runtime objects; C libraries; linker identity; ThinLTO cache handle | binary, library, or wasm | `rust-lld` (release), mold or `rust-lld` (fast), `wasm-ld` |
| `Receipt(invocation)` | closure digest of every leaf the invocation read; verb; argv; terminal mode | recorded stdout/stderr, exit status, terminal artifact digests | replay |

Invariants:

- **Front end first.** No node that produces machine code or restores an artifact may run until every `Check` node the invocation demands is complete and its diagnostics are classified (today's `FrontEndCompletion` gate, kept: `cache_cas.rs:177-220`). Errors stop the graph before any rustc starts; warnings are recorded with the `Check` node and replayed verbatim on hits.
- **One key law.** A node's key is the canonical serialization of its input digests plus the executor identity. No mtimes, no absolute paths, no ambient environment beyond the allowlist. Sequence order is preserved where order is semantic; sets are sorted.
- **Early cutoff.** A node whose output digest equals the stored one marks its dependents' inputs unchanged. A comment-only edit changes a `Source` digest, re-runs one `Check`, and stops there because the body record digest is unchanged.
- **Same graph, every lens.** The lens changes which executors run (`Compile` vs `Object`) and which profile flags enter keys; it never changes `Check`, `Partition`, or dependency restore (D-LIB-REUSE1's "same artifact identity and the same restore path serve every lens").

### II.3 Identity and invalidation

The identity ladder, coarse to fine:

1. **Invocation** — `Receipt` key: verb, argv, terminal mode, and the closure digest. A hit re-renders the recorded typed diagnostics for the current terminal, re-verifies the terminal artifact's digest in the store, and prints the receipt line. Owner ruling: memoized module checks satisfy D-INCR-UNIT1's "no path skips sema or diagnostics on a cache hit that still needs checking", because a module whose inputs are identical does not need checking; the recorded diagnostics are the check (ballot D-BUILD-NOCHANGE1). Diagnostics are stored typed (code, message, spans, severity) and rendered per invocation, so color, width, and `--json` are always right; `--json` records are byte-identical under a versioned schema.
2. **Package** — manifest, lock entry, member module digests, policy, authority, dependency package identities, compiler identity, target, profile. This is the sealed package object identity (D-LIB-REUSE1).
3. **Unit** — sorted member module names plus the package identity. Unit identity is a function of structure, never of an index or a size, so adding a module changes only the units whose membership changed.
4. **Module** — the `Check` key is the module's raw source bytes plus its path plus the digests of every other input the check reads (imported interfaces, package policy and authority facts, lint settings, target and profile facts, compiler identity, generated or external inputs). Raw bytes, not a canonical AST, because diagnostics carry source positions. The canonical AST digest (D-BUILDNORM1: whitespace and comments stripped, names kept), the **interface digest**, and the **body digest** are outputs of `Check` that later nodes key on; a comment-only edit therefore re-runs `Check` and stops there. An audit test instruments every read a check performs and fails on an undeclared one; a node kind that fails the audit checks fresh until fixed.
5. **Item** — per-item digests inside the body record, used by the query engine for editor and warm-check reuse (D-INCR-UNIT1 layer 1) and by `Emit` for fragment reuse.

Change detection: every leaf is content-hashed with SHA-256 (the existing std-only implementation). Stamps (size, mtime, ctime, inode) let the reader skip hashing an unchanged file; a stamp can only *skip* a hash, never *assert* a change or a non-change on its own. A file whose bytes change under a preserved mtime (`cp -p`) is caught by ctime; a file rewritten with identical bytes is a hit.

Invalidation is dependents-only along recorded edges. A body edit changes the module body digest and therefore `Emit(unit)` and `Compile(unit.impl)`; the interface digest is unchanged, so no dependent unit re-runs rustc (II.4 explains how the two-crate unit makes rustc agree). A signature edit changes the interface digest and re-runs `Compile` for units that import that module's unit. A dependency version bump changes that package's identity and the interface digests of whatever the root imports from it.

### II.4 Hidden units

A **unit** is a set of modules of one package that rustc compiles together. Units are invisible: no name in the manifest, no authority, no policy scope, no import semantics. They appear only as detail rows under their package in `jet explain-build`.

**Partition.** Build the module graph of the package: nodes are modules, edges are imports (acyclic today, `E0604`) plus **impl-placement edges**: a trait impl written in module M for type T (module `mod(T)`) and trait R (module `mod(R)`) must live in the crate of T or of R under Rust's orphan rule, so the partition adds the edge M → `mod(T)` (or `mod(R)` when T is foreign to the package) and places the impl's emission with that module. Collapse strongly connected components into units. Because imports are acyclic, cycles come only from impl placement; a cycle is a legal Jet program and simply yields a larger unit. The entry module is always its own unit. Units are named by the sorted list of member module paths (identity), and displayed as `<package>/<first-member>` (label). The partition is a pure function of the package's structure: identical on every machine, so sealed unit sets are shareable across the team.

Coalescing small units by size or by machine core count is **not** done: it would make identity depend on edit-sensitive sizes or on the machine, which breaks both stability and sharing. If measurement shows per-unit fixed costs dominate for packages with hundreds of tiny modules, coalescing by directory (a structural rule) is the fallback, gated on evidence (III.4).

**Two-crate unit (release lens).** rustc invalidates a dependent crate whenever a dependency's crate hash changes, and that hash covers every body in the crate. A single crate per unit would therefore recompile every transitive dependent on any body edit. Each unit is emitted as two crates:

- `unit.iface` — every type and enum with its fields and layout, every trait, every inherent impl and trait impl for the unit's types (Rust requires inherent impls in the crate that defines the type), every generic function and generic method body (rustc must see generic bodies to monomorphize them), non-generic method bodies as **forwarders** to implementation symbols, and one `unsafe extern "Rust" { pub safe fn ... }` declaration per non-generic free function of the unit, with a Jet-chosen symbol name.
- `unit.impl` — the non-generic bodies (free functions and the targets of method forwarders), each with `#[export_name = "<jet symbol>"]`. It depends on its own `iface` and on the `iface` crates of the units it imports. Nothing depends on an `impl` crate; they are only linked.

A body edit changes only `unit.impl`. Dependents compile against `unit.iface`, whose bytes did not change, so their `Compile` keys hit. A signature, type, trait, or generic-body edit changes `unit.iface` and correctly recompiles its importers.

Symbols: `_JET_<package-hash>_<module-path>_<item>_<abi-hash>`, where the ABI hash covers the signature and the ABI record (exact rustc build, target triple and features, panic strategy, layout-affecting flags). Every unit of a program is compiled by the same rustc, and the ABI record is in every `Compile` key, so the hash turns any iface/impl skew into a link error (an ICE, never undefined behavior). `safe fn` declarations in `unsafe extern` blocks are stable Rust since 1.82 (RFC 3484) and are exactly the construct for "the declarer vouches for these signatures", which is what a checked interface record is. Under I1, generated Rust `unsafe` is allowed only in user `#Unsafe` regions or vetted std/mem internals; the compiler's own symbol declarations for symbols it emitted are proposed as vetted internals, and that reading is an owner gate (ballot D-BUILD-UNITS1). Per-target contract: ELF and Mach-O keep export names through `--gc-sections`; COFF/MSVC keeps them through `/OPT:REF`; wasm units use `wasm-ld`. A target that cannot honor the contract falls back to single-crate units for that target and `jet explain-build` says so. If the owner declines the reading entirely, the fallback is single-crate units everywhere: still parallel, still edit-proportional for the optimizer (II.6), but transitive dependents re-run rustc on body edits. A one-interface-crate-per-package variant was considered and rejected: it is a serial rustc run on every critical path and recompiles every unit after any signature change.

Cross-unit calls to `extern` symbols are not inlinable by rustc. The release lens restores cross-unit inlining at link time through ThinLTO (II.6); the fast lens does not inline in any case.

**Generics, two phases.**

- *Phase 1:* generic functions and methods are emitted as Rust generics in `unit.iface`, as today; rustc monomorphizes each instantiation in the unit that uses it. Correct, simple, and the same LLVM duplication Cargo has across crates.
- *Phase 2:* Jet instantiates generics itself, extending the sema specialization path the JIT already uses for demanded methods (`TIR/mod.rs:1135-1310`) and for single-shape free functions (`:1381-1473`, which today skips functions demanded in several shapes at `:1387-1393`), and places each instance deterministically in the unit that owns its most specific package-local type argument (ties: the generic's own unit). Instances become ordinary non-generic functions with exported symbols; `unit.iface` then contains no bodies at all except impl forwarders, so interface digests change only on real interface changes, and every instantiation is compiled exactly once in the program. Phase 2 lands only if Phase 1 measurement shows duplicate monomorphization or interface churn in the top costs (III.4).

**Placement of single-crate globals.** `__JET_COMMAND_SCHEMA`, the optional `#[global_allocator]`, and the `main` wrapper are emitted only in the entry unit's `impl` crate. Runtime state already lives in the runtime objects.

**Visibility.** Generated items are `pub` across the package's crates. Product visibility (`pub`, `pub(package)`, private) is enforced by sema (`crates/jet-sema/src/Sema/Bundle/Outputs.rs:121-137`) and is unchanged; rustc visibility was never the product's visibility (today's single crate already makes everything reachable).

### II.5 The package boundary law, mechanically

- **Sealed package object = the package's unit set.** For a dependency package the `Emit`/`Compile`/`Object` nodes of its units, its interface records, and its generic templates (typed TIR, D-LIB-REUSE1 half one) form one artifact identity keyed at ladder level 2. Restore is per package: a package never splits into artifacts with independent trust, authority, or visibility, and packages never merge into one artifact.
- **Authority and policy** (`PackageAuthority`, `policy.contain`, `policy.harden`, lint deny, effect ceilings, unsafe paths: `crates/jet-pkg-model/src/Package/mod.rs:241-340`) are decided by sema per package exactly as today and recorded in every `Check` node of that package. Containment fences are dependency boundaries, which are package boundaries. Crate-level attributes a package's policy requires are emitted on every crate of that package.
- **Dependency edges** derive from declared `deps:` and resolved cross-package imports, never from path coincidence, and are never rewritten for scheduling convenience. Workspace membership is exactly the `workspace` index.
- **Outputs** remain the nine closed kinds (`Library`, `Executable`, `Service`, `Check`, `Environment`, `Image`, `Bundle`, `System`, `Fleet`); each is a `Link` sink or an action sink. `Library{native:true}` and `.jetlib` sealing are unchanged.
- **Build actions** (`b.action`) stay declared graph nodes with the D-JPK-SANDBOX2 sandbox law; generated modules flow into `Check` nodes through declared outputs.
- **Selectors** `-p` and `--affected[-since]` (D-JPK-SELECTOR1) compute identically with the store warm, cold, or bypassed.
- **Boundary regression proof** (III.1, A9): warm, cold, and `--no-cache` builds produce byte-identical manifest facts, lock contents, authority and policy decisions, diagnostics, outputs, and selection results.

### II.6 Lenses

**Release lens (`jet build`, `--release`, blessed profiles).** `Compile(unit.*)` runs rustc with the profile's `opt-level`, `-C linker-plugin-lto`, `-C embed-bitcode=yes`, `-C codegen-units=1` per unit (units are already small; the linker parallelizes), `-C metadata=<unit key>`, `--remap-path-prefix <workdir>=/jet/build`, and `--crate-type rlib`. `Link` runs `rust-lld` (shipped with rustc, so its LLVM matches the bitcode) with `--thinlto-cache-dir=<store>/lto/<output identity>` and a size-bounded pruning policy. ThinLTO keys each module's optimized object by its summary and imports, so an edit re-optimizes the changed unit and the modules that inlined from it, not the program; the runtime's modules are optimized once and reused forever. Runtime performance is unchanged: thin LTO across units at link time equals today's thin LTO within one crate, and the gauntlet runtime cells guard it (III.1, A10). mold stays out of the release lens because rustc's LTO plugin is version-bound and mold's plugin path has known failures with `-C linker-plugin-lto`; mold remains the fast-profile linker when installed.

**Fast lens (`jet build --profile=debug`).** `Object(unit)` lowers checked TIR through Cranelift to relocatable objects (D-AOT-CRANELIFT1=B; the entry point exists: `api_debug.rs:30-118`) and `Link` joins them with the **prebuilt optimized runtime** objects (II.7), the way Rust links an optimized `std` into debug binaries. No generated Rust, no rustc. Unsupported targets fall back to the release-lens executors at opt-level 0 and the fallback is named in `jet explain-build`, as ratified.

**Run lens (`jet run`, `jet dev`).** The all-or-nothing `RunCache` becomes per-unit tier artifacts: `Object(unit)` outputs in the JIT's format-5 payload, keyed like every other node, with digest sidecars and bounds. An edit re-lowers and recompiles the dirty unit; unchanged units reload machine code. `jet dev`'s resident swap swaps the dirty units. The default lens stays the tiered JIT (D-VERDICT-687-1); interpreter deopt paths call the same Prelude symbols (I9).

**Interpreter.** No artifacts; it is the reference semantics and takes part in every differential (R12).

**Web.** `Emit`/`Compile` wasm units, `Link` with `wasm-ld`, through the same graph and store. Web builds gain the no-change short circuit and front-end memoization immediately; per-unit wasm ThinLTO is measured, not assumed.

### II.7 The store

One machine-wide store replaces rows 1–6 and 8–9 of I.2:

```
~/.cache/jet/store/
  cas/<sha256>                 immutable blobs (rlib, .o, rmeta, generated Rust, records)
  ac/<action-key>              action-cache records: output digests, diagnostics, timings, executor identity
  lto/<output-identity>/       ThinLTO caches, each bounded, pruned by the linker's policy and by the store
  journal                      append-only last-use log for LRU; compacted on prune
  store.lock                   short critical sections for publish and prune
```

Rules, applied to every entry, not piecemeal:

- **Write:** temp file in the same directory, fsync, rename. A crash never leaves a partial entry.
- **Read:** verify the SHA-256 of every blob before use; a mismatch is a miss, the entry is removed, one `note:` line is printed, and the build proceeds. Never a failure, never a silent stale result.
- **Records are versioned** (`jet.store.v1`); a reader refuses other versions and treats them as misses. A compiler upgrade changes every key (compiler identity is in every node) and prints one line: `note: Jet X → Y: the store holds nothing for this compiler; rebuilding once` (D-LIB-REUSE1's accepted loss).
- **Bound (ballot D-BUILD-STORE1, recommended option E):** the default cap is `min(20 GiB, 10% of the filesystem holding the store)`, and a stricter host policy may lower it outside any repository. Admission is checked **before** every write against available space (`statvfs` available blocks, or the Windows volume API), never leaving less than a 2 GiB reserve free and always leaving room for the temp-plus-rename; the store evicts before it admits and refuses an entry that cannot fit safely with a tool error naming the path. Eviction is least-recently-used from a checksummed, size-counted journal, run opportunistically after a command (D-JPK-NODAEMON1: post-command work, no resident process). `jet cache status` prints the footprint, the effective limit and where it came from, and the tiers; `jet cache prune [--to <size>]` prunes now; `jet self doctor` shows the footprint row.
- **Concurrency:** two `jet` processes publishing the same blob race benignly (same content, same name); action records are immutable per key; a per-key in-flight lock lets a second process wait instead of duplicating rustc work and falls back to computing if the lock holder dies; **live leases** with crash recovery pin every entry a running build reads, so no process can prune another build's inputs.
- **Project directory:** `.jet/` keeps the lock, the workspace index, and a small pointer to the last receipt. Deleting `.jet/` loses nothing but that pointer; deleting the store loses only time.
- **Tiers:** `local` (this store), `team`, `public`, exactly as D-JPK-CACHECONFIG1/D-JPK-REMOTE1/D-JPK-CACHEAUTH1/D-JPK-REPROCACHE1 ratified: host-bound ordered mirrors, signed provenance from allowlisted writers, verify every hit, quarantine divergent outputs. Sealed unit sets and prebuilt runtime objects are two more object kinds flowing through the same tiers; nothing is re-encoded.
- **Prebuilt runtime and Core (ballot D-BUILD-PREBUILT1, recommended option D).** Core is split into fixed per-module units (`core.json`, `core.http`, …) instead of one program-shaped closure, so runtime objects have a fixed identity per (compiler, `rustc -vV`, target triple, ISA level, platform ABI such as glibc/musl baseline or macOS deployment target or MSVC toolset, linker and LTO version, panic/unwind settings, target features, profile flags). The first machine build compiles them once and stores them; the public tier, when the project operates it, serves them signed by the project's release key so a fresh machine downloads instead of compiling (the Rust `std` model). Third-party package objects may also appear on the public tier, but only under separate signing roots of registry-authorized builders, only when reproducible, and only for hosts whose policy opts in; the project's key never vouches for builds it did not perform. Signed provenance carries expiry or transparency checkpoints and revocation metadata, and the toolchain pins the expected runtime identity so a stale signed runtime cannot be substituted. Pay-for-what-you-call (R10) moves from compile time to link time: only referenced units are linked and ThinLTO or `--gc-sections` drop the rest. Shared objects are built for the family baseline (`x86-64` psABI level 1 on x86_64, base `aarch64`, per-architecture objects on macOS, no CPU tuning for wasm) with optional higher levels selected after CPU detection; the root package keeps `target-cpu=native` on host builds unless the user asks for shareable output.

### II.8 Scheduling

D-BUILDSCHED1's scheduler runs the whole graph. Pools: `cpu` (rustc, Cranelift, checks; limit = logical cores), `link` (limit 2; memory heavy), `io` (store reads, hashing), `net` (remote tiers). Additional rule for rustc: the effective `cpu` limit is `min(cores, available RAM / 1.5 GiB)` so a large package cannot swap the machine. Ready nodes are ordered by longest remaining path (estimated from durations recorded in the store; estimates change order, never outputs), then by key for a total order, so two runs with the same inputs schedule identically. Independent subgraphs keep going after a failure; dependents of a failed node are cancelled (as ratified). Diagnostics print in stable module order regardless of completion order. Speculative work (compiling units unaffected by an error while the user fixes it) runs only inside an opt-in resident session (II.12).

### II.9 Failure behavior

| Situation | Behavior | What the user sees |
|---|---|---|
| Sema error in any module | graph stops before any `Emit`/`Compile`; nothing is stored for the failed invocation | diagnostics, then `✗ shop not built · 2 errors · 0.31s · nothing compiled, store unchanged` |
| rustc rejects a unit | ICE (exit 101), generated Rust preserved for the report; **no inline-monolith retry** (the fallback in `CmdCompile.rs:8252-8309` is deleted: a rejected unit is a compiler bug, not a fallback case) | the existing ICE text plus the unit and generated path |
| Store entry fails verification | miss, entry removed, rebuild | `note: rebuilt shop/net — store entry failed verification (why: jet explain-build shop/net)` |
| Store version mismatch or compiler upgrade | every key misses once | one `note:` line |
| Disk full during publish | tool error naming the path; no partial entry | `Error [L21xx]` with the path and the `jet cache prune` fix |
| Linker missing | existing `L2101` | unchanged |
| Remote tier unreachable | offline-first: local only, recorded in explain-build | `note: team store unreachable (cache.acme.dev); using local store only` |
| Remote object fails signature or digest | quarantine (D-JPK-REPROCACHE1), local compile | `note:` line naming the object and the tier |
| Resident session stale or dead | `jet build` runs as a fresh process; outputs identical by construction | nothing, unless `--verbose` |
| Two builds of the same project at once | both proceed; identical results; duplicate work avoided where the in-flight lock is taken | nothing |

Every `note:` and error text is registered product copy with a UI snapshot (I4).

### II.10 Beginner defaults

Nothing to type, nothing to configure, nothing to learn. `jet build` is optimized and fast; the second build is faster; a no-change build is instant; an upgrade prints one line; a broken cache heals itself. The words *unit*, *store*, *ThinLTO*, and *tier* never appear unless the user runs `jet explain-build`. There is no clean step and no cache flag a beginner needs (`--no-cache` exists for experts, ratified). The only new default-visible surfaces are the live board and the receipt line (II.11).

### II.11 Expert inspection and UI

Owner direction: rich live board plus HTML report, in the Jet color scheme (black and red), modernized. Command names are law (D-BUILDQUERY1). Mockups for visual acceptance: `docs/proposals/automatic-build-optimization/mockups/terminal.html` and `report.html` (self-contained; rendered in headless Chromium from the full devshell; screenshots in III.6). The frames below are the design; the mockups reproduce them byte for byte and the mockup page checks that on load.

**Live board (TTY, ≥ 100 columns).** A ≤ 8-line region redrawn in place at ≤ 10 fps. The first line is the state line: spinner while running, then it becomes the receipt. One row per stage (`check` over all modules, `emit` and `compile` over all units, `link` over outputs; `actions` and `fetch` rows appear only when relevant), a `cpu` occupancy strip (two cells per core) naming the running processes, and a `path` row with the critical path and an estimate of the time left. Snapshot of a release build at 1.6 s:

```
⠹ jet build shop                                               release · x86_64-linux · 1.6s
  check    ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━  42 modules · 3 rechecked                  0.18s
  emit     ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━  3 units emitted · 18 reused               0.02s
  compile  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╸╸╸  1 of 3 compiled · 18 reused                1.4s
  link     ────────────────────────────────  waiting on compile
  cpu      ████░░░░░░░░░░░░░░░░░░░░░░░░░░░░  2 of 16 busy · rustc shop/net · rustc shop/cli
  path     shop/net → link                                                    est. 2.5s left
```

Bars are two-tone over the whole population: gray cells are work the store already held (reused modules or units), red cells are work done this build, `╸` cells are in flight, dim `─` cells are pending; red-class cells are `ceil(n / total × 32)`, at least one when `n > 0`. The red sliver is the work Jet did; the counts say the same in words. While linking, the `link` row shows an 8-cell sweep because link has no measurable progress.

**Receipt.** On completion the board collapses to one line plus one hint; a no-change build is one line; a failed build keeps Jet's ordinary diagnostic format above its receipt:

```
✓ shop built in 4.1s · 3 of 21 units compiled, 18 reused · release · ./build/shop
  why: jet explain-build shop
✓ shop up to date · 38 ms · 21 units reused · release · ./build/shop
✗ shop not built · 1 error · 0.31s · nothing compiled, store unchanged
```

Notes print above the board or before the receipt (`note:` prefix): compiling the Jet runtime once for a new rustc, the store holding nothing for a new compiler, a store entry that failed verification and was rebuilt, a remote tier that was unreachable.

**Color law.** The board uses the Jet palette from `site/assets/site.css`: near-black ground, Jet red accent, ink and two grays, a green `✓`, and one cool instrument tone. Roles and their ANSI SGR on a 16-color terminal: brand red `1;31` for `jet`, compiled cells, busy cores, and the `$` prompt; bright red `91` for in-flight cells, the spinner, `✗`, `Error`, and carets; default foreground for names, counts, paths, and source; dim `2;37` for reused cells, stage labels, header meta, and elapsed; bright black `90` for pending cells, idle cores, gutters, and hints; green `32` for `✓` only; cyan `36` for `note:`, remote tier names, and URLs. Today's theme paints the `jet` accent bright cyan (`Theme::ACCENT_SGR = "1;96"`, `crates/jet-foundation/src/Terminal.rs:48`); accepting this board moves `ACCENT_SGR` to red so every CLI surface matches (card #2527). `NO_COLOR` removes color only, so reused and compiled cells share the glyph `━` and the counts carry the distinction; `--ascii` (or a non-UTF-8 locale) keeps the distinction in glyphs: `=` reused, `#` compiled or busy, `>` in flight, `.` pending or idle, `->` arrows, `-\|/` spinner:

```
/ jet build shop                                                 release, x86_64-linux, 1.6s
  check    =============================###  42 modules, 3 rechecked                   0.18s
  emit     ===========================#####  3 units emitted, 18 reused                0.02s
  compile  ===========================##>>>  1 of 3 compiled, 18 reused                 1.4s
  link     ................................  waiting on compile
  cpu      ####............................  2 of 16 busy, rustc shop/net, rustc shop/cli
  path     shop/net -> link                                                   est. 2.5s left
```

**Width rules.** 80–99 columns keep every row and shorten counts. Below 80 columns the `cpu` and `path` rows are dropped and bars are 16 cells:

```
⠹ jet build shop                          release · 1.6s
  check    ━━━━━━━━━━━━━━━━  42 mod · 3 rechecked  0.18s
  emit     ━━━━━━━━━━━━━━━━  3 units · 18 reused   0.02s
  compile  ━━━━━━━━━━━━━━╸╸  1 of 3 · 18 reused     1.4s
  link     ────────────────  waiting
```

The `path` row shows an estimate only when recorded durations exist for the remaining nodes, labeled `est.`; otherwise the right side is blank. Non-TTY output is one line per stage as it completes, then the receipt:

```
jet build shop (release, x86_64-linux)
check    42 modules, 3 rechecked                  0.18s
emit     3 units emitted, 18 reused               0.02s
compile  3 units compiled, 18 reused               2.2s
link     shop                                      1.7s
built shop in 4.1s -> ./build/shop (3 of 21 units compiled, 18 reused)
```

One renderer owns stderr: it handles resize and `SIGTSTP`/`SIGCONT`, restores the cursor on every exit path, and falls back to plain lines when cursor control is unsafe (`TERM=dumb`, consoles without VT support). `--json` emits NDJSON events (`stage`, `node`, `receipt`) that editors and CI consume and that the HTML report embeds.

**`jet explain-build <target|unit|file>`** prints why each node ran (the input that changed, by path and digest prefix), the critical path with per-node durations and a proportional strip of where the time went, store hits and misses per tier, and unit detail rows under their package:

```
$ jet explain-build shop
shop · release · x86_64-linux · built 4.1s ago · 3 of 21 units compiled · 18 reused

why
  shop/net       compiled    2.2s   src/net/client.jet changed (fetch_all body)
  shop/render    compiled    1.3s   src/render/table.jet changed (Row.width signature) → 1 dependent
  shop/cli       compiled    0.4s   depends on shop/render interface
  link shop      relinked    1.7s   3 units changed
  18 units       reused         –   store · local

critical path  4.1s   check 0.18 → emit 0.02 → rustc shop/net 2.2 → link 1.7
               ░░░░▓███████████████████████████████████████████▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒
store          local hits 18 · misses 3 · team off · 4.2 GiB of 20 GiB
```

`--why <node>` prints one node's full input diff. `--json` returns the graph record. `--html [path]` writes a self-contained page in the same palette: header strip, per-lane timeline with the critical path outlined and connected, package/unit graph, sortable why-table with keyboard navigation, store panel, and the reproducibility class of every node (D-BUILDPROBE1). Bars are colored by kind in a red ramp (check, emit, compile) with link in ink; reused work is a hollow faint tick; a unit restored from a remote tier is the instrument tone. `--open` opens it. The LSP reads the same record for hover provenance (ratified).

**Timing.** `JET_TIMING` and `jet-timing.json` are replaced by per-node durations in the graph record; the perf dashboard reads `jet explain-build --json`. The receipt/timing conflict of I.2 disappears because measuring no longer disables replay.

### II.12 Opt-in resident session

Owner choice: opt-in. D-JPK-NODAEMON1 forbids a resident process but names the `jet dev` session as the process that may parent work, and says a change needs a new ballot. The design uses exactly that session: `jet dev --serve` keeps the existing foreground dev process (`Source/CmdDevTools.rs:409-596`) and additionally listens on a per-user, per-workspace endpoint (a Unix-domain socket in a user-owned 0700 directory under `$XDG_RUNTIME_DIR/jet/`, or a private per-user directory when that variable is absent; a named pipe on Windows). A `jet build`, `jet run`, or `jet test` started in that workspace attaches, submits a serialized request envelope (argv, cwd, environment allowlist, terminal mode), and receives the same NDJSON events and exit status it would have produced itself. `--session=auto|off|required` controls attachment. The session authenticates the peer's OS identity, requires a per-session nonce stored under the workspace's `.jet/`, rejects symlinked endpoints, allows one owner per workspace, refuses clients of a different Jet or protocol version (they run fresh), resets request-scoped state per request, binds each request to one source snapshot, re-verifies leaf digests (it never trusts its watch stream alone), schedules deterministically, holds output-path locks, propagates Ctrl-C as cancellation, bounds its queue, and may compile units affected by the last edit before anyone asks. No background process survives the terminal; nothing runs as root; there is no `start`/`stop` verb. Outputs are byte-identical to the fresh-process path, proven by a differential across success, warning, error, cancellation, and concurrent-edit cases (III.3, case 26). The session ships only if the measured fresh-process overhead after memoized checks exceeds 100 ms or 20% of the median edit build on the corpus (ballot D-BUILD-SESSION1).

### II.13 Determinism prerequisites

Keys are only as sound as their inputs and outputs. Before widening reuse beyond one machine:

- rustc runs with `--remap-path-prefix <workdir>=/jet/build` and `-C metadata=<unit key>`; generated Rust carries project-relative source-map comments only (today's loader fallback can retain a host path for files outside the root: `crates/jet-driver/src/Loader.rs:2057-2085`, `:4658-4663`; that path is made project-relative or rejected).
- Every emission order is a stable order. The prior draft named four `HashMap`-order tie-breaks in ownership selection; they are verified and replaced by ordered maps as the first slice, with a test that two builds of the corpus are byte-identical (III.1, A8).
- D-BUILDPROBE1 reproducibility classes are recorded per node and shown in `explain-build`; an output that differs between two builds with identical keys is quarantined from sharing (D-JPK-REPROCACHE1) and reported as a defect.

### II.14 Code generation for compile speed

rustc's cost on generated Rust is proportional to what it must parse, resolve, type-check, and hand to LLVM. Levers, each measured on the corpus before adoption (III.4):

- **Volume.** The preserved Jetpack file is 6.1 MB with the runtime blocks inline; the thin user crate is what rustc compiles after the split, and units shrink it further per invocation. Emission removes dead helper imports per unit (today a "giant `use super::{...}`" import list: `crates/jet-codegen/src/Codegen/Context.rs:453`).
- **Macros.** `format!` for interpolated strings (`emit/helpers.rs:297-348`) becomes direct calls into a runtime builder; generated code otherwise stays macro-free, so rustc's expansion pass is near zero.
- **Derives.** Representation derives (`Debug`, `Clone`, `PartialEq`, …: `Items.rs:323-418`) are emitted only when the interface record proves a use.
- **Types.** Every generated local carries an explicit type where inference would otherwise run.
- **Lints.** `#![allow(warnings)]` stays; `--cap-lints allow` is passed so rustc can skip lint emission work.
- **Instances.** Phase 2 of II.4 compiles each generic instantiation once per program.

### II.15 Clean cutover

Greenfield law: each slice migrates every consumer and deletes what it replaces in the same change. No compatibility flags, no fallback readers, no parallel caches.

| Today | Becomes | Deleted in the same slice |
|---|---|---|
| `Source/BuildCache.rs` | `Link` node output in the store | `BuildCache.rs`, `JET_CACHE_DIR` |
| `Source/RuntimeCache.rs` | prebuilt runtime unit nodes in the store | `RuntimeCache.rs`, `JET_RUNTIME_CACHE_DIR`, `JET_RUNTIME_CACHE_STATS`, the 512 MiB FIFO |
| `Source/RunCache.rs` | per-unit `Object` artifacts | `RunCache.rs`, `JET_RUN_CACHE_DIR`, `JET_RUN_TRACE` (folded into `--json`/`explain-build`) |
| `Source/ReceiptStore.rs` | `Receipt` node in the store | `ReceiptStore.rs`, `JET_RECEIPT_DIR`, `JET_RECEIPT_BYPASS` (use `--no-cache`) |
| `.jet/build-cache/{cas,actions,package-artifacts}` | the store | project-local CAS; `.sealed` receipts replaced by real sealed unit sets |
| `IncrementalSemaCache` in memory, Check-only | persisted `Check` records, all modes | the `CompileMode::Check` gate on incrementality |
| `BuildPlanReplay` (compiler side) | nothing in the compiler build path | none: the codec stays for its jetpack provider/store callers; the cleanup card confirms no compiler use remains |
| `PhaseTiming` + `JET_TIMING*` | per-node durations in the graph record | `PhaseTiming.rs` JSON writers, `JET_TIMING`, `JET_TIMING_DIR`, `JET_TIMING_SOURCE` |
| inline-monolith retry on split rejection | ICE | `CmdCompile.rs:8252-8309` retry |
| `BuildProgress` stage lines | live board / non-TTY lines / `--json` | old stage strings and their snapshots |
| one flattened crate | units per package | `emit_bundle_dbg_inner`'s single-string assembly for native |
| `tools/perf/dashboard.sh` phase parsing | reads `explain-build --json` | `jet-timing.json` parsing |

Order of slices (each independently shippable; each deletes its predecessor): determinism prerequisites → the store with `BuildCache`/`RuntimeCache` cutover → persisted checks and receipts → compiler nodes in the graph and inspection → hidden units phase 1 → ThinLTO cache → fast lens → per-unit run artifacts → sealed unit sets with payload → prebuilt runtime → scheduler completion → board and HTML → resident session → benchmark harness → web through the graph → cleanup and docs.

### II.16 Production seams

- `crates/jet-comptime/src/Comptime/Build/` — `plan_graph.rs`, `plan_impl.rs`: compiler node kinds (`Check`, `Partition`, `Emit`, `Compile`, `Object`, `Link`, `Receipt`) join `BuildPlan`; `errors_keys.rs`: one key domain per node kind; `execution_runtime.rs`: pools, priority, RAM cap.
- New `crates/jet-store/` (std-only, path dependency, I6): CAS, action records, journal, bound, tiers adapter, ThinLTO cache handles. Replaces the storage halves of `BuildCache`, `RuntimeCache`, `RunCache`, `ReceiptStore`, and `cache_cas.rs`.
- `crates/jet-driver/src/QueryService.rs`, `crates/jet-sema/src/Sema/Bundle.rs`: interface and body record serialization; incrementality for every `CompileMode`.
- `crates/jet-codegen/src/Codegen/mod.rs`, `Imports.rs`, `TIR/emit/*`: per-unit emission (`iface`/`impl`), symbol naming, root-prefix remap to dependency crates, entry-unit globals; `TIR/mod.rs`: Phase 2 instance placement.
- `crates/jet-jit/src/jit/api_debug.rs`: `Object(unit)` executor from `try_compile_debug_aot`.
- `Source/CmdCompile.rs`: becomes graph submission plus rendering; `Source/NativeLinker.rs`: `rust-lld` for release with ThinLTO cache flags, mold for fast.
- `Source/CmdDevTools.rs`, `crates/jet-devserver/`: `--serve` socket and attach protocol.
- `Source/Cmd*` for `jet cache status|prune`, `jet explain-build --html`; `crates/jet-cli/src/CLI.rs` registry.
- `tools/perf/`: peer producer, state matrix, wall/CPU/RSS rows, dashboard cutover.
- `docs/spec/architecture.md`, `docs/plans/compiler-speed.md`, `docs/spec/syntax-decisions.md` ledger, `docs/spec/diagnostics.md`: updated in the cleanup slice.

### II.17 Self-hosting continuity

Everything above except `Compile(unit.*)` and the linker choice is backend-agnostic: the graph, identity ladder, store, units, scheduler, board, and inspection survive rustc's replacement unchanged. When the self-hosted optimizing backend arrives, `Compile` becomes `Object` for every profile and the ThinLTO cache becomes the backend's own per-module optimization cache. The interface/implementation split, Jet-owned symbols, and Jet-owned instantiation are exactly the properties that backend needs on day one.

---

## Part III — Proof still required

Nothing below exists. Each item names its observable evidence.

### III.1 Acceptance criteria

- **A1 — Dev loop.** `jet build --profile=debug` (Cranelift) and default `jet run` beat `cargo build` (dev) and `cargo run` on every corpus program in the cold, no-change, and every edit state: Jet/Cargo < 1.00 on wall time against out-of-box and tuned Cargo.
- **A2 — No-change.** `jet build` no-change completes in under 50 ms on the pinned machine for every corpus program, replays diagnostics byte-identical to the last real run, and beats the matched Cargo no-change row.
- **A3 — Edit-proportional release builds.** For every corpus program with more than one unit, a body-only edit in a leaf unit runs exactly one `Compile(unit.impl)`, zero dependent `Compile`s, one `Link`; wall time beats the matched `cargo build --release` edit row against out-of-box and tuned Cargo.
- **A4 — Cold optimized builds.** `jet build` and `jet build --release` cold (project artifacts absent, toolchain-level runtime objects present on both sides) beat `cargo build --release` on every corpus program including the smallest. This is the cell the design fights rather than owns; it gates the epic and stays open until measured.
- **A5 — Multi-package clean.** A clean checkout of a multi-package corpus program with a warm local store compiles only the root package and links the rest; wall time beats both Cargo columns.
- **A6 — Binary performance unchanged.** Every gauntlet runtime cell holds its current ratio within noise after hidden units and ThinLTO-at-link; binary size within 2% of today.
- **A7 — Parity.** Every hostile case (III.3) produces the same program output and diagnostics on AOT release, AOT debug (Cranelift), default `jet run`, and `jet run --interpret` (R12, I9).
- **A8 — Determinism.** Two builds of every corpus program from identical inputs on two different checkout paths produce byte-identical binaries and store records.
- **A9 — Boundary invariance.** Warm, cold, and `--no-cache` builds produce byte-identical manifest facts, lock contents, authority and policy decisions, diagnostics, outputs, and `-p`/`--affected` selections.
- **A10 — Integrity.** Every corruption case in III.3 rebuilds correctly with one `note:` line and no wrong output; a fuzzed store never produces a binary whose digest differs from a clean build's.
- **A11 — One store.** After cutover, `rg` finds no reference to `BuildCache`, `RuntimeCache`, `RunCache`, `ReceiptStore`, `JET_TIMING`, `JET_RUN_TRACE`, `JET_RECEIPT_BYPASS`, or `.jet/build-cache` in `Source/`, `crates/`, `tools/`, or `docs/spec/`, and `BuildPlanReplay` is referenced only from `crates/jetpack/`; `~/.cache/jet/{build,runtime,run}` are never created.
- **A12 — UI.** The live board, receipt lines, notes, non-TTY, `NO_COLOR`, narrow, and `--json` variants are snapshot-tested across the archetype × width matrix; the owner has visually accepted the board and the HTML report from the mockups or their implementation.
- **A13 — Resident session parity.** Every corpus program built through `jet dev --serve` attachment and through a fresh process yields byte-identical outputs and NDJSON event streams modulo timestamps.

### III.2 Matched Cargo benchmark method

**Comparison boundary.** User command to verified artifact, wall clock. Jet's total always includes its rustc, ThinLTO, and link time. No row subtracts Jet's backend cost or compares Jet's front end alone.

**Peers.** Two columns, both required (owner decision):

- *out-of-box:* `cargo build` / `cargo build --release` with a rustup-installed stable toolchain pinned to the same rustc Jet uses (1.97.1 today), default profile, default linker (`rust-lld` on x86_64 Linux), no `.cargo/config.toml`.
- *tuned:* per profile and target, the fastest valid pre-registered Cargo configuration, published with every config file and version: mold configured as linker, `sccache` as `RUSTC_WRAPPER` with a warm cache where the state permits, `RUSTFLAGS=-C target-cpu=native` (matching Jet's host native profile), and for the dev column a nightly toolchain of the same date with `-Zthreads=8` and `-Zcodegen-backend=cranelift`, recorded as a separate exact toolchain identity (it is a different compiler build, not the pinned stable rustc). A tuned configuration that fails output or runtime parity is rejected.

**Profile mapping.** `jet build --profile=debug` ↔ `cargo build`; `jet build --release` ↔ `cargo build --release`; `jet build` (default: opt-level 2, thin LTO, strip) is reported against `cargo build --release` as the "what users type" row and must also win.

**States** (each with the exact recorded patch on a copied tree, D-PERFBUDGET-COMPILE1): `cold-machine` (all caches empty on both sides, including sccache and the store; toolchain-provided objects present: Rust `std`, Jet runtime units), `clean-project-warm-machine` (project artifacts removed; machine caches warm; for out-of-box Cargo this equals cold and is recorded as such), `no-change`, `edit-body-leaf`, `edit-body-hot` (a module imported by many), `edit-signature`, `edit-dependency-body`, `add-module`, `comment-only`, `reformat`.

**Metrics.** Wall time is the gate. CPU seconds, peak RSS, binary size, and the binary's runtime cells are recorded and reported per row. One warmup, twenty samples per cell (ratified); median gates, interquartile spread and Tukey outliers as today.

**Corpus.** Real programs gate: every gauntlet Jet/Rust pair, the Jetpack package once its Rust twin exists, and the Tower port when it exists (D-MEGAPROJ1). Synthetic twins generated from one seed at 10k, 50k, and 200k lines and 1, 10, and 100 modules, in both languages from the same shape, are published as scaling curves and never gate. The twin generator is reviewed for bias toward Jet's strengths before its first publication.

**Comparator (ballot D-BUILDBENCH1, recommended option E).** Runs are paired and randomized; a cell is a **win** only when the upper bound of a bootstrap confidence interval on Jet/peer is below 1.00 against both peer columns (owner decision; the 1.05 Rust band does not apply to build cells). A cell whose interval straddles 1.00 is inconclusive and keeps the gate open. CPU seconds, peak RSS, and binary size carry independent non-regression ceilings; runtime cells of the produced binary must hold. A cell without a valid peer row, with a mismatched input identity, or with a failed parity check is unavailable and fails the gate; no averaging. Matched pairs record source lines, module and package graph, dependency closure, enabled features, output identity, and the exact edit patch, reviewed before a pair enters the corpus. Claims are made per measured host/target class; unmeasured targets are unavailable, not assumed.

**Producer.** An in-repo peer producer under `tools/perf/` writes the ratified peer report format (`JET_PERF_PEER_REPORT`), runs on the same machine and target identity as Jet, and is itself snapshot-tested; today only synthetic fixture rows exist (`tools/perf/test-ci-perf-check.sh:86-108`).

### III.3 Hostile invalidation matrix

Each case is a test with an exact expected outcome (hit set, miss set, diagnostics, output) on every tier (A7).

1. Comment-only edit → one `Check` re-runs; body digest unchanged; no `Emit`/`Compile`/`Link`; receipt line says up to date except the check count.
2. Reformat (whitespace only) → as 1 (D-BUILDNORM1).
3. Body edit, non-generic fn, leaf unit → `Emit(unit)`, `Compile(unit.impl)`, `Link`; zero dependent `Compile`s; ThinLTO re-optimizes only that module and its importers.
4. Body edit, generic fn → `Compile(unit.iface)` and every unit that instantiates it (Phase 1) / only the units owning instances (Phase 2).
5. Signature change → `Compile(unit.iface)` and every importing unit; non-importing units untouched.
6. Add a module → `Partition` changes; only units whose membership changed re-key; others hit.
7. Delete a module → as 6; stale artifacts remain in the store until pruned, never linked.
8. Trait impl moved between modules → impl-placement edge changes; only affected units re-key.
9. Dependency version bump → that package's sealed set restores or compiles; root units that import changed interfaces re-run rustc; others hit.
10. rustc upgrade → every `Compile`/`Object`/`Link` misses; every `Check`/`Emit` hits; one `note:` line.
11. Jet upgrade → every node misses; one `note:` line (D-LIB-REUSE1).
12. Profile switch `debug` ↔ `release` and back → both artifact sets remain valid; no thrash.
13. Target switch → as 12.
14. `PATH` or unrelated environment change → no miss; `RUSTC_LINKER` change → `Link` misses only.
15. `touch` without content change → all hits (stamp differs, hash equal).
16. `cp -p` a different file of equal length over a source → `Check` misses (ctime differs; hash differs).
17. Bit flip in a stored blob → digest mismatch → miss, removal, `note:`, correct rebuild.
18. Truncated action record → version/length check fails → miss.
19. Two concurrent builds of one project → identical outputs; at most one rustc per unit when the in-flight lock is honored.
20. Two concurrent builds of different projects sharing a dependency → one sealed set in the store.
21. Disk full during publish → tool error; no partial entry; next build succeeds after `jet cache prune`.
22. Project moved to another path → all hits (no absolute paths in keys or outputs).
23. Generated module from `b.action` changes → dependent `Check` misses via the declared output digest.
24. Remote tier returns wrong bytes or bad signature → quarantine, local compile, `note:`.
25. `--no-cache` → every executor runs; outputs byte-identical to the cached build.
26. Resident session vs fresh process → identical outputs and events (A13); session with a stale watch stream re-verifies digests and still builds correctly.
27. Cranelift-unsupported target → release executors at opt-level 0; `explain-build` names the fallback.
28. Store cap reached → LRU eviction; the current build's inputs are never evicted mid-build.
29. Package with an import cycle attempt → `E0604` before any node runs (unchanged).
30. Package with `policy.harden: true` → every crate of that package carries the required attributes; fences unchanged (A9).

### III.4 Risks and kill criteria

- **Per-unit fixed cost.** If a 200-module package spends more than 25% of a cold build in rustc process start-up and metadata loading, adopt structural coalescing by directory (II.4) before widening.
- **Duplicate monomorphization.** If Phase 1 shows more than 15% of LLVM time in duplicated instances across units, land Phase 2.
- **ThinLTO cache misses.** If a leaf body edit re-optimizes more than the changed module plus its direct importers, the import summary policy is tuned (`-import-instr-limit`) before A3 is claimed.
- **Default profile cost in the cold cell.** If `jet build`'s thin LTO costs more than the margin in A4 for small programs, propose a profile adjustment by ballot (D-BUILDPROFILE1 territory); never de-optimize silently.
- **`unsafe extern` reading of I1.** If D-BUILD-UNITS1 is declined, single-crate units remain; A3 weakens to "transitive dependents recompile in parallel", and the claim is re-measured.
- **Prebuilt runtime without a public tier.** Until the tier exists, A4's "toolchain objects present" means "compiled once on this machine"; the benchmark records which.

### III.5 Owner gates (ballots on the board)

| Ballot | Question | Recommendation |
|---|---|---|
| D-BUILD-UNITS1 | hidden units: two-crate units with compiler-declared `unsafe extern { safe fn }` as vetted internals under I1, single-crate units, or none | A: two-crate units |
| D-BUILD-NOCHANGE1 | memoized module checks satisfy the no-skip clause; byte-identical replay | A: yes |
| D-BUILD-SESSION1 | opt-in resident session as `jet dev --serve`, a background `jet daemon`, or none; amends D-JPK-NODAEMON1's scope | A: `jet dev --serve` |
| D-BUILD-STORE1 | one store; adaptive cap `min(20 GiB, 10%)` with a free-space reserve, pre-write admission, live leases, and host override; `jet cache status|prune`; `.jet/` shrink | E: adaptive with reserve, leases, and override |
| D-BUILD-PREBUILT1 | prebuilt runtime/Core signed by the project; third-party objects under registry-authorized builder roots; family-baseline ISA for shared objects; seam now, operation later | D: federated roots |
| D-BUILDBENCH1 | matched Cargo method: strict win against both columns on wall clock with confidence bounds, resource ceilings, real-program gate, synthetic curves | E: strict with confidence bounds |
| D-BUILD-UI1 | live board + receipt + `explain-build --html` with the mockups as the accepted look | A: board + HTML |

### III.6 What remains unverified in this document

- The HTML mockups were checked by exact text comparison with this document's frames (the terminal page re-checks its frames on load and exposes `data-frames-ok`) and rendered in headless Chromium 151 from the full devshell (`scripts/agent/jet-env full chromium --headless=new --screenshot …`); screenshots are session evidence under `~/.cache/jet-luna/abo/shots/`. Owner visual acceptance remains the look-and-feel check.
- The `HashMap` ordering claims of the prior draft were not re-verified line by line; slice 1 verifies and fixes them.
- Cargo peer rows (I.4a) come from the `CargoPeer` lane; the front-end numbers in I.4 are from a debug build of the compiler and are upper bounds.

### III.7 Card slate (on the board, 2026-09-01)

Parent epic **#2514** — *Automatic build optimization: one graph, hidden units, one store* (epoch e11, milestone e11-m02 Fast compiler loop; refs this document). Children, in dependency order; each carries actual/expected behavior, exact paths, non-goals, invariants, criteria, and a proof command on the board. Ballots live on the card they gate: D-BUILD-UNITS1 (#2519), D-BUILD-NOCHANGE1 (#2517), D-BUILD-SESSION1 (#2529), D-BUILD-STORE1 (#2516), D-BUILD-PREBUILT1 (#2525), D-BUILDBENCH1 (#2530), D-BUILD-UI1 (#2527).

1. **#2515** Determinism prerequisites (remap, metadata, ordered maps, two-build identity test) — no gate.
2. **#2516** `jet-store` and `BuildCache`/`RuntimeCache` cutover — after D-BUILD-STORE1.
3. **#2517** Persisted module checks for every `CompileMode`; `Receipt` node; no-change replay — after D-BUILD-NOCHANGE1 (absorbs #1026's incremental batch sema).
4. **#2518** Compiler work as graph nodes; `graph`/`query build`/`explain-build` read them; `PhaseTiming` deleted; the compiler's use of `BuildPlanReplay` (none) confirmed; dashboard reads `--json`.
5. **#2519** Hidden units, phase 1 — after D-BUILD-UNITS1.
6. **#2520** Hidden units, phase 2 (Jet-owned instantiation) — measurement-gated.
7. **#2521** Sealed unit sets with payload (retargets the receipt-only result of #1422).
8. **#2522** Release lens ThinLTO with store cache; gauntlet non-regression.
9. **#2523** Fast lens wiring (Cranelift objects + prebuilt runtime link) — absorbs #1028.
10. **#2524** Per-unit run artifacts; `RunCache` deleted.
11. **#2525** Prebuilt runtime/Core units and the public-tier seam — after D-BUILD-PREBUILT1 (absorbs #1025).
12. **#2526** Scheduler completion for compiler nodes (pools, priority, RAM cap, keep-going, stable diagnostics).
13. **#2527** Live board, receipt, notes, `--json`, `--trace` — after D-BUILD-UI1 and owner visual acceptance.
14. **#2528** `explain-build` why/critical-path/store panel and `--html` — after D-BUILD-UI1 and owner visual acceptance.
15. **#2529** Resident session `jet dev --serve` — after D-BUILD-SESSION1 and its adoption measurement.
16. **#2530** Matched Cargo benchmark harness and peer producer; baseline regeneration — after D-BUILDBENCH1 (absorbs the harness half of #2345; #666 closeout depends on it).
17. **#2531** Hostile invalidation matrix as a cross-tier test suite.
18. **#2532** Web builds through the graph.
19. **#2533** Generated-code compile-cost levers (II.14), measured.
20. **#2534** Cutover cleanup and documentation.
21. **#2535** Defect (sidequest, P0): Jetpack package `jet build` ICE on master (I.5); blocks the real-program corpus.
22. **#2536** Defect (sidequest, P1): `jet build` receipts never replay because the receipt context differs between identical invocations (I.5); the store's `Receipt` node replaces the mechanism, and this card records the cause so the replacement's test covers it.

Existing cards updated with absorption notes and new blockers: #1026, #1028, #1025, #2345, #1422, #666.

---

## Appendix A — Owner decisions, 2026-09-01 (chat, verbatim)

- board_writes: "Write cards and ballots to Tower now"
- win_definition: "Strict win against both unless you can prove it is literally impossible to do so while we still transpile to rust"
- hidden_units: "Yes, automatic hidden units"
- no_change_law: "Yes, memoized module checks count"
- daemon: "Opt-in daemon"
- store_cap: "Adaptive cap: min(20 GiB, 10% of disk), LRU prune"
- public_mirror: "Yes: design the seam now, operate it when infra exists"
- ui_direction: "Rich live status board + HTML report"
- bench_corpus: "Real programs gate; synthetic twins as scaling curves"

Earlier in the same session: "Documentation is likely old. Do not rely on its accuracy unless it provides a philosophical position." and "YOUR GOAL IS TO BEAT RUST while transpiling to rust. We will try to beat rust even more when we transition to self hosting."

## Appendix B — Evidence files (session, not repository)

`~/.cache/jet-luna/abo/`: `PipelineFacts` (inline in agent output), `PackageFacts` (inline), `TowerFacts.md`, `LawSheet.md`, `BenchFacts.md`, `UIFacts.md`, `GapFacts.md`, `GapFacts2.md`, `MeasureNow.md`, `CargoPeer.md`, `JetpackIce.md`, `jetpack-ice-main.rs`, `ui-spec.md`.
