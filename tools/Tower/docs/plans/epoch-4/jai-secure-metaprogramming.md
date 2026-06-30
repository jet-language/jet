# Jai-class metaprogramming, secured for Jet

Status: proposal / decision-development. This is the consolidation target for
the Jai import/build-metaprogramming work: it keeps the useful power from
`jai-import-report.md` and `jai-import-vision.md`, adds the enterprise/security
authority model, and identifies the owner decisions required before the older
Jai proposal docs can be removed.

External context checked while writing this note:

- SLSA v1.2 build requirements:
  https://slsa.dev/spec/v1.2/build-requirements
- SLSA v1.2 build provenance:
  https://slsa.dev/spec/v1.2/build-provenance
- NIST SP 800-218 Secure Software Development Framework:
  https://csrc.nist.gov/pubs/sp/800/218/final
- NTIA SBOM minimum-elements report:
  https://www.ntia.gov/report/2021/minimum-elements-software-bill-materials-sbom
- GNU Make manual:
  https://www.gnu.org/software/make/manual/make.html
- Ninja manual:
  https://ninja-build.org/manual.html
- CMake buildsystem and presets manuals:
  https://cmake.org/cmake/help/latest/manual/cmake-buildsystem.7.html
  https://cmake.org/cmake/help/latest/manual/cmake-presets.7.html
- Meson overview and build-target docs:
  https://mesonbuild.com/Overview.html
  https://mesonbuild.com/Build-targets.html
- Bazel build concepts and remote caching:
  https://bazel.build/concepts/build-ref
  https://bazel.build/remote/caching
- Gradle build cache and incremental build docs:
  https://docs.gradle.org/current/userguide/build_cache.html
  https://docs.gradle.org/current/userguide/incremental_build.html
- Cargo manifest and build-script docs:
  https://doc.rust-lang.org/cargo/reference/manifest.html
  https://doc.rust-lang.org/cargo/reference/build-scripts.html
- Nix derivation docs:
  https://nix.dev/manual/nix/2.28/language/derivations
- MSBuild docs:
  https://learn.microsoft.com/en-us/visualstudio/msbuild/msbuild?view=vs-2022

The Jet-specific grounding is `D-CTEFFECT1`, `D-CTCODEGEN1`, `R11`, `I6`, and the
existing `#Impure` / effect-tag direction.

## 0. The proposal in one paragraph

Jet should import Jai's *power* but not Jai's unrestricted execution model. The
safe version is a capability-oriented compile-time system: pure comptime is
always on; reproducible effects such as `@embed`, `find`, and
`fetch(url, sha256:)` are allowed and recorded in `.jet/lock`; ambient effects
such as env, wall clock, unpinned network, filesystem mutation, and subprocesses
require an audited gate plus build-policy permission. Build-time code can
generate Jet source, but generated source must re-enter lexer -> parser -> sema.
Future macro/message-loop power must run behind a sandboxed, typed compiler API,
not as arbitrary code mutating the compiler process.

The design goal is blunt: an enterprise security team should be able to answer
"what code ran at build time, what authority did it have, what did it read or
write, and can I reproduce it offline?" without trusting prose.

## 1. Threat model

Build-time metaprogramming is code execution before the binary exists. That is
useful, but it is also a supply-chain attack surface.

Risks Jet must make visible and controllable:

- A dependency's build script reads secrets from `$HOME`, CI env vars, SSH
  agents, cloud metadata, or credential files.
- A build step downloads unpinned code or data and silently changes the binary.
- A macro/generator shells out to arbitrary tools that differ by machine.
- Generated code bypasses normal semantic checks or points diagnostics at code
  the user never wrote.
- A malicious package exfiltrates repo contents over the network during build.
- A compile-time cache is poisoned and reused by another build.
- A build is not reproducible because it depends on time, randomness, host
  paths, installed tools, or ambient environment.

The security posture cannot be "users should audit build scripts." The toolchain
must make authority explicit, enforceable, and machine-readable.

## 2. Non-negotiable Jet invariants

These should stay hard walls:

- **I6:** the compiler proper stays std-only. External crates needed for HTTP,
  WASM, JIT, sandboxing, or registry work live outside `Source/` in runtime/tool
  workspace members, are owner-approved, hash-pinned, and carry a native-ize
  obligation.
- **R11 / D-CTCODEGEN1:** generated code goes through the front end. No
  post-sema AST injection. No generated Rust as the user-facing truth.
- **D-CTEFFECT1:** comptime effects use tiers. Pure is default; reproducible
  effects are hashed; ambient effects are gated.
- **R9:** a single `.jet` file still runs with no manifest and no build entry.
- **I8:** one mechanism per job. `comptime` computes values in source. A build
  entrypoint orchestrates a package build. They should not become two spellings
  for the same thing.

## 3. The security architecture

### Layer 1: effect tiers

Keep the already-ratified tier model:

- **Tier 0, pure:** no I/O, no host access, no gate. This is ordinary comptime
  evaluation.
- **Tier 1, reproducible effects:** world-touching but content-addressed and
  lock-recorded. Examples: `@embed("file")`, `find("./packages")`,
  `fetch(url, sha256: "...")`.
- **Tier 2, ambient effects:** non-deterministic or host-authority effects:
  arbitrary fs, env, exec, time, random, unpinned network, `$HOME`, secrets.
  Requires both source-level audit and build-level permission.

Tier 1 is the enterprise sweet spot. It gives most Jai build-time convenience
without hiding inputs from caches, CI, or auditors.

### Layer 2: capability broker

Effect classification is not enough. Tier 2 needs a broker that grants exact
authority.

The broker should be deny-by-default and expose authority through a build context,
not ambient globals:

- Filesystem: read/write roots, glob roots, generated-output roots, no implicit
  `$HOME`.
- Network: fixed-output `fetch` by default; ambient network only to allowlisted
  domains or registries.
- Env: explicit allowlist of env vars; values recorded or redacted according to
  policy.
- Exec: command allowlist with argv captured, tool digest/version captured where
  feasible, no shell string by default.
- Time/random: deterministic injected clock/RNG by default; real clock/random are
  Tier 2.
- Secrets: never visible to dependency build code by default. If supported at all,
  secrets are named capabilities, never ambient env reads.

