# Whole-program reasoning must survive mixed-language work

[Master report](index.md) · [Learning and tools](03-comprehension.md) · [Compiler assurance](04-trust.md)

A language earns adoption when a team can use it inside its existing project without losing control of types, builds, debugging, failures, or ownership. Jet should make those boundaries inspectable, not pretend they vanish.

This report directly answers question 13 and the owner's foreign-language interoperation and conversion additions. It distinguishes calling existing foreign code, replacing one module, and converting source. It does not propose current Jet-version migration machinery.

<a id="q13"></a>
## Q13. Use one chain of reasoning from expression to deployment

**Direct answer.** Teach and expose the same semantic facts at each scale. Locally, explain values, state, and failure. Across functions, explain contracts and effects. Across modules, explain ownership and dependencies. At runtime, explain order, identity, resource use, and observations. At an FFI boundary, show which facts are checked by Jet and which are promises about the foreign implementation. Preserve that chain in every domain and tool.

| Scale | Developer's question | Required visible answer | Evidence or mechanism | Limit that must remain visible |
|---|---|---|---|---|
| Expression | What does this produce or fail with? | Type, value relation, numeric rules, failure alternatives | Checked expression facts and exact source span | A value prediction may depend on unknown input. |
| Statement and branch | What changes, and which path is possible? | Mutation, moved values, branch facts, unreachable paths | Ownership/state/fact analysis | A branch fact lasts only while its premises hold. |
| Function | What may callers rely on? | Inputs, result, failure, effects, ownership, resource/lifecycle contract | One checked signature and body obligations | The body can be unavailable at a foreign boundary. |
| Module | Which state and invariants belong together? | Public contract, hidden state, initialization/cleanup, imports | Semantic module identity and dependency graph | Visibility alone does not prove an invariant. |
| Program | What starts, waits, runs concurrently, and stops? | Entry/task tree, effect paths, exit and cancellation behavior | Shared operations and runtime trace | Dynamic inputs create many possible schedules. |
| Build and package | Which source and tools produced this artifact? | Resolved source/dependency/target/toolchain identity | Package model, build plan, receipts | A digest establishes identity, not correctness. |
| Execution mode | Will changing the mode change behavior? | The same allowed observations, or an explicit unsupported verdict | I9 contract and cross-mode qualification | No fresh current-tree execution was established here. |
| Foreign call | Which side owns data, errors, and callbacks? | Layout, lifetime, mutation, encoding, target and error policy | Binder descriptor plus checked overlay and provenance | Native behavior outside the descriptor is an assumption, not proved safe. |
| Domain | Does this satisfy the actual task? | Domain quantities, states, performance and acceptance oracles | Domain types plus matched examples and experiments | The language cannot infer an unstated physics or business rule. |
| Deployment and operation | Can it start, recover, and be diagnosed on the supported host? | Installation/launch prerequisites, artifact contract, observable failure and recovery | Clean-machine and real-workflow qualification | Local source checking is not deployment proof. |

This is not a proposal for ten independent analysis engines. [I3 and I9](../../../AGENTS.md) require front-end meaning and shared runtime semantics. [The architecture](../../spec/architecture.md) and editor source already point toward compiler-owned queries. The report's recommendation is to complete that ownership and expose projections of the same facts.

### Reasoning should feel the same across domains

A script's input stream, a service's request body, a game's event queue, and an embedded sensor feed all have order, consumption, failure, and lifetime. Their throughput and timing contracts differ. Teach the shared relation once, then name the domain-specific constraint.

A collection's shape, a tensor's dimensions, a database schema, and an FFI struct layout all describe data, but they are not interchangeable facts. The useful unification is a common fact mechanism with named meanings, not a universal untyped “shape” object.

