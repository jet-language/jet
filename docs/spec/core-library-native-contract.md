# Native Jet CoreLib contract

This document records the owner-confirmed contract for making Jet CoreLibs
complete, native, comprehensive, reliable, and competitive with first-class
standard libraries. It defines durable architecture and completion law. Tower
owns execution order, current status, blockers, and evidence logs.

## Goal

Jet CoreLibs are Jet modules that own public behavior. They must be useful for
real programs, safe by default, explicit about effects and capabilities, and
complete enough to compete with the strongest relevant parts of C, Rust,
Python, Bash, and other peer ecosystems.

There are no transitional placeholders. No exported operation may silently
return an empty value, false success, discarded data, fake deterministic
entropy, or an unimplemented fallback.

## Priorities

1. Correctness.
2. Performance.
3. Friction proportional to real usage frequency.
4. Beginner-friendly defaults with explicit expert control.

Correctness closes before comparative optimization. A performance result never
justifies changed semantics, weaker diagnostics, weaker safety, or tier drift.

## Scope

Horizon 1 covers every declaration currently present in `Core.jet`:

- all current module rows;
- all root types;
- all format declarations;
- all namespace-only declarations.

Each declaration needs a meaningful contract. Metadata declarations must have a
clear compiler or runtime role and a generated consistency proof. They cannot
hide missing callable behavior.

Horizon 1 exits only when every declaration has a real implementation and
scaled complete evidence, with no known gap left unresolved. Horizon 2 then
expands the surface using a mission and usage matrix. It adds high-value
missing capabilities across real Jet jobs and domains instead of attempting a
blind union of every peer library.

## Source authority

Jet module source is authoritative for Core exports and behavior. `Core.jet`
retains bootstrap, dependency, format, capability, and explicit native-dispatch
metadata. Generated registries and tier views derive from those declarations.

The generator must verify that:

- every declared export has one canonical Jet source;
- generated views match the declaration source;
- obsolete exports, aliases, shims, and duplicate mechanisms are absent;
- capability, effect, authority, and fallback metadata remain consistent.

Generated Rust is a projection, not a second API catalog. Generated files are
never hand-edited.
 
## Source links

- [Jet Core modules](../../Core/) own public exports and semantic bodies.
- [`Core.jet`](../../crates/jet-codegen/src/Prelude/Core.jet) owns bootstrap,
  dependency, format, capability, and explicit dispatch metadata.
- [Core table generator](../../scripts/agent/gen-core-tables.mjs) writes the
  generated projections.
- [Generated Core call projections](../../crates/jet-foundation/src/Syntax/core_calls.rs)
  and [module exports](../../crates/jet-foundation/src/CoreModuleExports.rs)
  are derived artifacts, never independent sources.

## Jet and Rust boundary

Jet owns:

- algorithms and composition;
- parsing and validation;
- defaults and policy;
- authority and effects;
- typed error meaning and diagnostics;
- data transformation and public API behavior.

Rust may provide only typed, policy-free leaf primitives. A leaf has a narrow
ABI, no user-facing policy or default selection, no multi-step application
semantics, and no backend-specific interpretation of Jet errors. Examples
include OS access, unsafe platform operations, vetted cryptographic primitives,
and hardware interfaces.

High-level Rust implementations are removed after the shared proof and route
audit for their wave. They are not retained as runtime fallbacks.

## One meaning across tiers

AOT, JIT, interpreter/comptime, and web are points on one continuum:

- interpreter: fastest preparation, slowest execution;
- JIT: middle preparation and execution cost;
- AOT: slowest preparation, fastest execution;
- web: the same semantic contract through its provider boundary.

Only preparation and runtime cost may differ. Validation, defaults, policy,
effects, authority, errors, limits, cancellation, and observable values must
not differ.

Every applicable tier uses one semantic Core path and one shared provider
contract. Tier adapters provide representation and execution mechanics; they do
not reimplement Core policy.

## Capabilities and fallbacks

Every host-sensitive operation declares one capability kind:

- `Pure`: universally executable without ambient resources.
- `NativeRequired`: needs a real host or security capability; no fallback is
  semantically valid.
- `PortableFallback`: has a canonical Jet fallback plus optional native
  acceleration.
- `ExplicitSimulation`: has a named virtual or test provider selected
  explicitly by the caller or tool.

Capability policy is generated metadata. It records the required effect and
authority, fallback identity where applicable, and canonical failure.

Safe defaults and `auto` may select a semantics-preserving portable fallback.
An explicit backend request never silently downgrades. A missing capability
returns the same typed, deterministic capability error on every tier.

Examples:

- automatic compute may use CPU when no GPU exists;
- explicit CUDA selection returns `DeviceUnavailable` instead of using CPU;
- a web filesystem needs a real virtual or host capability, otherwise it
  returns `FSUnavailable`;
- OS entropy is native-required and never falls back to deterministic bytes;
- a portable regex implementation may be accelerated by a native backend;
- virtual filesystems, fake clocks, in-memory databases, and test networks are
  explicit named providers, not hidden production substitutions.

Effects and authority remain the canonical user-visible access boundary.
Capability objects exist for explicit expert control and provider machinery,
not as a second permission system.

## API design

The first pass preserves valid semantics and coherent identity closely enough
for direct comparison. It does not preserve poor or misleading API shape.

An incoherent export is replaced at the contract boundary. Every caller,
example, test, registry, generated use, document, and environment variable is
migrated in one clean cutover. Runtime compatibility wrappers, aliases,
parallel parsers, and fallback paths are deleted.

The second pass may change any contract when the new Jet design has proof. It
may improve names, types, defaults, diagnostics, algorithms, allocation, and
performance. Public changes that are material owner decisions are gated before
implementation.

Peer languages provide semantic and edge-case evidence. Jet uses its own types,
effects, authority, diagnostics, and ergonomic surface. Common operations have
low-friction safe defaults. Expert control is explicit rather than hidden.

## Capability and API evidence

Every module or coherent module cluster receives a contract card before code.
The card freezes:

- API and types;
- semantics and edge cases;
- effects and authority;
- capability kind and fallback policy;
- error and diagnostic codes;
- peer references;
- valid baseline receipts;
- examples and goldens;
- hostile and failure cases;
- tier matrix;
- frequency and hot-path cells;
- explicit non-goals.

Every export receives scaled but complete evidence for normal behavior,
boundaries, hostile inputs, typed errors, diagnostics, authority/effects,
resource limits, determinism, concurrency or reentrancy where relevant, and
cross-tier parity.

First-pass comparison uses semantic fixtures. Coherent surfaces may compare
directly with valid Rust behavior. Redesigned surfaces map valid Rust inputs to
the new Jet contract and compare semantic values and errors. No runtime
compatibility layer is kept for this purpose.

## Implementation waves

Waves are dependency-ordered and cannot hand off partially.

1. **Foundations:** root carriers, text, collections, math, units, time,
   encoding, regex, args, files/path contracts, memory/IO foundations, and
   portable crypto primitives.
2. **System:** archives, networking, process, tasks, events, watchers,
   terminal, logging, email, and database.
3. **Data and web:** data loaders and transformations, HTTP, web/router/forms,
   reactive, sync, services/jobs, storage, and browser.
4. **Specialized:** compute/models, UI/TUI/font, games/raylib, plugin/mod,
   compiler/reflect, testing, and remaining host domains.

The exact member order is derived from the Core dependency graph. High-use
foundations lead within that graph.

For each wave:

1. write and owner-gate material contract choices;
2. freeze valid baseline behavior;
3. implement Jet semantics in the self-hosting safe subset;
4. implement the shared provider and approved leaf seams;
5. prove every applicable tier and failure surface;
6. migrate all callers and generated consumers;
7. remove obsolete high-level Rust and placeholders;
8. close correctness and record a performance baseline;
9. optimize only after correctness closure;
10. rerun parity and strict performance cells.

## Performance and learnability

The eventual performance gate is strict per matched workload and metric. No
aggregate score may hide a loss. Hot paths and representative workloads carry
peer cells for latency, throughput, allocation, copying, startup, compilation,
and artifact size where applicable. No performance claim extends beyond its
measured coverage.

Performance-motivated surfaces have paired plain and optimized programs. Safe
Jet code is optimized first. Audited native or unsafe leaves are explicit
expert paths and preserve the same contract.

A layered, frequency-weighted corpus combines current examples and
conformance, repository usage, representative beginner tasks, and domain
workloads. High-frequency APIs receive paired beginner/expert evidence for
source size, diagnostics and recovery, preparation cost, and runtime cost.

## Completion law

A CoreLib area is complete only when:

- Jet owns its public behavior;
- all declarations have meaningful contracts;
- every applicable tier shares one observable meaning;
- capability policy is explicit;
- errors, effects, authority, limits, and hostile cases are proven;
- examples, goldens, and generated views are current;
- all callers and obsolete implementations are migrated or removed;
- correctness evidence is integrated;
- a performance baseline exists;
- no known gap remains in the declared scope.

World-class status additionally requires the strict performance and
frequency-weighted learnability evidence defined above.

Execution status, active cards, blockers, evidence logs, and wave progress live
in Tower, not in this document.