The useful mental model: a build step does not "have a machine." It has a
`BuildContext` with a small set of explicitly granted handles.

### Layer 3: lockfile and provenance

`.jet/lock` should become the durable audit surface for metaprogramming inputs:

- package graph and package tree hashes
- Tier 1 `comptime_inputs`
- generated source file hashes
- build profile, target, compiler version, edition
- selected build entrypoint and its declared effects
- exact fixed-output fetch URL + hash pairs
- sorted `find()` result sets and tree hashes
- `#Impure` regions that executed, including reason text
- external tool invocations allowed by policy

Separate but derived outputs should include:

- SBOM for dependencies and bootstrap/runtime components
- build provenance compatible with SLSA-style concepts: builder identity,
  external parameters, resolved dependencies, and output digests
- `jet explain-build` / `jet audit-effects` human-readable reports

Enterprises will not accept "trust us, the build is deterministic." They need a
file and a command that proves what happened.

### Layer 4: generated code boundary

Build-time code generation must produce Jet source artifacts or typed source
fragments that re-enter normal checking. The compiler may show generated context,
but the diagnostic owner remains the user's trigger site.

Allowed:

- generate `generated/foo.jet`
- include that file as a normal source input
- parse and check it through lexer -> parser -> sema
- record the generated file hash in `.jet/lock`

Not allowed:

- mutate post-sema AST
- pass raw Rust through to the backend as user metaprogram output
- let macro code call compiler internals directly
- let generated code produce rustc errors as the primary user diagnostic

This is where Jet intentionally differs from the most dangerous form of Jai.

### Layer 5: sandboxed future macro power

If Jet later reopens full Jai-style macros/message-loop power, run that code
outside the compiler trust boundary:

- preferred substrate: sandboxed WASM Component Model or equivalent typed plugin
  ABI
- fallback substrate: isolated subprocess with a typed JSON/binary protocol
- compiler API: read-only typed reflection first; write/generate only through
  source-fragment outputs
- no compiler memory access, no internal AST pointers, no in-process arbitrary
  code from dependencies
- capabilities passed by the host, never discovered by the plugin

This is the difference between "compiler as a library" and "compiler as an
attack surface."

## 4. Build-system parity target

The build entrypoint is only the front door. To make separate build systems feel
silly, Jet needs to cover the jobs existing systems perform, but with Jet's
authority model instead of ambient shell scripts.

Representative lessons:

- **Make:** rule graph, file targets, variables, phony targets, install/clean
  conventions, parallel execution, and user-defined recipes.
- **Ninja:** fast generated action graph, multiple outputs, command-line changes
  as rebuild inputs, depfiles/discovered deps, pools, buffered logs, graph/query
  tools, and a bias toward doing decisions before execution.
- **CMake/Meson/GN:** high-level target model over lower-level build edges:
  executables, static/shared/module/object/interface libraries, custom commands,
  transitive usage requirements, toolchain/cross compilation, build
  configurations, tests, install, package, generated compile databases, and IDE
  support.
- **Bazel/Buck/Pants:** hermetic target graph, monorepo labels, sandboxed
  actions, content-addressed local/remote cache, remote execution, query tools,
  visibility, toolchains, test sharding, and reproducible CI posture.
- **Gradle/MSBuild:** lifecycle targets, task APIs, multi-project builds,
  incremental inputs/outputs, plugins, rich logging/reporting, IDE integration,
  publishing, and configurable entry targets.
- **Cargo:** package/build integration, workspaces, dependency lock, target
  discovery, profiles, features, build scripts, packaging, publishing, and a
  simple "one tool" user experience.
- **Nix/Guix:** derivations with declared inputs, fixed-output fetches,
  content-addressed store, isolated environments, and reproducible setup as part
  of the build story.
- **Autotools/pkg-config/vcpkg/Conan-style ecosystems:** system/library
  discovery, ABI/config probing, generated config headers, native dependency
  packaging, and cross-platform integration with C/C++/system libraries.
- **Xcode/Visual Studio/mobile build stacks:** resources, bundles, assets,
  code-signing, entitlements, manifests, installers, archives, and platform SDK
  packaging.

Parity does **not** mean Jet must copy every syntax or legacy behavior. It means
Jet must own the same jobs in one typed, auditable model.

### Required additions

**A. Typed action graph inside `BuildPlan`.**

`BuildPlan` must lower to a graph of actions, not just a list of source files.
An action needs at least:

- name and stable id
- inputs, outputs, order-only dependencies, validation dependencies
- command/tool or built-in operation
- argv as structured values, not shell text by default
- working directory
- declared env
- required capabilities
- platform/toolchain
- resource pool
- depfile/discovered-dependency mode
- cache policy
- log/output policy

This is the Ninja/Bazel layer. Without it, Jet cannot do correct incremental,
parallel, cached builds.

**B. First-class target model.**

Jet needs target kinds beyond "compile this executable":

- executable
- library: static, shared, module/plugin, object, interface
- test
- benchmark
- doc
- generated source
- asset/resource bundle
- custom action
- install/package/archive
- publish artifact
- plugin/wasm component
- system image / OS image when the JetOS track reaches that layer

Targets need names, labels, visibility, dependencies, outputs, and usage
requirements. A target is the user-facing concept; actions are the execution
plan.

**C. Transitive usage requirements.**

CMake's durable idea is not its syntax; it is target usage requirements. Jet
needs a typed version:

- exported imports/includes
- compile definitions / feature flags / cfg values
- link libraries and link options
- runtime search paths or bundle resources
- required capabilities
- generated headers/modules
- ABI/platform constraints

These requirements should propagate through target dependencies under explicit
rules, not through ambient global flags.

**D. Toolchain and platform model.**

Build settings cannot be ad-hoc strings. Jet needs typed toolchains:

- host/build/target triples
- compiler, linker, archiver, assembler, bindgen/C-bind backend
- sysroot, SDK, libc, CRT, platform version
- wasm/mobile/embedded profiles
- code-signing identity and entitlements as declared capabilities
- tool digests/versions recorded in lock/provenance

This is where CMake toolchains, Bazel toolchains, Nix environments, and platform
SDKs collapse into one Jet-owned model.

**E. Custom actions without shell-script escape by default.**