The [frontier report](02-frontier.md#q8) applies this chain to eight mission areas. The [correctness report](05-correctness.md) explains which bug classes the chain can exclude and which still need a domain oracle.

<a id="foreign-contract"></a>
## Interoperation is a contract, not a namespace prefix

A successful foreign symbol lookup is the beginning of evidence, not the end. Jet's ordinary foreign call should carry the following contract in one canonical descriptor and checked overlay.

| Contract dimension | What must be known | Failure when omitted | Beginner path and expert control |
|---|---|---|---|
| Identity | Exact source/header/library/descriptor/toolchain/target | Stale bindings or incompatible archive | Ordinary bind operation resolves the declared identity; expert inspects the receipt. |
| Calling convention and layout | Width, alignment, padding, representation, ABI | Correct-looking values read from wrong storage | Generate known layout; reject unsupported ambiguity; expert supplies a checked overlay. |
| Ownership | Who allocates, moves, retains, and releases | Double release, leak, use after close | Owning handles and close behavior are explicit; no invented lifetime from a pointer. |
| Borrow/mutation | Exclusive access, noescape, read/write extent | Foreign mutation violates Jet's alias promise | Follow ratified `&` exclusive-for-the-call semantics; never call it a read-only lend. |
| Bounds and encoding | Pointer extent, count relation, text encoding | Overrun or corrupted text | Safe span/string conversion when supported; expert sees copies and conversion policy. |
| Errors | Return codes, exceptions, panics, failure payload | Exception crosses an unsupported boundary | Translate at a defined boundary; do not silently discard the native failure. |
| Callbacks and tasks | Capture lifetime, reentrancy, thread, cancellation, close | Callback after shutdown or wrong-thread access | Owner-bound callbacks with explicit unsupported cases. |
| Cost | Copies, allocations, dispatch, crossing frequency | “Convenient” loop deep-copies a foreign container | Inspectable cost evidence and a noncopying expert path where semantics permit. |
| Availability | Host/target/runtime subset and prerequisites | Compiles on one machine, fails elsewhere | Early named unsupported verdict and exact prerequisite. |
| Replacement | Which side is authoritative and what was compared | Two editable truths diverge | Foreign source remains authoritative until the adopter accepts the replacement. |

These dimensions are not new syntax proposals. Existing ratified foreign laws and cards own their implementation. The new proof ballot asks how much of Jet-controlled behavior must be checked before 1.0; it does not approve a dependency or claim arbitrary native code is proved.

### Current Jet evidence

The [FFI architecture](../../spec/architecture.md) places type, signature, and unsafe diagnostics before code generation. The inventory locates boundary preparation in [FFI.rs](../../../crates/jet-pkg-model/src/FFI.rs), C overlay/merge/link handling in [CFFI.rs](../../../crates/jet-pkg-model/src/CFFI.rs), C header generation in [CBind.rs](../../../crates/jet-pkg-model/src/CBind.rs), and routing in [Foreign.rs](../../../crates/jet-driver/src/Foreign.rs).

Static source shows useful concrete mechanisms:

- C binding results include accepted symbols, skipped symbols with reasons, handle facts, link closure, and descriptor identity. An empty bindable result is rejected rather than cached as success.
- Pointer/count pairs can map to checked slice forms. Unsupported types remain explicit. C unions are overlay-only rather than partially emitted with the wrong layout.
- User `#Import` overlays and generated `#Bindgen` modules remain distinct. The C path merges them with checked clash rules instead of asking callers to choose one of two competing libraries.
- Foreign routing derives generated modules, provenance, host, and ABI from a binder descriptor. Identity includes exact source bytes and target/tool identities.
- The [mixed-repository guide](../../spec/reference/mixed-repo.md) distinguishes native library exports from Jet-as-host JVM/JavaScript/Python adapters. Debugging marks unavailable foreign frames rather than inventing them.

None of those source observations proves current execution. Existing [#1120–#1125](index.md#interop-cards), [#507](index.md#card-507), [#1344](index.md#card-1344), and the domain binder cards remain the owners. The current fresh compiler build failure prevents qualification here.

<a id="peer-interop"></a>
## Copy the useful boundary, not the peer's hidden cost

| Primary reference | What to take | What not to inherit | Jet comparison and evidence limit |
|---|---|---|---|
| [Swift C++ interoperability](https://www.swift.org/documentation/cxx-interop/) and [status](https://www.swift.org/documentation/cxx-interop/status/) | Direct header-based access, bidirectional generated interfaces, explicit supported subset | Assuming imported collection convenience is free; unsupported exception recovery or templates hidden behind “C++ supported” | Swift documents potential deep copies in collection operations and restrictions on exceptions/templates. Jet should expose equivalent cost and subset facts. No matched benchmark ran here. |
| [Swift safe C/C++ interoperation](https://www.swift.org/documentation/cxx-interop/safe-interop/) | Lifetime/noescape and bounds annotations can produce checked safe views without changing the C ABI | Treating an unannotated native type as automatically safe | Jet should preserve known bounds/lifetimes and require an honest boundary for what cannot be expressed. Annotation truth remains an external premise. |
| [CXX](https://cxx.rs/) and [core concepts](https://cxx.rs/concepts.html) | A deliberately constrained shared regime, paired generators, static signature assertions, opaque ownership | Marketing arbitrary C++ compatibility from a supported subset | CXX's overhead claim applies to its supported regime. Jet must measure its own boundary and distinguish signature matching from semantic safety. |
| [autocxx workflow](https://google.github.io/autocxx/workflow.html) and [safety](https://google.github.io/autocxx/safety.html) | Automate repetitive header work, allow hand-written wrappers, inspect generated bindings | A project-wide safety promise that makes all generated calls look individually justified | Jet should retain per-boundary assumptions and unsupported reasons. Automatic generation is not automatic safety. |
| [Java FFM, JEP 454](https://openjdk.org/jeps/454) | Separate memory layout, lifetime arena, call descriptor, symbol lookup, and callback machinery | Inferring pointer extent from an address; hiding platform widths or restricted reinterpretation | A returned unbounded pointer has no safe extent merely because Java wraps it. Jet should preserve the same honesty. JEP performance goals are not measured Jet results. |
| [jextract](https://docs.oracle.com/en/java/javase/22/core/call-native-functions-jextract.html) | Mechanical binding generation and inspectable layout helpers | Calling platform-specific generated bindings universally portable | Generate/compare per target where layout or preprocessing differs. C++ may need a C-facing interface in that tool. |
| [.NET source-generated P/Invoke](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/pinvoke-source-generation) and [marshalling](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/type-marshalling) | Ahead-of-time inspectable conversion, explicit encodings, ownership cleanup | Confusing C# `long` with native `long`, or one platform's `wchar_t` with another's | Jet should make conversion and cleanup visible, not force runtime code generation for ordinary boundaries. |
| [Kotlin/Native C interoperation](https://kotlinlang.org/docs/native-c-interop.html) | Generated IDE-visible mappings, lexical allocation scopes, explicit callback constraints | A temporary pointer escaping its scope or a callback throwing across C | Jet's lifetime and failure contracts should make these limits predictable before execution. |
| [C2Rust](https://c2rust.com/manual/) and [cross-check tutorial](https://c2rust.com/manual/docs/cross-check-tutorial.html) | Preserve original behavior first; compare traces during replacement | Equating translation with safe idiomatic output, or a few equal returns with full equivalence | Jet should keep unsupported/non-equivalent regions visible and compare domain observations, not promise whole-language conversion. |

The common lesson is that convenient calling syntax is earned by a detailed boundary underneath. Hiding repetitive ceremony is good. Hiding a copy, ownership assumption, unsupported method, or unproved error path is not.

<a id="foreign-conversion"></a>
## Foreign source conversion stays narrow and source-authoritative

The owner asked about entering Jet from existing languages. That is separate from migrating one Jet version to another. The former is in scope. The latter is not required during this prerelease campaign.

The ratified [migration tier map](../../spec/reference/migration-tier-map.md) already makes the important product choice: **bind in place broadly; source-convert only an explicit subset; replace modules with differential evidence.** Do not reopen it merely because another tool can emit a large amount of target code.

| Source language | Current documented product boundary | Supported conversion shape or binding | Explicit non-equivalence/limit |
|---|---|---|---|
| Python | Source importer plus `py.<lib>` sidecar binding | Annotated scalar functions, straight-line locals/returns/calls/expressions/assertions | Dynamic/untyped/compound values, imports, nested control, and exceptions stay source-owned. |
| Java | Source importer plus JVM binding | Public static scalar methods in the declared Java 8–21 subset | Objects, instance state, overloads, generics, reflection, exceptions, async remain foreign. |
| C# | Source importer plus .NET binding | Public static scalar methods in the declared C# 10+ subset | Managed state, overloads, generics, LINQ, reflection, exceptions, async remain foreign. |
| TypeScript | Source importer plus JavaScript runtime binding | Typed scalar functions; numeric literals follow the documented Float mapping | Erased types, unions, objects, imports, promises, decorators and generics are not converted generally. |
| JavaScript | Source importer plus JavaScript runtime binding | Statically recognizable scalar functions/arrows | Dynamic property/prototype behavior, closures, promises, modules and ambiguity require an explicit omission. |
| Go | Source importer plus C-archive binding | Scalar straight-line functions, including the documented entry mapping | Methods, interfaces, goroutines, channels, errors, multiple returns, packages and pointer state remain source-owned. |
| Ada | Binder-only, source-preserving manual replacement | GNAT C-ABI package exports; `jet import ada` writes a binder stub | No semantic source conversion of ranges, representation clauses, tasking, ownership, or exceptions. |
| Pascal | Binder-only, source-preserving manual replacement | FreePascal cdecl exports and opaque handles; `jet import pascal` writes a binder stub | Classes and runtime ownership are not source-converted. |
| VBA/Office | Binder-only manual replacement | Windows COM/IDispatch type-library binding | Workbook state, macros, source modules and unsupported automation remain source-owned. |
| C | Binder-only, checked overlay replacement | Supported C ABI/header subset | No source importer; pointer semantics, preprocessing and undefined behavior are not converted. |
| C++ | Binder-only, checked overlay replacement | Supported Clang/C++ boundary subset | No source importer by deliberate decision; templates, layout, exceptions and undefined behavior are not magically translated. |

This table is a static product/source map, not a fresh command transcript. A second source-only read checked [CmdImport.rs](../../../Source/CmdImport.rs) against the map:

- C and C++ dispatch explicitly reject source import and direct the user to binding and checked overlays.
- Ada and Pascal dispatch accept the import command but emit a binder stub, a `JT0101` omission, and a statement that foreign source remains canonical. They report zero translated functions/tests in that path.
- The command and map agree: acceptance prepares a binder stub, not semantic Ada/Pascal conversion. There is no observed implementation/map contradiction in this path. “Semantic Ada source translation is supported” would overstate this evidence; merely naming the existing import command would not.

[#2927](index.md#card-2927) owns the confirmed release-wording contradiction and a general claim census. Ada/Pascal belongs in that census as a consistent classification boundary, not a second observed defect. [#1156](index.md#card-1156), [#989](index.md#card-989), [#990](index.md#card-990), and [#1346](index.md#card-1346) remain the implementation and map owners. Historical decisions D-ADOPT-TIER1 and D-MIGRATE-SRC1 remain unchanged.

### A safe replacement sequence

1. Keep the foreign source authoritative and call it through a checked boundary.
2. Choose one module whose inputs, outputs, side effects, failure, and resource behavior can be observed.
3. Convert only the supported subset or write ordinary Jet manually. Preserve every omission and provenance record.
4. Run the original and replacement with controlled equivalent inputs. Compare the required observations, including failure and timing/ordering where part of the contract.
5. Separate nondeterminism from translation differences. Running each variant against itself can reveal an unstable oracle.
6. Accept the Jet module only when the adopting team owns the equivalence evidence. Do not delete the foreign source automatically.
7. Repeat at a genuine module boundary, not by keeping two indefinitely editable generated truths.

C2Rust's trace-based work is useful precedent, but no finite test corpus proves equivalence for arbitrary foreign code. Proved or restricted translations can support stronger claims within their model. Undefined foreign behavior has no stable meaning to preserve until the adoption contract chooses one explicitly.

<a id="portable-tools"></a>
## Portable workflow first, optional studio second

The proposed Jet experience has no AI dependency. A person, editor, CI process, or coding agent should consume the same deterministic commands and structured evidence.

Current documented command surfaces include:

- `jet inspect bind` for binding inspection/generation, with language-specific inputs and explicit skipped declarations.
- `jet build --lib --locked --output <manifest-output> <entry.jet>` as the [foreign build-host contract](../../spec/reference/foreign-build-hosts.md) for CMake, Gradle, Bazel, and MSBuild adapters.
- `jet cc` and `jet c++` as the [declared C/C++ toolchain driver](../../spec/reference/cc-driver.md), not an invitation to silently pick a host compiler.
- `jet check`, `jet build`, `jet test`, `jet run`, `jet perf`, and `jet debug` as the [mixed-repository daily loop](../../spec/reference/mixed-repo.md).

The angle-bracket command is a documented invocation template, not a command executed in this campaign. Host/target availability still requires exact prerequisites and a real launch receipt.

An optional visual studio should display the same build graph, boundary contract, source identity, effects, call chain, and evidence states. It must not own a separate package model or become necessary to run a build. The [learning report](03-comprehension.md#q17) gives the visual design and acceptance rules.

### The competitive and agent lens

Jet beats a peer here only when the same mixed-language job needs less manual glue, has a more accurate verdict, and meets its performance/portability contract. Count copied boundary bytes, allocation/call overhead, unsupported API coverage, rebuild scope, and the time to a correct diagnosis. Do not rank a binder by the number of names it emits.

For coding agents, preserve five quantities: verdict fidelity, verdict latency, repair actionability, context economy, and repair determinism. A concise record with an exact unsupported signature and source span is better than either a giant generated file or a falsely successful stub. Humans benefit from the same structure.

## Generalize every finding

| Finding | Defect shape | Predicted other instances | Structural correction | Card disposition |
|---|---|---|---|---|
| Foreign namespace mistaken for safe support | Binding name hides contract boundary | Every binder family, callbacks, imported layouts | Canonical descriptor plus checked assumptions and unsupported verdict | Reuse #1120–#1125/#507; research #2925 |
| Convenient iteration hides copying | Surface changes cost without changing visible intent | Foreign containers, Core views, marshalled arrays | Cost receipts, copy census, matched workload | Reuse #2858/#2890/#2899 |
| Command acceptance can be mistaken for semantic conversion; inspected Ada/Pascal bounds are consistent | Dispatcher presence alone cannot establish a wider capability | Stub emitters and partial generators | Distinguish binding, stub preparation, conversion, and accepted replacement | Classification obligation, not another observed bug; reuse #1156/#989/#990/#1346 and #2927 claim census |
| Generated file becomes a second truth | Provenance lacks ownership transition | Bindings, converters, headers, build artifacts | Source-authoritative generation and conflict-before-write | Preserve D-MIGRATE-SRC1; reuse #1156/#2506 |
| Portable source mistaken for portable artifact | Target/tool/layout premise omitted | C widths, native libraries, host adapters | Target-specific descriptor and clean-machine proof | Reuse #1344/#2334–#2343/#2919 |
| Editor or studio re-explains semantics | Tool maintains independent language meaning | Hover, graph, debugger, generated help | Project compiler-owned facts, with identity and honest unavailable state | Reuse #2389/#2505/#2506/#2902/#2906 |
