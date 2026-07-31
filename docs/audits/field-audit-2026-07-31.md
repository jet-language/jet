# Field audit — Zig-style C toolchain and incremental replacement path

**Date:** 2026-07-31. **Trigger:** owner goal statement. **Frame:** day-zero; artifacts only; every finding names work Jet can do.

## Goal audited

Owner statement, 2026-07-31: as a later phase, Jet must (1) support Zig-style compilation of C; (2) offer an exceptional, incremental replacement path for the most common languages, aimed at enterprises and large codebases; (3) win market share through ease of adoption and transition over time; (4) ultimately replace every language in every domain, staged, not overnight.

## Method

Compared the goal against the shipped tree (`crates/jet-pkg-model/src/FFI.rs`, `CFFI.rs`, `CBind.rs`, `CppBind.rs`, `crates/jet-driver/src/Foreign.rs`, `Source/CmdCompile.rs`, examples/tests), the specs (`docs/spec/architecture.md`, `syntax-decisions.md`, `roadmap.md`, `release-policy.md`), the plans (`docs/plans/epoch-3` UL11, epoch-4, epoch-5), and the live Tower board (read-only). No board data was modified.

## What ships today (relevant slice)

- **Call-in C interop is real.** S59 C modules (`use c.<lib>`), `#Bindgen`/`#Extern` overlays, inline `#FFI(c|cpp|asm)` under `#Unsafe`, clang-AST C++ binder, pkg-config / nixpkgs / system-lib link resolution. Proven by `tests/cffi.rs`, `examples/features/lowlevel/{cbind,inline_c,ffi}.jet`.
- **Nineteen language binders are registered Active** (C, C++, JS, Go, Java, .NET, COBOL, Fortran, Ada, Pascal, Lua, Perl, Ruby, PHP, R, Tcl, Dart, PowerShell, COM); Rust namespace binder, Python, Swift are Planned.
- **One narrow source importer:** `jet import py` with a proven subset.
- **Export:** wasm Component Model plugin (`--target=plugin`) only.
- **Toolchain at user sites:** host `rustc`, `cc`/linker, `clang`/`ar` for inline bodies, `pkg-config`, optional `nix`. Jet bundles no C toolchain.

## What is planned