Jet must support arbitrary build steps, but they should be declared actions:

```jet
ctx.action(.{
    name: "compile shaders",
    tool: ctx.tool("shaderc"),
    args: ["--target", target.gpu, "-o", out, input],
    inputs: [input],
    outputs: [out],
    caps: #(Fs),
})
```

Shell snippets are a Tier-2 compatibility escape hatch, not the normal API. A
custom action with no declared outputs is a side effect, not a build step.

**F. Incrementality and cache as core semantics.**

Every action's cache key should include:

- input content hashes
- generated input hashes
- tool digest/version
- argv
- declared env
- platform/toolchain
- Jet compiler version
- relevant BuildPolicy
- dependency graph version

Jet should support local cache first, remote cache later, and remote execution
only after sandboxing and provenance are solid. Cache poisoning must be designed
against: an action result is reusable only if its key and output digests match.

**G. Scheduler, resources, and pools.**

Build systems are also schedulers. Jet needs:

- parallel execution
- resource pools: cpu, memory, network, linker, console, exclusive device
- jobserver-style integration only as legacy interop, not core dependency
- cancellation and cleanup
- stable buffered logs
- deterministic ordering where output order matters
- progress events for IDE/LSP/CI

Without pools, "parallel by default" becomes flaky on link-heavy, memory-heavy,
or device-backed builds.

**H. Generated-source discipline.**

Generated source must have a first-class home:

- default root: `build/generated/`
- generated file hashes in `.jet/lock`
- generated files listed as target outputs
- generated files checked by sema before downstream compile actions
- stale generated files cleaned by graph ownership
- `--locked` verifies generator outputs or rejects drift

This is the answer to Make/Ninja missing-dependency bugs and Jai AST injection
risk at the same time.

**I. Structured discovery and configuration probes.**

Jet needs replacements for configure scripts:

- `ctx.find_program(...)`
- `ctx.find_library(...)`
- `ctx.pkg_config(...)`
- `ctx.has_header(...)`
- `ctx.has_symbol(...)`
- `ctx.compile_check(...)`
- `ctx.run_check(...)`

Each probe must declare whether it is reproducible. Toolchain-package discovery
can be Tier 1 if the toolchain is locked. Host probing is Tier 2 unless fully
captured in lock/provenance. This keeps enterprise CI from depending on surprise
host state.

**J. Build lifecycle targets.**

Jet needs built-in lifecycle verbs so users do not reach for Make:

- `jet build`
- `jet run`
- `jet test`
- `jet bench`
- `jet doc`
- `jet fmt`
- `jet lint`
- `jet package`
- `jet install`
- `jet publish`
- `jet clean`
- `jet graph`
- `jet query`
- `jet explain-build`
- `jet audit-effects`

The `#Build` entrypoint returns the graph; lifecycle verbs select roots in that
graph.

**K. Tests and benchmarks as graph nodes.**

`#Test` and `#Bench` should become build targets, not separate ad-hoc paths:

- unit/integration/property tests
- test data as declared inputs
- sharding
- retries only when explicitly requested
- per-test sandbox
- coverage/profiling artifacts
- benchmark machine/context metadata

This is required for enterprise CI parity.

**L. Packaging, install, and deploy artifacts.**

Parity with CMake/MSBuild/Xcode requires packaging:

- install layout
- staged `DESTDIR`-style install
- archives
- OS packages where in scope
- app bundles
- resources/assets
- code signing
- SBOM and provenance attached to artifacts
- release profiles

Packaging must be a graph root, not a post-build shell script.

**M. Query and explain tooling.**

Large build systems live or die by introspection:

- list targets
- show dependencies
- show reverse dependencies
- why did this rebuild?
- why did this not rebuild?
- why was this cache miss?
- what generated this file?
- what capabilities did this build use?
- emit compile database
- emit IDE/LSP project model
- emit SBOM/provenance

This should be built into Jet from the start, because it is also the enterprise
audit surface.

**N. Plugin model for build extensions.**

Jet cannot hard-code every ecosystem forever. It needs build plugins, but they
must follow the same sandbox/capability model:

- typed plugin ABI
- no ambient host access
- declared inputs/outputs
- declared capabilities
- versioned API
- cache/provenance integration
- org policy can deny third-party build plugins

This is how Jet replaces Gradle/MSBuild plugin ecosystems without inheriting
their arbitrary in-process execution risk.

**O. Legacy interop as migration, not foundation.**

Enterprises have CMake/Make/Ninja/MSBuild/Gradle today. Jet should interoperate
enough to migrate:

- import `compile_commands.json`
- call legacy build under `ctx.legacy_action(...)` with Tier-2 marking
- wrap a CMake package as an external package
- consume pkg-config/vcpkg/Conan-style metadata through locked adapters
- emit a Ninja file only as a debugging/export artifact, not as the required
  execution engine

The goal is not "Jet shells out to CMake forever." The goal is "Jet can bring
legacy projects under policy while they migrate to native Jet build targets."

### MVP versus full parity

MVP for the first implementation:

1. `#Build fn build(ctx) -> BuildPlan ?`
2. `BuildPlan` with targets and actions, not only source lists
3. executable/library/test/custom-action target kinds
4. generated-source outputs under `build/generated/`
5. local content-hash action cache
6. parallel scheduler with basic pools
7. `ctx.generate`, `ctx.find`, `ctx.fetch(sha256:)`, and declared `ctx.action`
8. `jet build build.jet`, `jet run --build build.jet`, `jet graph`, and
   `jet explain-build`
9. `.jet/lock` records Tier-1 inputs, generated outputs, action keys, and build
   entrypoint
10. no dependency Tier-2 by default

Full parity later:

1. shared/module/object/interface libraries and transitive usage requirements
2. cross compilation/toolchain packages
3. install/package/archive/code-signing roots
4. remote cache and remote execution
5. structured configure probes
6. IDE export and compile database
7. build plugins
8. mobile/embedded/platform bundle support
9. legacy adapters
10. SLSA-style provenance and richer SBOM integration

## 5. Cross-check: current Jet, Tower, and P0 integration

This proposal should not create a second build roadmap. It should become the
parent plan that pulls the existing comptime, package, workspace, cache, target,
and enterprise-audit work into one implementation track.

