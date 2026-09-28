# Native Jet CoreLib contract

This specification is for Core maintainers, provider authors, and reviewers. It
defines where Core owns semantics, where a host may provide a typed leaf, how
capabilities and effects are exposed, and what evidence makes an export
complete. The declaration registry is
[`Core.jet`](../../crates/jet-codegen/src/Prelude/Core.jet); Jet source bodies
are under [`Core/`](../../Core/). Generated call and module views are derived
from those inputs by the [Core table generator](../../Tools/agent/gen-core-tables.mjs).
The [surface ledger](../../tests/core_surface_ledger.rs) checks selected
identity rows; runnable provider examples live under
[`Examples/features/`](../../Examples/features/).

The Rust-hosted compiler under `Source/` and `crates/` remains the production
and reference compiler. The default CLI continues to use that reference path.
The staged Jet-authored compiler under `Compiler/` may implement the same
contracts in bootstrap scopes, but this document does not claim that Jet is
self-hosted. Rust emission, rustc/LLVM, and Cranelift remain supported target
and execution seams. No Core contract is defined as permanently owned by a
particular compiler implementation.

## Purpose and scope

Core modules own public behavior. A Core API must be useful for real programs,
safe by default, explicit about effects and authority, and complete enough to
serve its intended domain. The contract applies to every module row, exported
function, exported type, root type, format row, namespace-only row, and
explicit dispatcher row declared in `Core.jet`.

Each declaration has a meaningful role. A metadata-only declaration identifies a
compiler or runtime fact and has a generated consistency check; it cannot hide
missing callable behavior. An exported operation must not silently return an
empty value, report false success, discard data, manufacture deterministic
entropy in place of required entropy, or use an unimplemented fallback.

Core expands through a dependency graph and a usage mission, not a blind union
of peer libraries. High-value capabilities follow the graph and the real jobs
that need them. A scope may state explicit non-goals, but a declared export
still needs a contract, an error meaning, and evidence appropriate to its role.

## Source authority and generated projections

`Core.jet` is the authority for Core's bootstrap dependencies, module rows,
exports, root types, namespace-only declarations, format rows, and explicit
native-dispatch metadata. A `source_module` row names the canonical Jet source
path and the members that it owns. The corresponding `Core/**/*.jet` file owns
public algorithms, validation, defaults, policy, effects, authority, typed
errors, and composition.

The generator must verify that:

- every declared export has one canonical Jet source or an explicit registered
  provider/leaf role;
- generated module and call views agree with the declaration source;
- duplicate mechanisms, unapproved aliases, shims, and obsolete projections do
  not create a second public identity;
- effect, authority, capability, fallback, signature, and tier metadata remain
  consistent.

The generated Rust tables are projections, not a second API catalog. Never
hand-edit them. The main generated views are
[`CoreModuleExports.rs`](../../crates/jet-foundation/src/CoreModuleExports.rs),
[`core_calls.rs`](../../crates/jet-foundation/src/Syntax/core_calls.rs), and the
corresponding generated projections in the Prelude. Rust implementation files
such as [`Core.rs`](../../crates/jet-codegen/src/Prelude/Core.rs),
[`PortableCore.rs`](../../crates/jet-codegen/src/Prelude/PortableCore.rs), and
[`CoreLib/`](../../crates/jet-codegen/src/Prelude/CoreLib/) provide registered
leaves and adapters; they do not become an independent public API registry.

## Jet semantics and native leaves

Jet owns the parts that give an operation its meaning:

- algorithms and composition;
- parsing, validation, bounds, and normalization;
- safe defaults and user-facing policy;
- effects and authority requirements;
- typed error meaning and diagnostics;
- data transformation and public API behavior.

A native provider may supply a typed, policy-free leaf. Such a leaf has a narrow
ABI, no user-facing default selection, no multi-step application semantics, and
no backend-specific interpretation of a Jet error. It may expose an operating
system service, an unsafe platform operation, a vetted cryptographic primitive,
a database or network transport, a font shaper, or a hardware interface.

The surrounding Jet function remains the semantic boundary. For example,
`core.files` keeps path composition, walking, limits, and error mapping in Jet
while its typed provider performs the OS file operation. `core.http` keeps
method, header, framing, limits, and deterministic convenience behavior in Jet
while the host supplies a typed transport leaf. `core.crypto` keeps typed
values and fail-closed policy in Jet; the OS CSPRNG is a host cell. The raw
algorithm controls in `core.crypto.expert` remain an explicit audited door, not
a hidden provider policy.

A high-level native implementation must not be retained as an invisible runtime
fallback for a Jet implementation. If an API changes identity or semantics,
make the change at the contract boundary and migrate its users in one cutover.

## One meaning across execution tiers

AOT, resident JIT, interpreter/comptime evaluation, and web execution are
points on one semantic continuum. Preparation and runtime cost may differ, but
the following observable contract does not:

- validation and bounds;
- defaults and policy;
- effects and authority;
- typed errors and diagnostics;
- resource limits and cancellation;
- deterministic behavior and observable values.

Every applicable tier consumes one semantic Core call record and one provider
contract. The tier adapter supplies representation and execution mechanics; it
does not reimplement Core policy. The shared call record names the signature,
fallibility, effect, capability, provider route, pure/interpreter route, and
AOT/JIT adapter facts so that a tier cannot quietly invent a second meaning.

A portable fallback is valid only when it preserves the same contract. A web or
test provider is explicit when it changes the host resource, and its typed
availability or policy error remains visible to the caller. Cross-tier evidence
compares semantic values and typed errors, not merely whether each adapter
returned.

## Provider classification and policy

The canonical call record classifies each host-sensitive operation with one
capability kind:

| Kind | Contract |
| --- | --- |
| `Pure` | The operation needs no ambient provider, fallback, or capability failure. |
| `NativeRequired` | A named provider is required; no semantic fallback is valid, and absence has a typed failure. |
| `PortableFallback` | A named provider has a canonical portable implementation and an optional native acceleration; the failure identity is shared. |
| `ExplicitSimulation` | A named virtual or test provider is selected explicitly; it is not an ambient production substitution. |

The `CoreCapabilityContract` records the provider, fallback identity when one
exists, and typed failure identity. `Effect` and its precise effect leaf remain
the authority for effect checking; capability metadata does not create a second
effect vocabulary or permission system.

Safe defaults and an `auto` selector may choose a semantics-preserving portable
fallback. An explicit provider request never silently downgrades. When a
required capability is missing, every tier returns the same typed,
deterministic capability error.

Examples establish the intended distinction:

- `core.compute` uses its checked CPU oracle for `auto`; an explicit CUDA,
  Metal, Vulkan, or WebGPU request reports a typed device or unsupported error
  rather than silently using the CPU.
- `core.font.shape` requests the font-shaping provider, while
  `core.font.shape_with` is an explicit deterministic approximate fallback.
- `core.crypto.random` uses the operating-system CSPRNG. A missing or rejected
  provider returns `CryptoError.Unavailable`; it never falls back to a weak or
  deterministic generator.
- Virtual filesystems, fake clocks, in-memory databases, and test networks are
  named providers selected by the caller or test tool, not hidden production
  substitutions.

Effects and authority are the user-visible access boundary. Provider objects
and capability metadata exist for explicit expert control and provider
machinery, not as a second permission system.

## API identity and migration

A first comparison pass preserves valid semantics and a coherent identity closely
enough for direct semantic comparison. It does not preserve a misleading API
shape merely because another language has one.

When an export is incoherent, replace it at the contract boundary. Migrate every
caller, example, test, registry row, generated consumer, document, and affected
environment variable in one clean cutover. Delete runtime compatibility
wrappers, aliases, parallel parsers, duplicate implementations, and hidden
fallback paths.

A later design pass may improve names, types, defaults, diagnostics, algorithms,
allocation, or performance when the new Jet contract has proof. Material public
choices are owner-gated before implementation. Peer languages supply semantic
and edge-case evidence; Jet still uses its own types, effects, authority,
diagnostics, and ergonomic surface. Common operations have low-friction safe
defaults, while expert control is explicit rather than hidden.

## Contract and evidence

Before code, give every module or coherent module cluster a contract record that
freezes:

- public API and types;
- semantics, bounds, and edge cases;
- effects and authority;
- capability kind and fallback policy;
- error and diagnostic codes;
- peer references and valid baseline receipts;
- examples and goldens;
- hostile and failure cases;
- the applicable tier matrix;
- frequency and hot-path cells;
- explicit non-goals.

Every export receives evidence scaled to its role but complete for its declared
surface. Cover normal behavior, boundaries, hostile inputs, typed errors,
diagnostics, authority and effects, resource limits, determinism, concurrency or
reentrancy where relevant, and cross-tier parity.

Use semantic fixtures for first-pass comparison. A coherent surface may compare
directly with valid Rust behavior. A redesigned surface maps valid Rust inputs
to the new Jet contract and compares semantic values and errors. Do not keep a
runtime compatibility layer merely to make that comparison.

## Dependency order

Core work follows the dependency graph. These categories describe dependency
order, not a progress ledger:

1. **Foundations:** root carriers, text, collections, math, units, time,
   encoding, regex, args, files and path contracts, memory and I/O foundations,
   and portable cryptographic primitives.
2. **System:** archives, networking, process, tasks, events, watchers, terminal,
   logging, email, and database.
3. **Data and web:** data loaders and transformations, HTTP, web/router/forms,
   reactive, synchronization, services/jobs, storage, and browser.
4. **Specialized:** compute/models, UI/TUI/font, games/raylib, plugin/mod,
   compiler/reflect, testing, and remaining host domains.

Within a category, high-use foundations lead according to the dependency graph.
For each coherent area:

1. owner-gate material contract choices;
2. freeze valid baseline behavior;
3. implement Jet semantics in the approved portable Jet subset;
4. implement the shared provider and approved typed leaf seams;
5. prove each applicable tier and failure surface;
6. migrate all callers and generated consumers;
7. remove obsolete high-level implementations and placeholders;
8. close correctness and record a performance baseline;
9. optimize only after correctness closure;
10. rerun parity and strict performance cells.

## Performance and learnability

The performance gate is strict per matched workload and metric. No aggregate
score hides a loss. Hot paths and representative workloads carry peer cells for
latency, throughput, allocation, copying, startup, compilation, and artifact
size where applicable. No performance claim extends beyond measured coverage.

A performance-motivated surface has paired plain and optimized programs. Optimize
safe Jet code first. Audited native or unsafe leaves are explicit expert paths
and preserve the same contract.

Use a layered, frequency-weighted corpus of examples, conformance cases,
repository usage, representative beginner tasks, and domain workloads.
High-frequency APIs receive paired beginner and expert evidence for source size,
diagnostics and recovery, preparation cost, and runtime cost.

## Completion criteria

A CoreLib area is complete only when:

- Jet owns its public behavior;
- every declaration has a meaningful contract;
- every applicable tier shares one observable meaning;
- capability policy is explicit;
- errors, effects, authority, limits, and hostile cases are proven;
- examples, goldens, and generated views are current;
- callers and obsolete implementations are migrated or removed;
- correctness evidence is integrated;
- a performance baseline exists;
- no known gap remains in the declared scope.

A stronger comparative claim additionally requires the strict performance and
frequency-weighted learnability evidence above. Execution order, active work
records, blockers, and evidence logs belong in Tower rather than in this
specification.
