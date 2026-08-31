# Automatic build optimization

**Status:** proposal. This document replaces the prior draft in full. The prior draft was never committed; its claims were re-verified against the tree, Tower, and the specs, and several were stale or wrong. Corrections are recorded inline.
**Scope:** compiler, package manager, dependency graph, build actions, caches, code generation, and linking.
**Goal:** beat Rust and Cargo on matched clean and incremental workloads without sacrificing safety, diagnostics, determinism, or execution-tier parity (I9).
**Hard rule:** optimization never changes user-declared package, subpackage, or workspace boundaries. Declared authority, policy, visibility, outputs, and dependency edges are preserved exactly. Optimization works only inside those boundaries and across their declared graph. A large package gains fine-grained reuse without reorganization.

The document has three parts. Part I states verified current facts. Part II states the proposed design. Part III states the proof still required. Nothing in Part I is proposed. Nothing in Part II is claimed to exist.

---

## Part I — Current facts (verified 2026-08-30)

### I.1 The measured problem

The Jetpack canary is the best real-program evidence we have (`docs/audits/dogfood-jetpack-2026-08-28.md:25-38`, `dogfood/jetpack/METRICS.md:144-161`):

| Workload (3,064-LOC Jet package vs Rust jetpack) | Jet | Rust | Result |
|---|---:|---:|---|
| Cold build | 19.695 s | 121.942 s | Jet 6.19× faster |
| Comment-only warm rebuild (median of 5) | 8.096 s | 3.622 s | **Rust 2.24× faster** |
| Cold build peak RSS | 1,097,280 KiB | 2,819,324 KiB | Jet 2.57× smaller |
| Optimized binary | 1,377,432 B | 65,947,688 B | Jet 47.9× smaller |

The Rust source envelope is conservative (14,875 LOC with behavior beyond the canary phases), so the source ratio is directional. The build timings are direct measurements of matched commands.