### Current Jet substrate

The codebase already has meaningful pieces that should be reused:

- **D-CTEFFECT1 is mostly shipped.** `#Impure("reason")`, `--allow-impure`,
  Tier-1 `@embed`/`find` hashing into `.jet/lock`, and E3410/E3411/E3412/L3102
  diagnostics exist. The remaining gap is the `core.net.fetch(url, sha256:)`
  backend, which is wired as Tier 1 but still returns E3412 until the `jet-net`
  runtime-side HTTP member lands.
- **`comptime {}` is shipped.** D-CTMARKER1's execution block exists and runs
  inside the same comptime/effect model. `$` splice is narrower: it is part of
  the derive/reflection track, not a general macro language. The build system
  should not depend on `$` as a second control-flow syntax.
- **R11 / D-CTCODEGEN1 is already a standing architecture rule.** Generated code
  must re-enter lexer -> parser -> sema. The build graph should extend this rule
  to generated source files and action outputs, not reopen AST injection.
- **The effect system exists.** The build entrypoint should reuse existing
  effect tags and ceilings instead of inventing a parallel permission language.
  The missing layer is a `BuildContext` capability broker that maps policy to
  exact handles.
- **`.jet/lock` exists but is not yet a build provenance lock.** Current lock
  support records package graph data and `[[comptime_inputs]]`. It must grow to
  record build entrypoint, profiles, action keys, generated output hashes,
  toolchains, probes, external tools, and policy-relevant parameters.
- **`workspace.jet` exists but is not fully reconciled with the build graph.**
  Source has computable workspace members and a separate `.jet/workspace.lock`.
  Tower notes still mark end-to-end member resolution as partial/folded into
  c50. The comprehensive build plan must decide whether workspace information is
  folded into `.jet/lock` or remains a separate generated lock.
- **D-BUILDPROFILE1 is shipped.** Named profiles, `--release`, `--profile=...`,
  and profile-sensitive rustc flags exist. Build entrypoints should select from
  or extend this surface, not create another profile mechanism.
- **`Source/BuildCache.rs` is a useful precedent, not the final cache.** It keys
  a produced binary from generated Rust source plus profile. Native build parity
  requires an action cache keyed by inputs, outputs, argv, tool digests, env,
  toolchain, policy, and compiler version.
- **Manifest targets exist in pieces.** Executable/library/test/example and
  benchmark targets are shipped. `plugin` remains reserved under c81. The target
  model in this proposal should absorb those package targets rather than fork
  a separate `BuildPlan` target taxonomy.
- **Package build-from-source is partial.** c50 has the source-realization/store
  path and several bridge patterns, but tar tuple conversion, rusqlite/build.rs,
  and workspace member resolution remain unresolved. Native build actions and
  structured toolchains should become the long-term answer to those gaps.
- **Publish/vendor/audit/SBOM work already exists in the package track.** c96
  local publish/yank/resolver and the package trust/SBOM floor should feed the
  enterprise build plan. Registry upload and stronger signing remain gated on
  c56/registry infrastructure.
- **Plugin and JIT decisions preserve I6.** c81's sandboxed WASM plugin target,
  c139's runtime-side JIT dependency, and c164/`jet-net` all follow the same
  pattern: external crates live outside `Source/`, are pinned, and carry a
  native-ize obligation. Build plugins should follow that model exactly.

### Tower cards to integrate

The comprehensive card should explicitly depend on or absorb these existing
cards instead of duplicating them:

- **c157 / D-CTEFFECT1:** prerequisite for build authority. Finish
  `fetch(url, sha256:)` before claiming full Tier-1 parity.
- **c162 / D-CTMARKER1:** shipped `comptime {}` is the local execution block.
  Keep it distinct from the package build entrypoint.
- **c159 / D-BUILDPROFILE1:** shipped profile selection becomes the profile
  layer for `BuildPlan`.
- **c156 / D-WORKSPACE1/D-WORKSPACE2/D-MONOREF1:** workspace members become
  graph roots and package labels; lock strategy must be reconciled.
- **c155 / reflection and user derives:** related metaprogramming substrate.
  Useful for generated-code ergonomics, but not a blocker for build-entrypoint
  MVP.
- **c154 / full Jai message loop:** remains frozen/post-self-host. This proposal
  should explicitly not thaw arbitrary compiler-message-loop macros.
- **c161 / D-CTCODEGEN1:** already shipped as R11; all generator work must obey
  it.
- **c160 / compiler internal seams:** useful for graph/query/IDE exports and
  sandboxed future APIs, but not required for the first build graph.
- **c50 / package build-from-source:** native build actions and toolchains should
  eventually replace bridge-specific hacks and build.rs limitations.
- **c81 / plugin target:** build plugins should reuse the sandboxed WASM
  Component Model direction, not create a second plugin runtime.
- **c96 and c56 / publish, registry, signing:** packaging/publish roots should
  feed existing publish UX; signed cache remains gated on registry/signing
  decisions.
- **c139 / JIT and c144 / debugger:** useful downstream consumers of the target
  graph, but not core prerequisites.
- **c164 / full HTTP library:** not required for the build system beyond the
  immediate c157 fixed-output fetch backend.

### P0 overlap map

Relevant P0 ideas map into this proposal as follows:

| P0 idea | Integration action |
| --- | --- |
| `$` for comptime/macros | Reuse D-CTMARKER1 only for splices; do not add `$if`/`$for` or general macro control flow. |
| Library extensibility tiers | Respect D-EXT1: protocols/open hooks are fine; third-party reader/proc macros stay out of v1. |
| User-authored derives/reflection | Keep under c155; build graph can consume generated source but does not require derive reflection to start. |
| Full Jai metaprogramming/message loop | Keep frozen under c154; future version must be sandboxed and typed. |
| Broad gated build-time I/O | This is the direct prerequisite: D-CTEFFECT1 plus `BuildContext` policy broker. |
| Jai/Zig-style build system | This proposal becomes the comprehensive parent plan for that P0 item. |
| Named build profiles | Already shipped; integrate as the profile surface for build entrypoints. |
| Monorepo workspace surface | Integrate workspace members as labels/roots in the build graph. |
| Effect ceilings / `#(no_net)` prohibitions | Future policy hardening; not an MVP blocker, but the build effect model should leave room for it. |
| Budgets/resource tags | Future scheduler/resource policy; MVP should still include simple pools. |
| Compiler internal seams | Use for `jet query`, IDE export, and future typed compiler APIs. |
| Package build-from-source | Reuse source store/fetch/build infrastructure; replace ad-hoc bridge gaps with native actions. |
| Signed package cache | Enterprise mode should require signed packages once c56 thaws. |
| Publish/registry UX | Packaging/publish build roots should call into the existing c96 surface. |
| Plugin target | Build plugins should ride c81's sandboxed WASM target. |
| JIT tier | Keep as a runtime/dev target consumer, not a build-system dependency. |