- **UL11 / Unified FFI** (#180, #1120–#1126): one checked boundary for C/C++/Rust/Swift/JS/Python/JVM/.NET/R/Julia; ownership/callback/async mapping; live upstream conformance; **replacement overlays** that must pass differential (golden/fuzz/perf/side-effect) proof before a Jet package may substitute a foreign one (D-FFI-UNIFY1 in-situ shadowing).
- **Enterprise adapters:** COM/VBA (#507), PowerShell/Dart/Tcl/Ada/Pascal (#986–#990), COBOL strangler-fig with copybooks (#504), Octave (#1154).
- **Source migration contract** (#1155/#1156, D-MIGRATE-SRC1): `jet import <lang> <dir>` producing editable Jet plus TODOs, with differential and idempotence proof.
- **Jetpack:** Nix bridge with "consume, coexist, replace; no flag day"; lockfile/migration importers from npm/Cargo/PyPI/etc. (E4-JP17, #941); flake bridge (#1070); offline bundles (#957).
- **Epoch 5:** legacy build wrappers — Jet's build graph wrapping CMake/Make/Gradle/npm/cargo (D-BUILDLEGACY1).
- **Cross-compilation:** toolchain/sysroot resolution (#758), C headers/libs/linker for foreign targets (#1058), expert target controls and matrix (#1059/#1060).
- **Enterprise compatibility promise:** editions plus `jet fix` (`docs/spec/release-policy.md`).

This planned surface is genuinely aligned with staged replacement. The gaps below are what the goal needs and nothing covers.

## Gaps — not covered or planned anywhere

Ranked by leverage toward the stated goal.

### G1 — Jet as guest: no way to compile Jet into a legacy codebase

The entire planned interop story is Jet-as-host (Jet calls foreign code). Incremental enterprise replacement runs the other direction for years: the legacy application stays the host, and teams rewrite leaf modules in Jet that the old build consumes. Missing, with no card or ballot:

- `jet build --emit c-archive|c-shared` producing `.a`/`.so`/`.dylib`/`.dll` with a stable C ABI. (The Dart ballot text mentions `--emit c-archive` in passing; no board item exists.)
- C header generation from exported Jet functions (cbindgen-class).
- Host-runtime packaging of those exports: JNI/Panama bindings for Java, P/Invoke package for .NET, N-API module for Node, CPython extension/wheel, Ruby/PHP native extensions.
- Runtime coexistence facts: Jet runtime init/shutdown inside a foreign process, thread ownership, signal/allocator/TLS policy, two-runtimes-in-one-process rules.

Without G1, "strangler fig" only works when Jet is the trunk. Enterprises will not make Jet the trunk first.

### G2 — No `jet cc`: Zig's actual adoption wedge is absent and unplanned

Zig wins C-estate mindshare because `zig cc` is a hermetic, cross-compiling C/C++ compiler: bundled clang+lld plus libc headers/sysroots for every target, usable as `CC=zig cc` in unmodified Makefiles. Teams adopt the toolchain before the language. Jet has nothing equivalent: inline `#FFI` bodies compile with the **host** clang; Doctor requires host `cc`; no doc, card, or ballot mentions bundling a C toolchain. Needed:

- Owner ballot: bundle clang/lld and multi-target libc sysroots (Zig model) versus hermetic toolchain provisioning through the Jetpack store (Nix model, already half-built). Both reach "clone a C project, `jet cc` builds it on any host for any target"; the second reuses shipped machinery and avoids vendoring LLVM into the product.
- Explicit I6 ruling either way: a bundled clang is a large product dependency; a store-provisioned clang keeps the compiler seam clean.
- `jet cc` / `jet c++` drop-in entry points so foreign build systems can consume Jet as their compiler — the trojan horse.

### G3 — Foreign build hosts cannot invoke Jet

D-BUILDLEGACY1 wraps foreign builds **inside** Jet's graph. The reverse rule set does not exist: CMake toolchain file / `find_package(Jet)`, Gradle/Maven plugin, Bazel rules, MSBuild targets that compile Jet sources or link Jet archives as one step of an existing build. Large codebases change build systems last, not first. No cards.

### G4 — Importer coverage stops at Python

D-MIGRATE-SRC1 defines the contract, and only Python is carded as the first proof. The languages that dominate enterprise line counts — Java, C#, TypeScript/JavaScript, Go, C++ — have binders (call-in) but no source-import scope, not even a scoped-and-rejected verdict. The goal needs, per language, an explicit ruling: source import (full or subset), binder-only coexistence, or overlay replacement — plus cards for the chosen tier. C++ source import in particular should get an honest "binder + manual rewrite only" verdict rather than silence.

### G5 — No publishing into foreign ecosystems

Rust won polyglot share partly through maturin/PyO3 wheels and napi-rs npm packages: foreign developers consume the new language without knowing it. Jet has no plan to publish Jet-built artifacts to PyPI/npm/Maven/NuGet/crates.io with native packaging. Depends on G1. No cards.

### G6 — Mixed-repo developer experience is unspecified

UL11 names "debugger/source-map integration" for FFI, and that is the only mention. A 95%-Java / 5%-Jet repository also needs: mixed-language stack traces, profiling across the FFI boundary, coverage aggregation, test orchestration, and CI recipes for incremental adoption. No cards; not in UL7's tooling scope.

### G7 — Enterprise adoption pack has no owner

`docs/spec/roadmap.md` defers "full adoption documentation (migration, services, debugging guides)" to Epoch 3, but no UL section or card owns it; live onboarding cards (#679, #1034–#1037) are beginner-facing. Missing as work items: per-language migration playbooks, LTS/edition calendar with dates (release-policy promises the mechanism, not the schedule), compliance bundle (SBOM exists in Jetpack lanes; SLSA/provenance attestation and air-gapped install runbooks do not — #957 is adjacent), and reference case studies produced by the UL14 capstones.

### G8 — The rustc dependency is itself an adoption cost

Every AOT user site needs rustc plus a linker today. For "Jet as the toolchain you standardize on," that is a distribution and audit burden until Epoch 9 self-hosting lands. Nothing new to plan — E9 covers it — but the adoption program should state the interim story (pinned hermetic rustc through the store) as a deliverable rather than an accident.

### G9 — Truth fixes in the existing surface

- `docs/spec/philosophy.md` still says "C FFI is a needed future addition"; S59 shipped. Stale prose against I-truth norms.
- The C ABI stop-line stands: `Sema/FFI.rs:90` accepts types `CModule.rs:83` emits as `/* unsupported */ ()`; #180 remains blocked. The wedge strategy is not credible while basic C ABI correctness is open — UL1/#436 ordering is right; this audit just notes the dependency.
- Zig itself is absent from the polyglot binder wave (minor; add or reject explicitly).

## Sequencing finding

UL11 sits late in the epoch-3 dependency order (after UL1–UL3 + UL13), which is correct for FFI correctness — but G1/G2/G3/G5 are not in UL11 or any epoch at all. They form a coherent adoption-wedge program ("Jet as guest" + "Jet as toolchain") whose early slices (c-archive export, store-provisioned `jet cc`) depend only on UL1 C ABI correctness, not on the full UL stack. If market share is the goal, this program deserves its own named epoch or UL11 expansion rather than falling between Jetpack and FFI.

## Keep list (already aligned, do not disturb)

- D-FFI-UNIFY1 in-situ shadowing and UL11 replacement overlays with mandatory differential proof.
- COBOL strangler-fig adapter (#504) — the exact enterprise pattern, applied to the hardest estate.
- Jetpack "consume, coexist, replace; no flag day" and lockfile importers.
- Editions + `jet fix` compatibility promise.

## Proposed new work (ballot-ready choices flagged)

1. **Card: Jet native library export** — `--emit c-archive|c-shared`, header generation, embed/runtime-init contract. (Syntax/CLI spelling → owner ballot.)
2. **Ballot: C toolchain strategy** — bundled clang/lld+sysroots vs Jetpack-store-provisioned hermetic toolchain behind `jet cc`; names the I6 ruling.
3. **Card: `jet cc`/`jet c++` drop-in front ends** over the chosen toolchain, proven by building unmodified upstream C projects (curl/sqlite class) cross-target.
4. **Cards: host-runtime packaging** — Python wheel, Node N-API, JVM, .NET wrappers over exported Jet archives; then foreign-registry publishing.
5. **Cards: foreign-build-host rules** — CMake toolchain file, Gradle plugin, Bazel rules (pick order by target market).
6. **Ballot: per-language migration tier map** — for Java, C#, TS/JS, Go, C++: source import vs binder-only vs overlay; spawns importer cards for the ratified tiers.
7. **Card: mixed-repo DX** — cross-language debug/profile/coverage/CI acceptance lanes, attached to UL7/UL11.
8. **Card: enterprise adoption pack** — playbooks, LTS calendar, compliance bundle, air-gap runbook; capstone case studies feed it.
9. **Chore: fix stale philosophy.md C-FFI sentence.**