The warm number is the important one. The warm rebuild reports a final-binary cache **hit** with `backend=0` and `link=0` (#2371, closed). The remaining 8.1 seconds are pure front-end re-execution: package entry discovery, the programmable-build front end (full parse and sema of the whole bundle), semantic-index program-value construction, and native key derivation, all before the cache gate (`docs/audits/dogfood-jetpack-usage-experience-2026-08-30.md:122-130`). Jet already wins cold builds and already caches the backend. Jet loses warm builds because the front end runs in full on every invocation.

Two more lived failures shape this proposal:

- Before #2371, the Jetpack warm rebuild took 21.3 s against an 18.2 s cold build (`dogfood/jetpack/METRICS.md:209`). That is Theo anti-goal 1 (`docs/plans/compiler-speed.md:223-225`) observed in our own tree: incremental worse than clean. The cause was a hidden FFI bridge missing from the cache key. Identity gaps do not degrade politely; they invert the cache.
- #2346 (phase: building) records `aot-release-no-change` observing cache hits 0 / misses 1 in the perf dashboard. The latest diagnosis on the card says the native keys are identical and the miss sits in the **receipt replay layer**; the dashboard currently works around it with `JET_RECEIPT_BYPASS=1`. The no-change fast path exists and is broken, which is worse than absent: it is unmeasured and untrusted.

### I.2 The pipeline

Ordinary native compilation is whole-program and batch: Loader lex/parse with a bounded 8-worker fan-out and deterministic source-order reassembly (`crates/jet-driver/src/Loader.rs:17-24,75-153`) → sequential sema, FFI prep, TIR lower (`crates/jet-driver/src/Driver/mod.rs:5240-5413`) → one flat generated Rust crate with modules as nested `mod` blocks (`crates/jet-codegen/src/Codegen/mod.rs:5144-5252`) → one `rustc` process for backend and link (`Source/CmdCompile.rs:7483-7555`; a split-runtime rejection can retry inline at `7551-7555`). There is no per-module backend action graph on this path.

Tier routing is ratified and implemented: `jet run` and `jet dev` use the fast profile and the JIT lens; `jet build` is optimized AOT (D-BUILD-DEFAULT1=B; `Source/main.rs:490-520`, `Source/CmdCompile.rs:1216-1281`). Linker selection is explicit `RUSTC_LINKER`/`CC` override, then host mold, then lld, then the system linker; cross targets use the system linker (`Source/NativeLinker.rs:96-165`). rustc rejection of generated code maps to an internal compiler error, never a user diagnostic (I2; `Source/CmdCompile.rs:7562-7596`).

The typed programmable build graph is real but separate: `BuildPlan`/`BuildGraph` with deterministic Kahn stages, `BTreeSet` ready order, and CPU/memory/linker/console/GPU pools (`crates/jet-comptime/src/Comptime/Build/plan_graph.rs:12-90`, `execution_helpers.rs:53-126`). It executes only when the package selects a `fn build` (`Driver/mod.rs:3001-3013,3295-3406`). An ordinary build runs the programmable-build front end for checking, then falls into the monolithic `build()` path (`Source/CmdCompile.rs:1800-1839,2234-2325`). Graph execution for ordinary compilation does not exist today.

### I.3 The reuse mechanisms that exist today

Ten build-relevant reuse mechanisms coexist, each with its own key derivation, storage, integrity rules, and failure behavior:

1. **`BuildCache`** — whole-program native binary at `~/.cache/jet/build/<key>/bin` with a `bin.sha256` sidecar, keyed on canonical AST bytes, toolchain, dependencies, runtime, and settings (`Source/BuildCache.rs:17-41,90-167,208-295`; key assembly `Source/CmdCompile.rs:5795-6002`). No size bound, no pruning. Two hit timings exist: plain `jet build` probes only **after** the full front end (`CmdCompile.rs:1910-1937`); AOT-profile `jet run` probes and executes the cached binary **before** the front end (`CmdCompile.rs:1695-1767`).
2. **`RuntimeCache`** — content-addressed `jet_runtime`/`jet_runtime_core` rlibs at `~/.cache/jet/runtime`; 512 MiB bound, digest sidecars, oldest-entry eviction, per-key locks, fail-open to inline code (`Source/RuntimeCache.rs:18-35,101-148,316-421,636-790`). Bypassed entirely when FFI is present (`CmdCompile.rs:7397-7423`).
3. **`RunCache` + JIT tier cache** — warm default-`jet run` Cranelift machine code (`module.bin`, format 5 in `crates/jet-jit/src/jit/tier_cache.rs:31-41`) at `~/.cache/jet/run`, keyed on compiler identity plus the absolute watched-path closure with content digests and mtime/length stamps (`Source/RunCache.rs:75-185,192-261`). A warm hit returns **before** the front end (`Source/Interpreter.rs:924-944`); a miss runs the full front end, lowering, and Cranelift compile (`:945-1065`). All-or-nothing over the whole watched closure. Structural decode validation but no cryptographic sidecar, no size bound, no pruning.
4. **`jet-queries` + `IncrementalSemaCache`** — in-process demand cache with module-interface fingerprints, reverse-import-closure invalidation, and a checked-body cache (`crates/jet-queries/src/lib.rs:60-287`, `crates/jet-sema/src/Sema/Bundle.rs:738-933`, `Validation.rs:481-624`, `crates/jet-driver/src/QueryService.rs:274-395`). Wired into LSP, `jet try`, default `jet check` (`Source/lib.rs:250-251`), `lint --a11y/--complexity`, `prove`, and `supply`. **Not** wired into `jet build`, default `jet run`, or `jet dev`. In-memory only; nothing persists across processes.
5. **Programmable action layer** — canonical action keys (`jet.action-key.v2`, length-prefixed SHA-256 over content snapshots, no mtimes; `errors_keys.rs:253-423`; argv/input/output order is caller-defined and preserved), `LocalCas` and action records under `.jet/build-cache/{cas,actions,explanations}` (`execution_runtime.rs:465-477,1588-1598`), a `FrontEndCompletion` gate that forbids cache restore before parser, sema, policy, and diagnostics complete (`cache_cas.rs:177-211`), sandboxed execution, remote wire records and a remote scheduler (`cache_cas.rs:2872-3015`, `remote_scheduler.rs`), and a versioned deterministic `BuildPlanReplay` record (`replay.rs:1-61`).
6. **Compiler-owned `compile-package:<name>` actions** — minted per package when dependency roots exist (`plan_impl.rs:229-232,263-279,385-443`) and executed through the graph (`Driver/mod.rs:3231-3293`). The cached payload today is a `jet.sealed-package.v1` **text stamp** (name, source digest, compiler identity, target, profile, dependency digests; `Driver/mod.rs:3277-3292`). #1422 is done: the identity and action layer exist and are proven. The payload is not a compiled object, and nothing restores dependency compile work from it. The sidequest's line "sealed package-object reuse remains unbuilt" (`docs/sidequests/library-reuse-and-linking.md:7`) is true of the payload and false of the action layer; both facts matter.
7. **FFI bridge cache** — per-key Cargo-built bridge artifacts with provenance at `~/.cache/jet/ffi` (`crates/jet-pkg-model/src/FFI.rs:1993-2030,2619-2690,3246-3251`). The one place user builds invoke Cargo.
8. **C binding caches** — header-hash keyed generated bindings under project `.jet/bindings/<lang>/` with hash sidecars and auto-regeneration (`CBind.rs:16-49`, `CFFI.rs:1344-1467,1552-1588`).
9. **Package source stores, two layers** — Jetpack's Hangar under `$XDG_DATA_HOME/jet` (or `~/.local/share/jet`) with a shared CAS, strict metadata checks, lock-connected receipts, and a 128 GiB default quota with unreachable-object eviction (`crates/jet-pkg-model/src/Store.rs:19-70,164-230`, `crates/jetpack/src/Store/Quota.rs:20-97`, `Provider.rs:924-1001`); and the compiler Loader's legacy locked-source staging fallback at `~/.jet/store/<name>-<version>-<fingerprint>` with exact tree-hash verification (`crates/jet-driver/src/Loader.rs:2241-2305`). The generic `Lock::verify_store_fingerprint` helper is an incomplete stub (`crates/jet-pkg-model/src/Lock.rs:3731-3757`).
10. **`ReceiptStore`** — content-addressed whole-invocation records for `check`, `build`, `test`, `prove`, and `budget check` under project `.jet/receipts`, with input-closure validation on replay (`Source/ReceiptStore.rs:69-125,247-575`). `run` and `dev` are excluded. This is the layer #2346's diagnosis implicates.

Workspace membership is declared in `workspace.jet` and mirrored in `.jet/lock` (`crates/jet-pkg-model/src/WorkspacePlan.rs:19-58`, `Lock.rs:592-637`). The `workspace` key inside `package.jet` is reserved-empty (`Package/mod.rs:2168-2177`). Package facts parse name, version, deps, boundaries, members, outputs, settings, build profiles, build allowances, authority, and policy (`Package/mod.rs:175-236,2033-2165`).

### I.4 What a one-line edit costs today

- **`jet build`:** full front end, whole-crate TIR and Rust emission, whole-crate rustc, backend, link. Only runtime/Core rlibs, FFI bridges, C bindings, and declared-action records are reused. On a no-change build the front end still runs in full before the post-front-end cache probe.
- **Default `jet run`:** warm hit skips everything (pre-front-end); any change in the watched closure discards the whole artifact and pays full front end plus full JIT lowering and compilation.
- **`jet dev`:** full bundle reload and `check_bundle_gates` on every detected change (`Source/CmdDevTools.rs:846-896`); the incremental sema cache is not used. Dev's benefit is resident runtime state, not compiler-work elimination. The watcher trusts existence/mtime/length stamps with no content digest (`crates/jet-devserver/src/WatchService.rs:49-71`).
- **Web:** whole re-emission and a whole wasm crate per change (`Source/CmdCompile.rs:6163-6422`).

### I.5 Ratified law this proposal composes (not reopens)

Decision IDs verified against the live Tower store and its archive; archived records remain law.

- **Two-lens law:** one core, one TIR; the JIT lens wins dev velocity, the AOT lens ships optimized binaries (D-VERDICT-666-1, D-ONECORE1=A, D-VERDICT-687-1, D-LENS-RUN2=A, D-BUILD-DEFAULT1=B). Cranelift AOT is only the explicit debug profile (D-AOT-CRANELIFT1=B). R12 makes parity structural: one structured TIR, exhaustive consumers per backend, interpreter reference semantics, no durable unsupported gaps (`docs/spec/architecture.md:729-755`).
- **D-INCR-UNIT1=A:** the dirty model is three layers — item/query reuse, module-interface invalidation, sealed package artifacts. Package-only dirty sets punish large packages; file-only sets ignore module boundaries.
- **D-LIB-REUSE1=B**, both halves: sealed package objects keyed on exact identity (sources, dependency digests, compiler identity, target, profile), typed generic bodies traveling in the artifact, a compiler upgrade emptying the cache with a one-line message; and pinned Jet dynamic libraries with a checked compiler-identity match before mapping. "No cache path skips parsing, sema, policy, or diagnostics" and "a package restore skips recompiling that package; it never skips checking the package being edited" (`docs/sidequests/library-reuse-and-linking.md:69-76,134-142`). Restore serves every lens identically.
- **D-ECO-GRAPH1=A / D-ECO1=A:** one typed source-to-machine graph and one lock identity power run, test, build, and explain; outputs are thin projections.
- **The D-BUILD slate:** typed actions with declared inputs/outputs/argv/env/caps (D-BUILDACTION1=A); a default-on local action cache with full identity including tool, compiler, toolchain, target, policy, and generated hashes (D-BUILDCACHE1=A); deterministic scheduling with named pools (D-BUILDSCHED1=A); read-only inspection as `jet graph`, `jet query build`, and `jet explain-build` (D-BUILDQUERY1=A; these spellings, no `inspect` prefix); typed lock-recorded toolchains (D-BUILDTOOLCHAIN1=A); typed reproducibility-class probes (D-BUILDPROBE1=A); local by default with remote cache and remote execution as separate policy grants (D-BUILDREMOTE1=A); AST-level rename-sensitive normalization for cache identity (D-BUILDNORM1=A).
- **The D-JPK cache slate:** private namespaces and allowlisted signed writers (D-JPK-CACHEAUTH1=D); host-bound typed cache bindings, never repo or flags (D-JPK-CACHECONFIG1=D); verify-every-hit with quarantine of divergent outputs (D-JPK-REPROCACHE1=D); offline-first ordered mirrors with separate write grants (D-JPK-REMOTE1=D); signed output-hash substitution (D-JPK-CACHE1=A); sealed verification manifests with explicit full rehash (D-JPK-VERIFYONCE1=A); bounded typed dynamic plan stages with fragment digests in action identity and the lock (D-JPK-DYNAMICPLAN1=D); typed store endpoint capabilities (D-JPK-STOREBACKEND1=D); `-p <member>` and `--affected[-since]` derived from action-cache input hashes with the dependent closure always included (D-JPK-SELECTOR1=C). D-JPK-NIXCACHE1 is an **open** ballot, not law.
- **Content-addressed package identity** (D-CASTORE1=A) and the shared verified generated-Rust test cache (D-VERIFY-CACHE1=C).
- **R8:** verified warm object reuse; corrupt, rejected, or malformed artifacts fall back to complete inline compilation; caches are bounded; the final key carries relevant runtime/Core digests, not a compiler-binary hash (`docs/spec/architecture.md:665-679`). R2: any sema-pass program must produce compiling Rust (`:641-643`).
- **Comptime effects:** D-CTEFFECT1=A (Tier 0 pure; Tier 1 hashed into `.jet/lock`; Tier 2 ambient behind `#Impure` and a gate) and D-MODCOMPUTE1=A (pure computed-field graphs, deterministic order). D-DET1 is an **imported open record**, not a ratified decision; its operative text lives in `docs/spec/syntax-decisions.md`.
- **Measurement law:** D-PERFBUDGET-COMPILE1=C (typed Clean/NoChange/named-Edit compile workloads, exact recorded patch on a copied tree, one warmup, twenty samples; `docs/spec/performance-budget-decisions.md:25-40`) plus the ratified perf-budget slate (surfaces, baselines, grammar, reports, providers, integration). D-COSTLAW1=A is a standing law with a loop lint and a per-line cost view, not just a CLI flag. Benchmarks use `.measure`/`jet test --measure` with tier labels and the `keep(x)` sink (D-BENCH-MARKER1=A, D-CLAIM-BENCH1=A, D-BENCH-KEEP1=A). Script warm budget: ≤ 2× the fastest peer (D-SCRIPT-BUDGET1=B; #741 done).
- **Theo/Xcode anti-goals** (`docs/plans/compiler-speed.md:216-237`): incremental never worse than clean; no world rebuilds for a local edit; bounded pinpoint type-checker diagnostics (D-TYPECHECK-BOUND1=A, ratified); no cache-purge folk remedies; dev profile never silently diverges from ship profile.
- **Honest physics** (`docs/plans/compiler-speed.md:273-280`): the dev loop can beat Cargo by large multiples because it does strictly less work. Transpile-era optimized AOT is Cargo-release parity at best on cold single-package builds. Claiming otherwise is dishonest.

### I.6 Known defects and determinism holes

- **#2346** (building): the no-change AOT miss, diagnosed to the receipt replay layer; the dashboard bypasses receipts. **#2345** (ready, blocked by #2346): perf-harness acceptance and a baseline never regenerated. #666, #1023, #1026, #1027, #1028 wait on them; #1025 (reusable stdlib objects) is ready with a stale blocker pointing at completed #1024.
- **Nondeterministic ownership tie-breaks:** `dep_roots` is a `HashMap` (`crates/jet-foundation/src/AST/program_imports.rs:618-620`), and three sites select owners by `.max_by_key` over its iteration order when depths tie: `crates/jet-codegen/src/Codegen/Context.rs:3499-3517`, `crates/jet-sema/src/Sema/BudgetSpecs.rs:164-175`, and `crates/jet-driver/src/Loader.rs:2599-2610` (with `realized_authorities: HashMap` at `Loader.rs:325-327`).
- **Ambient environment:** the cache env recorded for the final rustc invocation is empty while the spawned process inherits the full ambient environment (`Source/CmdCompile.rs:7409-7413,7503-7517`).
- **Paths in artifacts and keys:** the native generated-source comment embeds `module.display`, which can be absolute (`Codegen/mod.rs:5159-5165`); web source maps are project-relative by design (`Web.rs:25-30`). `RunCache` hashes absolute watched paths (`RunCache.rs:148-185`).
- **Watcher stamps:** existence, mtime, and length only; a same-size mtime-preserving rewrite is invisible to the dev watcher (`WatchService.rs:49-71`).
- **Store verification split:** the Loader verifies exact tree hashes itself; the generic `Lock::verify_store_fingerprint` is a stub. Two source-store layers (Hangar, legacy `~/.jet/store`) coexist.
- **Nixpkgs cold path:** the native store audit measured 0/28 `env.jet`, 0/22 direct-shell, and 0/48 full-shell selections on a cold no-Nix machine (`docs/audits/jetpack-native-nixpkgs-2026-08-24.md`). No part of this proposal may assume that support exists.

---

## Part II — Proposed design

### II.1 The thesis

The measured enemy is unconditional front-end re-execution, not missing backend caches. Jet already wins cold builds by 6× and already skips warm backend and link work. It loses the warm race because every invocation replays the whole front end before any cache gate, and its ten reuse mechanisms each carry private keys, private stores, and private failure rules.

The design is four laws:

1. **One graph.** Every invocation lowers to the existing typed `BuildPlan`. Ordinary compilation becomes compiler-owned nodes in the same graph that runs user `fn build` actions. D-ECO-GRAPH1=A already demands this; the work is promotion and deletion, not invention.
2. **One identity.** Every durable artifact is keyed by the canonical action-key discipline, on an identity ladder from declared boundary facts down to per-item fingerprints.
3. **No-change is an invocation-level replay.** When the complete input closure is digest-identical, the invocation replays its receipt: recorded diagnostics, verified terminal artifact, exit. Target: under 100 ms.
4. **An edit pays for its dirty set.** The front end re-checks the changed module and its dependents through the already-built incremental sema engine; engines rebuild only dirty fragments. Work is proportional to the edit, in every lens.

### II.2 The canonical work graph

Every `jet build`, `jet run`, `jet dev`, `jet test`, and web build lowers to one `BuildPlan`, whether or not the package declares `fn build`. Node kinds, all expressed as today's typed actions:

| Node | Today | Becomes |
|---|---|---|
| Package front end (parse, sema, TIR per package) | implicit driver phases | compiler-owned node; internally demand-driven via `jet-queries` |
| Sealed package object (dependency compile) | `compile-package:<name>` action with a v1 stamp payload | real object payload; restore from local, team, and public tiers |
| Runtime/Core rlib compile | standalone `RuntimeCache` | compiler-owned action; same key discipline; rlib split mechanics and 512 MiB bound retained |
| FFI bridge, C binding | standalone caches | compiler-owned actions (keys already compatible) |
| Generated-crate emit, rustc, link (AOT lens) | monolithic `build()` | terminal actions; linker keeps its pool of 1 |
| JIT module artifact (JIT lens) | all-or-nothing `RunCache` | per-module tier-artifact node |
| User actions, generated sources | already actions | unchanged |
| Final binary, web artifact set | `BuildCache`, ad-hoc writes | terminal action outputs in the action-record store |

The scheduler is the existing deterministic one: Kahn topological stages, `BTreeSet` ready order, automatic CPU/memory pools, serial linker/console/GPU pools (`execution_helpers.rs:53-126`). No second scheduler. The lens changes which terminal nodes are demanded, never which dependency work is reusable (D-LIB-REUSE1, I9). The Loader fan-out cap becomes `min(available_parallelism, configured jobs)`, recorded in receipts. Sema stays single-threaded per bundle; parallel sema is a self-host architecture bet owned by #669. Independent package front-end nodes parallelize as ordinary graph stages.

**What is deleted, and by what.** `BuildCache` → terminal-action records plus CAS. `RuntimeCache`'s private store logic → compiler-owned actions (its mechanics survive as the action implementation). `RunCache`'s bespoke store → per-module tier-artifact nodes with standard integrity and bounds. The two ad-hoc pre-front-end probes (AOT-profile `jet run` binary probe, `RunCache` warm hit) → the one receipt-replay discipline of II.4. `ReceiptStore` stays: it answers the invocation-level question, and its input closure derives from graph node snapshots instead of a parallel walk. The Hangar package store stays: it is the immutable source-input layer, not an output cache. The Loader's legacy `~/.jet/store` staging folds into the Hangar layer, and `Lock::verify_store_fingerprint` is completed or deleted in the same change. Nothing new is layered beside anything.

### II.3 Identity and invalidation

One key discipline for all durable artifacts: `jet.action-key.v2` (`errors_keys.rs:253-423`). Canonical serialization, length-prefixed SHA-256, content snapshots, declared inputs, no ambient state, no mtimes. Sequence order is preserved where order is semantic (argv, declared outputs); unordered sets are serialized sorted. The identity ladder, coarse to fine:

1. **Boundary identity** — the declared facts: `package.jet` facts as parsed (name, version, deps, boundaries, members, outputs, authority, policy, settings, build profiles), `workspace.jet` membership, `.jet/lock` including Tier-1 comptime input hashes (D-CTEFFECT1) and dynamic-plan fragment digests (D-JPK-DYNAMICPLAN1). Changing a declared fact invalidates everything it governs. Nothing else may.
2. **Package identity** — canonical module bytes under D-BUILDNORM1 normalization, sorted relative module paths, sorted dependency artifact digests, compiler identity, target, profile, toolchain record (D-BUILDTOOLCHAIN1). This keys sealed objects.
3. **Module interface identity** — the existing interface fingerprint (`Sema/Bundle.rs:738-933`). An interface change dirties the reverse import closure; a body change dirties one module.
4. **Item identity** — the existing checked-body cache key (`Validation.rs:481-624`). Comptime-dependent bodies stay uncacheable, as today.
5. **Engine-partition identity** — content keys for emitted-Rust fragments and per-module JIT artifacts. These exist only inside engines and have no user-visible meaning.

Comment-only and formatting-only edits preserve identity at layers 2–5 because normalization excludes them (D-BUILDNORM1); the edited package still gets a real front end and fresh spans. Invalidation is exact, monotone, and boring: a changed input changes a key; a changed key is a miss; a miss rebuilds that node and re-demands dependents. No timestamp heuristics, no "probably fresh," and no invalidation that requires user action. `jet clean` as a remedy is a compiler bug (Theo anti-goal 4). The Jetpack FFI-bridge inversion (I.1) is the standing lesson: every engine input, including hidden bridges, must appear in the key, and the hostile matrix in Part III tests for the class, not the instance.

### II.4 The no-change law

A no-change invocation must not re-run the front end. Mechanism: the invocation computes its input-closure digest (sources, manifest, workspace, lock, toolchain, settings, environment allowlist) from graph node snapshots. If a stored receipt matches exactly, the invocation replays: recorded diagnostics byte-identical, terminal artifact digest re-verified, exit. Any digest difference disqualifies replay entirely, and the affected packages get a real front end.

This is composition, not new law. `ReceiptStore` already records and validates whole invocations for `check`, `build`, `test`, `prove`, and `budget check`. The shipped warm `jet run` path already returns before the front end on an exact-identity hit, and #741 closed with that behavior under the ratified D-SCRIPT-BUDGET1=B budget. The AOT-profile run probe does the same for cached binaries. What changes: the replay layer becomes the single product path for every verb in every lens, it is fixed (#2346 lives exactly there), and the two ad-hoc probes are deleted into it.

**One interpretive point is flagged rather than assumed.** D-LIB-REUSE1's clause "no cache path skips parsing, sema, policy, or diagnostics" governs artifact restore; its own beginner pass promises "the second build is fast" with no re-check, and the shipped run path already skips the front end on unchanged inputs. This proposal reads the clause as governing builds where something is being compiled, with digest-identical invocation replay as the no-change path that reproduces the recorded front-end verdict exactly. If the owner reads the clause strictly (every invocation re-runs sema), sub-second no-change builds are impossible and law wins; the target then falls back to the II.5 incremental front end (warm-process fingerprint hits, not replay). Confirm before slice 1 changes `jet build` semantics.

### II.5 Incrementality per lens

**Front end (all lenses).** Wire `CompilerQueries`/`IncrementalSemaCache` into `jet build`, default `jet run`, and `jet dev` — the same engine that already serves `check`, LSP, `try`, `prove`, and `supply` (D-INCR-UNIT1 layers 1–2). Persist module-interface fingerprints and the checked-body cache into `.jet/build-cache` so warm starts across processes skip unchanged-module sema. The `FrontEndCompletion` gate is unchanged: no artifact restore before parser, sema, policy, and diagnostics complete for the demanded set. The dev watcher adds content-digest confirmation; stamps stay as the cheap first filter.

**AOT lens (`jet build`).** In honesty order:

1. *No-change:* receipt replay (II.4). Cargo's no-change is also cheap; parity plus replayed diagnostics.
2. *Dependency work:* sealed package objects restored from local, team, and public tiers. This is the structural beat: Cargo recompiles every dependency per project and per clean checkout; Jet restores exact-identity artifacts machine- and team-wide. Clean builds of multi-package projects become "compile the root, link the rest." The #1422 action layer is the landing pad; the work is the real payload (typed TIR with generic bodies, interface, emitted fragment) and restore into emit and link.
3. *Current-package edit:* incremental front end on the dirty set plus per-module emitted-Rust fragment reuse, so emission cost is proportional to the edit. The final rustc invocation stays whole-crate in the transpile era; its cost is bounded by sealed dependencies (less input), cached rlibs, mold, and fast-profile flags. We do not pretend LLVM away: optimized single-package cold AOT targets a stated factor of `cargo build --release`, not a win. The AOT win claims live in rows 1–2 and in multi-package topologies.
4. Splitting the current package's generated Rust into multiple rustc crates is rejected for the transpile era: it multiplies I2 surface and link complexity for a backend the self-hosted compiler replaces.

**JIT lens (`jet run`, `jet dev`).** Replace the all-or-nothing `RunCache` with per-module tier artifacts: content-keyed Cranelift machine code per module (same format-5 payload, standard integrity sidecar, bound, pruning, root-relative keys). An edit re-lowers and recompiles only dirty modules; unchanged modules reload machine code. A `jet dev` iteration becomes: digest-confirm the dirty set → incremental sema on it → per-module JIT rebuild → resident swap. This is where "beats Cargo" is structural and large: the JIT lens does no optimization, no rustc, and no link, and after this change does work proportional to the edit.

**Interpreter and web.** The interpreter tier stays cache-free; deopt and ambient paths call the same Prelude semantics (I9), and replay never serves a tier the receipt did not record. Web builds route through the same graph (front-end nodes shared with native; wasm rustc and artifact writes become terminal actions), gaining the no-change short-circuit and front-end incrementality. Per-fragment wasm reuse waits for the web backend to stabilize.

### II.6 The package boundary law, mechanically

The optimizer's only units with user-visible meaning are the declared ones: package, module, item.

- Sealed-object granularity is exactly the declared package. The compiler never splits one declared package into artifacts with independent trust, authority, or visibility, and never merges packages into one artifact identity.
- Fine-grained reuse inside a large package uses ladder layers 3–5, which are invisible: no name, no manifest presence, no authority, no policy scope, no import semantics. `jet explain-build` may show them as detail rows under their owning package node; nothing else surfaces them.
- Dependency edges derive from resolved cross-package imports and declared `deps`, never from source-path coincidence, and are never rewritten for scheduling convenience. Workspace membership authority is `workspace.jet`, exactly as declared.
- `-p` and `--affected` selection semantics (D-JPK-SELECTOR1=C) are identical with optimization warm, cold, or disabled.
- Boundary regression proof: builds with optimization fully warm, fully cold, and disabled produce byte-identical manifest facts, lock contents, authority and policy decisions, diagnostics, outputs, and selection behavior.

### II.7 Cache integrity, trust, and failure behavior

One law, applied to every store (today it is applied piecemeal):

- **Verify on read.** A digest sidecar is checked on every hit. Sealed objects and remote hits additionally verify signatures and provenance (D-JPK-CACHEAUTH1, D-JPK-CACHE1), with sealed verification manifests and explicit full rehash (D-JPK-VERIFYONCE1).
- **Fail open, silently, once.** A corrupt, truncated, wrong-format, or rejected artifact is deleted and rebuilt inline (R8). Never a user diagnostic, never a stale result, never "try `jet clean`."
- **Bounded.** Every store has a byte bound and age-based pruning under lock. This fixes `BuildCache` and `RunCache`, which are unbounded today. `jet self doctor` reports every footprint.
- **Atomic.** Temp-write plus rename publication everywhere. A cancelled build (SIGINT mid-graph) publishes nothing; the next build finds a consistent store.
- **Concurrent.** Per-key build locks across all stores, as `RuntimeCache` and the FFI cache do today: two concurrent builds of one key produce one artifact and one waiter.
- **Compiler upgrade** empties artifact tiers keyed on compiler identity and prints one line: dependencies rebuild once (D-LIB-REUSE1).
- **Remote** is exactly the ratified D-JPK machinery: host-bound endpoints, ordered mirrors, first verifying hit wins, writes need a separate grant, unreproducible outputs are quarantined with downstream taint (D-JPK-REPROCACHE1), remote execution is a separate policy grant (D-BUILDREMOTE1), and offline beats every mirror. No new trust anything.
- **Diagnostics parity law.** For every cache state (cold, warm, corrupted-then-recovered, remote, replayed), diagnostics for the same source state are byte-identical to a fresh build's. This is testable and gates every reuse slice.

### II.8 Determinism prerequisites

Keys are only as sound as their inputs. Before widening reuse:

- Replace the three `HashMap`-iteration `.max_by_key` ownership tie-breaks (I.6) with total-order selection, plus an audit pass for the class.
- Run the final rustc with an explicit allowlisted environment; the allowlist enters the action key, closing the ambient-env hole at `CmdCompile.rs:7503`.
- Make native generated-source comments and provenance paths project-root-relative, as web maps already are; key the `RunCache` successor on root-relative paths so artifacts survive checkout moves and team-tier hits are honest.
- Add watcher content-digest confirmation (II.5).
- Complete or delete `Lock::verify_store_fingerprint`; one authoritative store-verification path.
- Standing check: two independent builds of the pinned corpus produce identical artifact digests, using the D-BUILDPROBE1 typed probes; the first differing path is named.

### II.9 Beginner defaults, expert inspection

**Beginner:** nothing to type, nothing to configure, nothing to learn. The first build compiles; the second build is fast; an upgrade prints one line; a broken cache heals itself. No cache flag, no clean step, no stale-artifact failure mode. This is the ratified beginner pass of D-LIB-REUSE1 applied to the whole graph.

**Expert (ratified surfaces only; this proposal adds no syntax and no new commands):**

- `jet graph`, `jet query build`, `jet explain-build <node>` (D-BUILDQUERY1=A spellings): the graph, its provenance, and per-node hit/miss reasons. The `.jet/build-cache/explanations` directory becomes the backing store.
- `jet explain --cost` stays the semantic-cost surface (D-COSTLAW1); build caching never hides a semantic cost row.
- `jet cache bind`: mirror order, roles, credentials (D-JPK-CACHECONFIG1).
- `jet self doctor`: every store footprint and bound.
- `JET_TIMING=1` phase receipts extend to per-node graph timing; `jet dev` iterations gain a per-phase breakdown instead of one elapsed number.

### II.10 Clean cutover

Greenfield law applies: each slice migrates every consumer and deletes the replaced mechanism in the same change. No compat flags, no fallback readers, no parallel caches. Order:

1. **Fix the replay layer.** Close #2346 where its diagnosis points (receipts), remove `JET_RECEIPT_BYPASS=1` from the dashboard, close #2345, regenerate the v4 baseline. No optimization claim before this. Requires the II.4 owner confirmation if replay semantics for `jet build` change.
2. **Determinism hardening** (II.8). Prerequisite for wider keying.
3. **Graph promotion:** ordinary compilation lowers to `BuildPlan`; final binary and web artifacts become terminal actions; `BuildCache` is deleted; the two ad-hoc pre-front-end probes are deleted into replay; explanations feed `jet explain-build`.
4. **Front-end incrementality:** queries wired into build/run/dev; fingerprints and the body cache persisted; watcher digests. #1026's canary already proves the batch dirty-set behavior this extends.
5. **Sealed package objects:** real payload and local-tier restore on the #1422 action layer; `RuntimeCache` store logic folded into compiler-owned actions; #1025's stdlib-object scope lands here as the first proven package.
6. **Per-module JIT artifacts:** `RunCache` successor with standard integrity and bounds; the old whole-closure store deleted.
7. **Team and public tiers:** sealed objects through the ratified mirror and trust machinery.
8. **Benchmark harness** (II.11): built alongside slices 3–6, reporting continuously.

Every slice lands with its hostile-invalidation tests (Part III) and the diagnostics-parity differential green on every applicable tier: parser → sema → TIR → AOT → JIT/dev → interpreter → web where applicable (I9). No slice closes with a `jit_gaps` entry.

### II.11 Matched Cargo benchmark method

**Comparison boundary.** User command to verified artifact. Jet's total always includes its rustc and link time; Jet pays for the backend it chose. A generated-Rust-versus-direct-rustc row may exist for engineering, but it is never a Jet-versus-Rust result.

**Harness.** Extend `tools/perf/dashboard.sh` with a Cargo command adapter, preserving the v4 receipt schema, cache-state checks, output-digest validation, phase receipts, and identity records; `ci-perf-check.sh` rejects missing Cargo-side identity exactly as it rejects missing Jet identity. Workload states follow D-PERFBUDGET-COMPILE1=C: Clean, NoChange, and named Edits as exact recorded patches on a copied tree, one warmup, twenty samples. Gauntlet's report-only win/parity/loss grading carries over.

**Matched workloads.** The same semantic program in both languages, specified first (inputs, algorithm, observable output, error paths, package topology, dependency set), not line-by-line translation. The Jetpack canary stays as the standing real-program row with its envelope caveat stated. Matrix:

- LOC tiers: small (~100–300), medium (~1k–3k), large (~10k–30k) authored LOC; a generated ~50–100k scale row reported separately.
- Topologies: single package, and a matched multi-package workspace where Jet packages map one-to-one to Cargo workspace crates by declaration. Never let one Jet package silently map to many crates or the reverse.
- States per row: clean; no-change; comment-only edit; private-body edit; public-interface edit; dependency edit; manifest or lock edit.

**Protocol.** One pinned machine (CPU, topology, governor recorded); pinned rustc, Cargo, and Jet identities; the same linker (mold) on both sides, recorded; the same explicit job count on both sides; disk-backed fresh target and build directories per temperature; sccache off or identically provisioned and reported; Cargo incremental state and fingerprint reuse reported per row; medians, IQR, Tukey outliers; wall time primary, CPU time and peak RSS recorded; samples interleaved between tools; outputs digest-verified before any timing is trusted. Lens mapping: `jet build` ↔ `cargo build --release --locked`; default `jet run` and `jet dev` ↔ `cargo run` dev profile. Interpreter and web rows are parity evidence, not speed rows.

**Reporting.** Every row is win, parity, or loss with full identity. No aggregate "Jet beats Cargo" claim without the complete matrix. A row that skipped a diagnostic, reused a stale artifact, or diverged in output is a failed row, not a fast one. The two standing targets from current evidence: erase the Jetpack 2.24× warm loss, and keep the 6.19× cold win while doing it.

---

## Part III — Proof still required

Nothing below exists yet. Each item names its observable evidence.

### III.1 Acceptance criteria

- **A1 (dev loop, the structural win).** Default `jet run` and `jet dev` clean and representative-edit medians beat the matched `cargo run` dev-profile rows at every LOC tier and both topologies.
- **A2 (no-change).** `jet build` no-change completes via receipt replay in under 100 ms on the pinned machine and is at or under the matched Cargo no-change row, with replayed diagnostics byte-identical. Requires #2346 closed and the receipt bypass removed from the dashboard.
- **A3 (warm edit, the Jetpack row).** The Jetpack-canary comment-only and private-body warm rows beat the measured Rust warm rebuild (3.622 s). Phase receipts prove unchanged-module sema hits and emission work proportional to the dirty set.
- **A4 (dependency reuse).** A multi-package clean build with a warm local sealed tier beats `cargo build --release` clean at medium and large tiers. Cold single-package clean `jet build` stays within a stated factor of Cargo release (target ≤ 1.5×, measured then recorded); this is a parity claim, not a win claim.
- **A5 (parity).** Zero R12 tier diffs on the golden suite; the dev-corpus gate green at its fixed denominator; diagnostics byte-identical between cached, replayed, and fresh builds in every cache state.
- **A6 (boundaries).** The II.6 regression proof: byte-identical declared facts, lock, authority and policy decisions, selection semantics, and outputs across optimization on, warm, cold, and disabled.
- **A7 (integrity).** The fault-injection rows of III.2 green, with no user diagnostic and no purge instruction ever emitted.
- **A8 (determinism).** Two independent builds of the pinned corpus produce identical artifact digests, D-BUILDPROBE1 probes green, and the three tie-break sites fixed with a regression test for the class.
- **A9 (footprint).** Every store bounded; `jet self doctor` reports each; no store grows without bound under a 1,000-build soak.

### III.2 Hostile invalidation matrix

Each case is a test with an exact expected outcome: hit set, miss set, diagnostics, output.

1. Comment-only edit — layers 2–5 identity preserved (D-BUILDNORM1); the edited package still gets a real front end; diagnostics carry fresh spans; terminal artifact reused.
2. Whitespace and formatting edit — same as 1.
3. Rename-only edit — identity changes (normalization is rename-sensitive by decision); dependents of the interface miss.
4. Private-body edit — one module re-checked; dependents hit; sealed dependencies hit; terminal miss.
5. Public-interface edit — reverse import closure re-checked; same-package artifacts miss; other packages hit.
6. Item reorder without interface change — module identity changes; dependents' interface fingerprints may still hit.
7. Tier-1 comptime input change (hashed file, find, fetch) — lock identity changes; dependent generated sources and their consumers miss (D-CTEFFECT1).
8. Dynamic-plan fragment change — fragment digest changes action identity and lock (D-JPK-DYNAMICPLAN1); offline replay stays deterministic.
9. Dependency version bump — that package's sealed object and dependents miss; unrelated packages hit.
10. Lock edit without manifest edit — fail closed per locked-mode rules; no reuse from a mismatched lock.
11. Policy tightening — governed artifacts miss; forbidden loosening is rejected before any build; capability widening via cache is impossible because keys include grants.
12. Target, profile, or linker change — full artifact-tier miss along the changed axis; no cross-profile bleed (Theo anti-goal 5).
13. Compiler upgrade — one-line message; one full rebuild; old artifacts unreachable and pruned.
14. Hidden engine input (the #2371 class) — an FFI bridge, C binding, or generated-source change misses every artifact it feeds; a warm build is never slower than its cold build on the same state.
15. Corrupted CAS blob, sealed object, tier artifact, or final binary — digest refuses; entry deleted; silent rebuild; correct output.
16. Truncated or wrong-format artifact — same as 15; strict format decode refuses.
17. mtime-preserving same-length edit — content digest catches it in both the watcher and the keys.
18. Ambient environment change (PATH, locale, RUSTFLAGS) — no key change unless allowlisted; an allowlisted change misses.
19. Remote cache serving wrong bytes or a bad signature — refused; next mirror or local build; provenance failure quarantines the writer's output (D-JPK-REPROCACHE1).
20. Unreproducible action output — untrusted namespace, downstream taint, no silent promotion.
21. Concurrent builds of one project — per-key locks serialize; both succeed; one compile.
22. Cancelled build (SIGINT mid-graph) — no partial publication; the next build finds a consistent store.
23. Receipt replay with any single input digest changed — replay refused entirely; real front end runs; no stale diagnostic survives.
24. Interpreter, JIT, and AOT cross-check after every case above — same output, same diagnostics (I9).

### III.3 Open points

- **The II.4 interpretive point** is the one owner call in this proposal: whether digest-identical invocation replay satisfies D-LIB-REUSE1's no-skip clause for `jet build`. Everything else composes ratified decisions and adds no syntax, no dependency, and no invariant carve-out.
- Three numbers are set by measurement and then recorded: the A4 cold-clean factor, the AOT-edit factor, and the store byte bounds.
- If measurement forces a change that touches ratified scope (for example, engine partitioning of the current package after all), that returns as a Tower ballot with the evidence attached.

### III.4 What this proposal explicitly rejects

- A new universal graph, store, or identity layer beside `jet-queries`, the action graph, and the receipt store. The seams are real, ratified, and load-bearing; unification happens by promotion and deletion, not by an eleventh mechanism.
- Splitting the current package's generated Rust into multiple rustc crates in the transpile era: I2 surface and link complexity for a temporary backend.
- Package-boundary inference, splitting, or merging for optimization. Forbidden by the hard rule, full stop.
- De-optimizing AOT to win benchmarks, benchmarking Jet's front end against Cargo-plus-rustc totals, or any row that subtracts Jet's rustc cost.
- Daemons, background watchers, or a second runner as the reuse mechanism. Warm reuse stays at command boundaries, as #741 ratified.
- Any reliance on the open D-JPK-NIXCACHE1 ballot or on Nix-backed store paths the 2026-08-24 audit measured at zero coverage.