### Reconciliation items before implementation

These are the seams that need owner-approved decisions or explicit card text:

1. **Unified lock versus `.jet/workspace.lock`.** Recommended: `.jet/lock`
   becomes the canonical build/provenance lock, with workspace lock either
   folded in or treated as a generated compatibility view.
2. **`#Build` marker versus `Build.entry` alone.** Recommended: keep both:
   `Build.entry` selects the function and `#Build("reason")` makes authority
   auditable at the definition site.
3. **BuildContext-only authority versus direct `core.fs`/`core.exec`.**
   Recommended: build entrypoints use `BuildContext` for all world access.
   Direct Tier-2 `core.*` stays available for narrow comptime code but is not
   the normal build-system API.
4. **Dependency Tier-2 default.** Recommended: dependencies get Tier 0 and
   locked Tier 1 by default; Tier 2 dependency build code is denied unless an
   explicit project/org policy grants it.
5. **Existing binary cache versus action cache.** Recommended: keep
   `BuildCache` as a precedent or wrapper, but introduce an action-cache key
   model rather than extending the source/profile hash until it becomes opaque.
6. **Profile home.** Recommended: package `build {}` profiles remain canonical;
   `build.jet` may compute targets/actions for the selected profile, but should
   not define a competing profile language.
7. **Generated output root.** Recommended: build-owned generated source lives
   under `build/generated/` and is verified under `--locked`.
8. **Legacy interop boundary.** Recommended: CMake/Make/Ninja/MSBuild/Gradle
   calls are Tier-2 legacy actions with declared inputs/outputs. They are
   migration tools, not the foundation.
9. **Manifest target merge.** Recommended: package targets lower into the same
   `BuildPlan` target graph used by build entrypoints.

### Implementation sequence for a comprehensive Tower card

1. **Status sync.** Update stale sidequest text that still says D-CTEFFECT1 or
   `comptime {}` are unimplemented, and record the real remaining gaps:
   `fetch(url, sha256:)`, workspace end-to-end resolution, and action graph.
2. **Decision bundle.** Create ballots for `D-BUILDENTRY1`, `D-BUILDACTION1`,
   `D-BUILDTARGET1`, `D-BUILDTOOLCHAIN1`, `D-BUILDCACHE1`, `D-BUILDPROBE1`, plus
   two additional seams: `D-BUILDPOLICY1` for enterprise policy shape and
   `D-BUILDLOCK1` for unified lock/provenance ownership. If this proposal is to
   replace the older Jai reports entirely, include `D-SURFACEFAMILY1` for typed
   surface ownership.
3. **MVP build graph.** Implement `BuildContext`, `BuildPlan`, typed targets,
   typed actions, generated-source outputs, `jet build build.jet`, and
   `jet run --build build.jet`.
4. **Lock, cache, and explain.** Extend `.jet/lock`, add action keys/output
   hashes, add local action cache, and ship `jet graph` plus
   `jet explain-build`.
5. **Workspace/package integration.** Lower package manifest targets and
   `workspace.jet` members into the same graph; connect c50 source-realization
   and c159 profiles.
6. **Enterprise modes.** Add policy files/modes, `jet audit-effects`,
   offline/locked verification, SBOM/provenance attachment, and signed-package
   enforcement when c56 is ready.
7. **Parity expansion.** Add structured probes, toolchain packages, packaging
   roots, install/deploy roots, build plugins, legacy adapters, remote cache,
   and remote execution in that order.

### Jai import report coverage

The older `jai-import-report.md` framed Jai's importable power as three things:
integrated build system, compile-time execution, and a compiler reachable from
build/user code. This proposal covers the first two directly and keeps the third
behind Jet's existing safety line: internal compiler seams for tools now,
reflection/derives for user code in v1, and any deeper message-loop power only
through a future sandboxed typed API.

Coverage against the report:

| Import report item | Status in this proposal |
| --- | --- |
| Integrated build system | Covered by `#Build`, `BuildContext`, typed `BuildPlan`, targets/actions, lifecycle verbs, package/workspace/profile integration, cache, and policy. |
| Compile-time execution | Covered by pure comptime, `comptime {}`, D-CTEFFECT1 tiers, Tier-1 fixed inputs, and Tier-2 `#Impure` plus policy. |
| Compiler reachable from build/user code | Partially covered by c160 seams, c155 reflection/derives, generated source re-entry, and future sandboxed macro/plugin API. Not exposed as arbitrary compiler mutation. |
| Jai `#run` value computation | Covered by Jet comptime bindings and `comptime {}` under effect tiers. |
| Jai `#run` build orchestration | Covered by the build entrypoint, not by hidden module-level execution. |
| Jai `#insert` / AST injection | Intentionally rejected by R11/D-CTCODEGEN1. Jet generates source and re-checks it. |
| Reflection / `Type_Info` | Owned by c155. Build graph can consume generated source without depending on full reflection for MVP. |
| Package manager | Jetpack remains the package layer; this proposal integrates it but does not replace it. |
| Workspaces and monorepo addressing | Covered by c156/D-WORKSPACE and D-MONOREF integration; implementation still needs real member resolution through the build graph. |
| Build profiles | Covered by shipped D-BUILDPROFILE1; build entrypoints must reuse it. |
| Internal compiler seams | Covered by c160 as a supporting card, not a blocker for the first build graph. |
| `System` / `Image` typed surfaces | Not owned by this build card. The build graph must leave target kinds for system/image artifacts, but OS-surface semantics stay behind D-OS ballots. |
| `Env` / dev-shell surface | Not owned by this build card. Toolchains and build environments overlap, but dev-shell UX should remain a separate surface unless owner folds it in. |
| Jai memory-model ergonomics: allocator context, arenas/temp storage, scoped cleanup, SOA/AOS | Mostly existing or tracked elsewhere: arena/regions, `#Context(allocator: ...)`, `core.scope.guard`, and `#layout(columnar)` are not build-system blockers. Do not hide remaining allocator/context work in this card. |

The only feature from the report not yet represented as a clear decision seam is
the **family of typed surfaces** claim: `Package`, `Env`, `Build`, `Workspace`,
`System`, and `Image` as one grammar/evaluation model with partitioned
responsibilities. The build plan can be compatible with that vision, but it
should not silently ratify it for every surface. `D-SURFACEFAMILY1` below exists
for that exact owner choice.

### Devil's advocate pass

These are the objections from the older report, updated against current Jet and
this proposal.

1. **Typed surfaces can double-home facts.** Resolved only if
   `D-SURFACEFAMILY1` or equivalent card owns a fact matrix. The build card
   should not put sources, deps, profiles, and workspace members in multiple
   places.
2. **Computed workspace members hurt external tooling.** Current Tower direction
   chose full-power `workspace.jet`, so the mitigation is lock/provenance, not a
   re-litigation of the weaker declarative option. `D-BUILDLOCK1` must decide
   how `.jet/workspace.lock` relates to `.jet/lock`.
3. **One build entrypoint may feel weaker than Jai's anywhere-`#run` model.**
   Resolved by separating jobs: `comptime {}` handles local computation;
   `#Build` handles graph construction. Hidden dependency entrypoints remain a
   hard no unless policy grants them.
4. **Source generation may feel weaker than AST injection.** Intentionally
   resolved by R11. The hard lock is no post-sema mutation. Reopening this would
   require reversing D-CTCODEGEN1, not just adding a build feature.
5. **Compiler-as-library can become a security hole.** Resolved for MVP by not
   exposing arbitrary compiler mutation. Future compiler APIs must be typed,
   capability-scoped, and preferably out-of-process or WASM-sandboxed.
6. **A self-contained build system can accidentally become "Jet shells out to
   CMake."** Resolved by making legacy actions Tier-2 migration edges with
   declared inputs/outputs. Native targets/actions/toolchains are the foundation.
7. **`BuildContext` could become an ambient authority bag.** Requires
   `D-BUILDPOLICY1`: capabilities must be exact handles, not a back door to the
   host machine.
8. **Action caching can lie.** Requires `D-BUILDCACHE1`: cache keys include
   inputs, tools, env, argv, platform, policy, compiler version, and output
   digests. Timestamp-only cache is rejected.
9. **Dependency build code is the enterprise adoption cliff.** Resolved by
   defaulting dependencies to Tier 0 plus locked Tier 1. Tier 2 dependency builds
   require explicit policy and provenance.
10. **System/Image and Env can be accidentally pre-empted by the build plan.**
    Resolved by treating them as integration points only. Do not decide OS or
    dev-shell semantics in a build-system card.
11. **Jai memory-control features could be lost when deleting the old docs.**
    Resolved by noting they are mostly implemented/tracked outside this card.
    Any remaining allocator/context policy should stay in the memory/runtime
    track, not the build-metaprogramming track.
12. **Deleting the old reports could lose rationale.** Resolved only after this
    proposal gains `D-SURFACEFAMILY1` or an explicit statement that the surface
    family is out of scope. Until then, `jai-import-report.md` is superseded for
    build/comptime/security, but not fully superseded for the broader typed
    surface architecture.

### Deletion readiness for the older Jai docs

The older Jai docs can be removed once this file is accepted as the canonical
planning source and these conditions are true:

1. `D-SURFACEFAMILY1` is either accepted into the comprehensive plan or explicitly
   rejected/out-scoped with a note that `Package`/`Env`/`System`/`Image` stay on
   their existing tracks.
2. The build decision bundle covers all remaining report decisions: build
   entrypoint, action graph, targets, toolchains, cache, probes, policy, lock,
   dependency effects, and generated-source boundary.
3. c156/c50 carry the workspace/member-resolution residue from the old D2/D3
   discussion, including external-tool lock visibility.
4. c155/c154/c160 carry the compiler/reflection/message-loop residue from the
   old D6/D7 discussion.
5. Memory-control notes from `jai-import-vision.md` stay tracked by the memory
   and low-level roadmap, not by this build card.

Supersession ledger:

| Old doc item | Canonical home after deletion |
| --- | --- |
| `jai-import-report.md` D1, dot construction | Already shipped under D-DOTCTOR1/c158; not part of this build plan. |
| D2, `Workspace` Jet surface versus `jetpack.toml` | c156/D-WORKSPACE already chose `workspace.jet`; this plan owns lock/provenance reconciliation through `D-BUILDLOCK1`. |
| D3, in-monorepo `source.package` addressing | c156/D-MONOREF plus c50/c156 implementation residue. Build graph must consume these as package labels. |
| D4, compile-time effect tiers | c157/D-CTEFFECT1 plus this proposal's `BuildContext`, policy, lock, and dependency-default rules. |
| D5, generated source re-check | c161/R11/D-CTCODEGEN1 plus this proposal's generated-source discipline. |
| D6, metaprogramming depth | c155 reflection/derives for v1; c154 full Jai message loop remains frozen; future macro power must be sandboxed. |
| D7, compiler library seams | c160; useful for query/IDE/future APIs, not a build MVP blocker. |
| D8, build profiles | c159/D-BUILDPROFILE1 shipped; build entrypoints reuse it. |
| Vision syntax primer | Canonical syntax lives in `docs/spec/` and syntax decisions. The vision examples are illustrative, not requirements. |
| Persona 1 single-file path | Preserved as R9 in this proposal and existing spec behavior. |
| Persona 2 CLI/package/build profile path | Split across package manifest/Jetpack, c159 profiles, and this build plan. |
| Persona 3/4 monorepo scale story | Split across c156 workspace/addressing, c50 build-from-source, `api: stable`, and this build graph. |
| `api: stable` / stable surfaces | Already ratified/implemented under D-CAP4/D-CAP6/D-CAP8/c129; not owned by this build card. |
| Dev shell / `Env` surface | Existing Jetpack/env track; only overlaps through toolchain/build-environment integration. |
| `System` / `Image` / JetOS examples | D-OS/JetOS track. Build graph should expose artifact roots but not decide OS semantics. |
| Native/polyglot dependencies through sources | Jetpack providers, C-FFI/native dependency work, c50, and this proposal's toolchain/probe/action graph. |
| Jai memory-control ergonomics | Existing arena/region, `#Context(allocator: ...)`, `core.scope.guard`, `#layout(columnar)`, and memory/P0 roadmap. |
| Persona prose and narrative examples | Intentionally not preserved as requirements. They can be deleted once this ledger is accepted. |

## 6. Enterprise build modes

Jet should support a few named policy postures rather than forcing every
enterprise to invent wrapper scripts.

Recommended modes:

- **Developer default:** Tier 0 and Tier 1 allowed; Tier 2 requires
  `#Impure("reason")` and `--allow-impure`.
- **CI strict:** `--locked`, no unrecorded Tier 1 drift, Tier 2 hard-error unless
  explicitly allowed by project/org policy.
- **Offline:** no network except already-vendored/fetched store entries;
  fixed-output fetch must resolve from store.
- **Enterprise restricted:** only approved registries, signed packages where
  required, allowlisted build entrypoints, no dependency Tier 2 by default.
- **Repro audit:** rebuild from lock and compare generated source hashes and
  output digests.

Illustrative command shape:

```text
jet build --profile=ci --locked --offline --deny-impure --sbom --provenance
jet audit-effects
jet explain-build
```

Policy should be data, not convention. A future policy surface could own:

```jet
security: BuildPolicy.{
    impure: .deny
    network: .fixed_output_only
    env: []
    exec: []
    require_signed: true
    provenance: .slsa
    sbom: .spdx
}
```

That exact spelling is illustrative; the important part is one canonical policy
home.

## 7. Compile-time entrypoint: the good version of the idea

The idea of a compile-time entrypoint analogous to runtime `main` is strong. It
gives build metaprogramming a single choke point. The security version should be
explicitly selected by the package/build surface, not magically discovered in
every module.

The minimum capability this proposal is meant to support:

1. The user writes a `build.jet` file containing one explicit build entrypoint.
2. The driver runs that build entrypoint before compiling the runtime program.
3. The build entrypoint sets build settings by returning a `BuildPlan`, not by
   mutating compiler globals.
4. The driver compiles the runtime entry/sources named by that `BuildPlan`.
5. If the user asked to run, the driver runs the produced artifact.
6. Every authority used by the build entrypoint flows through `BuildContext`,
   D-CTEFFECT1 tiers, and the active CLI/project/org policy.

Illustrative CLI:

```text
jet build build.jet
jet run --build build.jet
```

Whether plain `jet run build.jet` should auto-detect a build entrypoint is a CLI
sub-decision. The conservative recommendation is no: `jet run file.jet` keeps
meaning "compile and run this file's runtime `main`", while `jet run --build
build.jet` means "run the build entrypoint, compile its plan, then run the
result." A softer option is to allow `jet run build.jet` only when the file has
exactly one `#Build` entrypoint and no runtime `main`, but that is convenience
syntax over the same explicit internal path.

Recommended shape:

```jet
build: Build.{
    entry: build
    profiles: [
        .{ name: "debug", optimize: .none },
        .{ name: "release", optimize: .speed },
        .{ name: "ci", optimize: .speed, security: .locked },
    ]
}

#Build("generate assets and schema")
fn build(ctx: BuildContext) #(Fs, Net) -> BuildPlan ? {
    assets #= ctx.find("assets/**/*.png")
    schema #= ctx.fetch(
        "https://example.com/schema.json",
        sha256: "..."
    )?

    source #= generate_schema_module(schema)
    ctx.generate("generated/schema.jet", source)?

    return ctx.plan(
        sources: ["src/main.jet", "generated/schema.jet"],
        assets: assets
    )
}
```

This gives Jet a build-time `main`, but with stricter rules than runtime `main`:

- The entrypoint is package/workspace scoped, not per imported module.
- It is opt-in. A bare file still has no build entrypoint.
- It receives `BuildContext`; all world access flows through that context.
- It declares an effect ceiling with the existing `#(Fs, Net)` mechanism.
- Tier 1 actions are hashed into `.jet/lock`.
- Tier 2 actions still require `#Impure("reason")` and policy permission.
- It returns a `BuildPlan`, not arbitrary compiler mutations.
- Generated source is checked normally.
- The driver can run `jet audit-effects` without executing arbitrary hidden module
  code.

Why not call it `comptime main` and auto-run it by name? Because compile-time
execution is more dangerous than runtime execution. Running `main` is the user's
explicit goal. Building a dependency should not silently execute a hidden
top-level function with host authority. The build entrypoint should be named and
selected by the `Build` surface so CI, IDEs, and auditors know where authority
starts.

Why not only use `comptime { ... }` blocks? Because they solve a different job.
`comptime { ... }` is local build-time work inside a source context. A build
entrypoint computes the build graph: sources, generated files, assets, profiles,
targets, and policy. Keeping those separate preserves I8.

## 8. Dependency build entrypoints

The root package and dependencies need different defaults.

Root package:

- may run its selected build entrypoint under the current project policy
- may use Tier 2 only with explicit flags/policy
- owns generated source under the project build directory

Dependency package:

- may run Tier 0 and Tier 1 under lock/store control
- should not get Tier 2 by default, even if the root allows Tier 2 for itself
- must run in a sandbox with no repo-wide read access
- generated outputs become part of the package store fingerprint
- any dependency build entrypoint must be visible in the lock/provenance

This is where enterprise adoption will be won or lost. Most organizations can
tolerate powerful root builds. They will reject arbitrary dependency build code
that reads the host or network by default.

## 9. What to build next

Near-term:

1. Finish the remaining D-CTEFFECT1 gap: the `fetch(url, sha256:)` backend in
   the runtime/tool workspace, with sha256 verification and lock recording.
2. Add the build decision bundle: `D-BUILDENTRY1`, `D-BUILDACTION1`,
   `D-BUILDTARGET1`, `D-BUILDTOOLCHAIN1`, `D-BUILDCACHE1`, `D-BUILDPROBE1`,
   `D-BUILDPOLICY1`, `D-BUILDLOCK1`, and, if replacing the older Jai reports,
   `D-SURFACEFAMILY1`.
3. Add a native action graph to `BuildPlan`; do not ship a source-list-only
   build entrypoint as the final shape.
4. Add `jet graph`, `jet explain-build`, and `jet audit-effects` over `.jet/lock`,
   action keys, and sema effect summaries.
5. Integrate shipped build profiles, workspace members, and manifest targets into
   the same graph instead of creating parallel mechanisms.

Later:

1. Add policy surface and enterprise modes.
2. Add generated-source hash verification and replay checks.
3. Add SLSA-style provenance output.
4. Add sandboxed plugin/macro substrate if full Jai-style metaprogramming is
   reopened.

## 10. Decision seam

**D-BUILDENTRY1 - compile-time build entrypoint.**

Options:

- **A. Explicit `Build.entry` points at a `#Build fn build(ctx) -> BuildPlan ?`.**
  Recommended. One choke point, explicit in manifest/surface, auditable.
- **B. Magic `fn build()` discovered by name.** Convenient, but too implicit for
  enterprise build execution.
- **C. Magic `comptime main` in any module.** Reject. Hidden execution by import
  creates exactly the supply-chain risk Jet is trying to avoid.
- **D. No build entrypoint; only `Build.{...}` fields and `comptime { ... }`.**
  Safest, but leaves too much build orchestration power outside Jet.

Sub-decisions:

- Does the marker spell `#Build`, `#ComptimeMain`, or no marker at all because
  `Build.entry` is enough?
- What is the CLI spelling? Recommended: `jet build build.jet` and
  `jet run --build build.jet`, with optional later sugar for `jet run build.jet`
  only when the file is unambiguously a build file.
- Is `BuildContext` the only authority path, or can `core.fs`/`core.exec` be
  called directly under `#Impure`?
- Are dependency build entrypoints Tier-2-disabled by default? Recommended yes.
- Does `BuildPlan` permit generated files only under `build/generated/`, or can a
  package write beside source? Recommended generated directory only.
- Does `--locked` verify generated source hashes? Recommended yes.

**D-BUILDACTION1 - native action graph.**

Options:

- **A. `BuildPlan` lowers to typed targets and actions.** Recommended. This is
  the only path that can replace Ninja/Make/Bazel-class execution.
- **B. `BuildPlan` is a list of sources/settings only.** Too weak; useful for an
  MVP demo but not a real build-system replacement.
- **C. `BuildPlan` emits a Ninja/Make file and delegates.** Reject as the core
  model. Accept only as an export/debug/migration feature.

**D-BUILDTARGET1 - target and usage-requirement model.**

Options:

- **A. First-class target kinds plus transitive usage requirements.**
  Recommended. This is the CMake/Meson parity layer.
- **B. Only package-level executable/library fields.** Too small for enterprise
  C/C++/native/mobile/library graphs.

**D-BUILDTOOLCHAIN1 - toolchain/platform model.**

Options:

- **A. Typed, locked toolchains with host/build/target separation.**
  Recommended. Necessary for cross compilation, SDKs, C ABI work, and
  reproducibility.
- **B. Let build code discover tools from PATH by default.** Reject. It recreates
  configure-script nondeterminism.

**D-BUILDCACHE1 - action cache and scheduler.**

Options:

- **A. Content-addressed local action cache plus parallel scheduler/pools.**
  Recommended. Remote cache/execution can layer later.
- **B. Timestamp-only incremental builds.** Reject for Jet's secure default.

**D-BUILDPROBE1 - structured configure probes.**

Options:

- **A. Typed probes through `BuildContext`, recorded in lock/provenance.**
  Recommended.
- **B. Shell out to configure scripts as the normal path.** Reject as the normal
  path; allow only as Tier-2 legacy interop.

**D-BUILDPOLICY1 - enterprise build policy surface.**

Options:

- **A. One canonical `BuildPolicy` surface used by CLI, package, workspace, and
  org policy.** Recommended. It prevents wrapper-script drift and lets CI audit
  one policy model.
- **B. Flags only.** Too weak for enterprise reuse, but useful as command-line
  overrides.
- **C. Separate policy mechanisms for build, package, and workspace.** Reject by
  I8; it makes authority impossible to audit consistently.

**D-BUILDLOCK1 - unified build/provenance lock.**

Options:

- **A. Extend `.jet/lock` into the canonical package, comptime, workspace, action,
  and provenance lock.** Recommended. One artifact answers what ran and why.
- **B. Keep `.jet/workspace.lock` and action locks as separate canonical files.**
  Acceptable only if `.jet/lock` indexes them and `jet audit-effects` treats
  them as one graph.
- **C. Leave build actions outside the lock.** Reject; this loses the enterprise
  audit trail.

**D-SURFACEFAMILY1 - typed surface family and fact ownership.**

Options:

- **A. Adopt the family model as an architecture rule.** Authored project
  configuration is Jet typed surfaces, with one owner per fact: package identity
  and deps in `Package`/manifest, member discovery in `Workspace`, build graph in
  `Build`, dev environment in `Env`, and machine/image config in
  `System`/`Image`. Recommended if this proposal is meant to supersede the old
  Jai import reports completely.
- **B. Adopt only `Build` plus already-ratified `Workspace`, and leave
  `Package`, `Env`, `System`, and `Image` on their existing tracks.** Safer for
  the build card, but the old "one typed surface family" rationale is not fully
  superseded.
- **C. Keep each surface independent with no fact-ownership rule.** Reject if the
  goal is one comprehensive Jai-import plan; this reintroduces double-homing risk
  and weakens I8.

## 11. Bottom line

Jet can get the full practical power of Jai by making compile-time execution a
first-class build graph with capabilities, not a free-for-all interpreter hook.
The enterprise-safe slogan is:

**Jai power, Jet authority model.**

Power users get build-time computation, source generation, reflection, fixed-output
fetching, and eventually sandboxed macros. Enterprises get hermetic defaults,
least privilege, lockfiles, SBOM/provenance, and one place to audit every effect.
