# Jetpack: our portable application platform

> **Status:** **DESIGN PROPOSAL — PROPOSED and UNRATIFIED.** Jetpack is the product described here. This document is the canonical Markdown technical specification and proposal; the platform is not built or measured by this document. It is not current runtime law, an implementation report, a benchmark, a security proof, or a Tower decision. No permission, default, command registration, compiler/CoreLib behavior, host ABI, plugin lifetime, or package authority changes by publication of this document.
>
> **What the product is:** Jet is the language/compiler foundation. Jetpack is the proposed platform for building, sharing, running, inspecting, and updating applications. New portable Jetpack applications would use Jetpack's checked engine and host broker by default rather than requiring Docker, a JVM, or a third-party Wasm runtime as their native-host execution layer. Browser and foreign-application adapters remain explicit exceptions, not the hidden core product.
>
> **Technical names:** The portable instructions are **Jet bytecode**. On first technical use, `JPM/1` means **Jet Portable Machine, version 1**, the contract for those bytes; `JPH/1` means **Jet Portable Host, version 1**, the typed rules by which an application requests services from its computer. `/1` is a contract version, not a product or Jet source-language version.
>
> **Evidence date:** Source inspection represented here is dated **19 September 2026**. It cites primary specifications, official documentation, and repository paths. No runtime/performance/conformance/security result is claimed unless an observed result is explicitly identified; the present document contains none. HTML is a later short guide, not a second specification authority.
>
> **Owner goal:** Evaluate and, if ratified, implement one Jetpack-owned portable application journey: source and exact dependencies, Jet bytecode, the Jetpack engine, JPH/1 host policy and broker effects, and the existing package/lock/Hangar/receipt graph for trust, lifecycle, and deployment. Foreign formats remain explicit projections or adapters.
>
> **Legend:** **SOURCE-INSPECTED** means a current source/specification/documentation fact, not execution. **PROPOSED** means a normative recommendation awaiting owner ratification. **UNMEASURED** means the required runtime, benchmark, security, or conformance observation has not been made. Existing draft labels `CURRENT LAW`, `EVIDENCE`, `ANALYSIS`, and `UNKNOWN` are interpreted under this legend. Requirement IDs use disjoint prefixes: `M-` machine, `H-` host, `P-` package, and `C-` comparison; they are proposed requirements, not registered Jet diagnostics or Tower decisions.

## Contents and suggested reading paths

### Start here

- [Product overview](#product-overview)
- [Application journey](#application-journey)
- [Permission change](#permission-change)
- [Failure guide](#failure-guide)
- [Name map](#name-map)

### Technical chapters

1. [Summary](#summary)
2. [Goals, replacement boundary, and comparison law](#goals)
3. [Terminology](#terminology)
4. [Existing contracts](#existing-contracts)
5. [Architecture](#architecture)
6. [Artifact format](#artifact-format)
7. [Instruction set](#instruction-set)
8. [Verification](#verification)
9. [Memory model](#memory-model)
10. [Execution tiers](#execution-tiers)
11. [Host ABI: JPH/1](#host-abi)
12. [Capabilities and authority](#capabilities)
13. [Host service registry](#host-services)
14. [Resource control and hard termination](#resource-control)
15. [Isolation, trust, and residual risk](#isolation)
16. [Package graph](#package-graph)
17. [Owned source](#owned-source)
18. [Supply chain](#supply-chain)
19. [Lifecycle](#lifecycle)
20. [Distributed operation](#distributed-operation)
21. [Compatibility and host-platform profiles](#compatibility)
22. [Developer experience](#developer-experience)
23. [Worked examples](#worked-examples)
24. [Competitors and incumbent strengths](#competitors)
25. [Downside ledger](#downside-ledger)
26. [Conformance vectors and malicious cases](#conformance)
27. [Performance protocol and gates](#performance)
28. [Alternatives and same-program trade-offs](#alternatives)
29. [Decisions and owner-only amendments](#decisions)
30. [Adoption boundary](#adoption)
31. [Sources and provenance](#sources)
32. [Coverage and remaining unknowns](#coverage)

**Start here:** [Product overview](#product-overview) -> [Application journey](#application-journey) -> [Permission change](#permission-change) -> [Failure guide](#failure-guide) -> [Name map](#name-map). These are explanations of the contracts below, not a second normative schema.

**Suggested paths:** New readers should start with the five sections above, then follow Summary -> Goals -> Architecture -> Artifact format -> Host ABI -> Package graph -> Worked examples -> Conformance. Implementers should read Artifact format -> Instruction set -> Verification -> Memory model -> Execution tiers -> Host ABI -> Resource control -> Compatibility. Owners evaluating adoption should read Summary -> Competitors -> Downside ledger -> Conformance -> Performance -> Alternatives -> Decisions -> Adoption.

<a id="product-overview"></a>
## Product overview

**Jetpack is the product.** The proposal is for one Jet-powered platform that lets a developer build an application, share its exact program and dependencies, run it on another compatible computer, inspect what it did, and update it without losing the old version's identity. Jet is the language and compiler foundation; Jetpack owns the portable application record, checked engine, host-service boundary, permissions, and lifecycle history. The proposal is not built or measured yet.

At the user level, the record answers three questions: which exact source and library versions are being run, which outside access the application is allowed to request, and what action history follows it through run, inspection/debugging, and update. The technical graph, lock, authority, store, receipt, and generation records below implement those roles; they are not five new things a beginner must author. A developer ships a Jetpack application record containing Jet bytecode, exact required library versions, requested host services and limits, and source/debug links. The receiving computer needs a compatible Jetpack runtime and the services the application declares. It verifies integrity, bytecode, required features and limits, and local permission policy **before** starting. A missing mandatory capability or enforcement backend means refusal, not silent weaker execution and not a trusted-native fallback. The user's photographs, host-local paths, secrets, a whole guest OS, and arbitrary native compatibility are not automatically inside the portable record.

### The three jobs Jetpack is trying to combine

Docker, WebAssembly (Wasm), and the Java Virtual Machine (JVM) are useful mechanisms with different jobs. WebAssembly System Interface (WASI) is a standard way for Wasm programs to request outside services; the WebAssembly Component Model describes how separately built components exchange typed values and services. This proposal keeps their strengths instead of claiming that a new format makes them disappear:

| incumbent mechanism and job | strength Jetpack keeps | proposed Jetpack replacement or benefit | remaining cost and proof required |
|---|---|---|---|
| **Docker/container ecosystem:** package an application with much of the environment it expects and operate isolated instances | repeatable versions, separate data, controlled rollout, and mature image operations | a portable Jetpack record carries the program, exact dependencies, requested services/limits, source/debug identity, and running generations without making a general Linux userland/syscall image the default portable path | host runtime, libraries, application data, adapters, migration, and operations still exist. Image/install/start/memory/operations savings are unknown until equal-work measurements; unchanged foreign native applications still need their existing image/process path |
| **WebAssembly (Wasm):** distribute portable instructions behind a checked boundary and language-neutral host calls | verifiability, explicit host calls, browser reach, and mature Component Model/WASI ecosystems; WASI standardizes outside-service requests and components define typed values/services between separately built parts | Jetpack owns its native-host Jet bytecode/engine and makes package/source identity, host permissions, debugging, and update lifecycle one product contract instead of requiring users to assemble those pieces around an independent runtime | a new verifier, engine, tooling, security corpus, and JPH adapter burden is real. The strongest Wasm/component path remains the paired comparison and hard gate; browser execution still uses browser-approved machinery |
| **Java Virtual Machine (JVM):** run Java and other managed applications with mature compilation, services, debugging, and optimization | portable execution, useful optimization, source inspection, and mature Java libraries and runtimes | Jetpack's core does not require every application to adopt a Java-style class/object/collector model. Jet's shared libraries define Jet behavior; a language-specific managed runtime is carried only when an adapted program needs it | Java, .NET, JavaScript, and Erlang runtimes retain automatic memory cleanup (garbage collection), runtime type inspection (reflection), startup, compatibility, and library costs; none disappear. JIT/AOT code produced from verified Jet bytecode remains inside the same sandbox; directly loading a foreign native library is a separate explicit trust transition |

This is a structural proposal, not a measured universal win. [Competitors](#competitors), the [downside ledger](#downside-ledger), and the [performance gates](#performance) keep incumbent strengths, residual costs, and falsifiers visible. If Jetpack's owned format does not earn its verifier/engine/tooling cost against the strongest standard substrate, the format recommendation must be rejected or narrowed.

The central ownership split is intentionally simple: a confined **worker** runs application state, memory, tasks, and typed references; a **host broker**—Jetpack's host-side software, normally a separate service or process—is the part allowed to touch files, networks, devices, and OS handles. The broker is software, not a person or an ambient grant. JPH/1 is the typed request/result boundary between them. A package can request access but cannot grant access to itself. JIT and AOT compilation of verified Jet bytecode may produce native machine instructions while remaining inside the selected sandbox; loading an unrelated foreign native plugin/helper is a different trust and containment transition.

<a id="application-journey"></a>
## Application journey: one image filter from source to result

**PROPOSED EXAMPLE, not an observed run.** This illustration starts with a compatible Jetpack runtime already installed and the package delivered; file transfer or registry retrieval is transport, not a permission grant. It claims no shipping installer or exact CLI spelling. If preflight finds a missing runtime, service, or enforceable limit, it reports that missing prerequisite and leaves the application unstarted. Use one application throughout this overview: `image-filter` reads images from a logical input folder named `input`, inverts each 8-bit RGB channel with `255 - c`, and writes converted images to a logical output folder named `output`. A pixel `(10,20,30)` is expected to become `(245,235,225)` when the example completes. No generic image codec, GPU library, screenshot, receipt, or speed result is claimed.

1. **Author.** The developer edits the application and its source dependencies. A source dependency is code brought into the project; its editable copy is project-owned by default under the proposed `.jet/deps/` convention, not disposable cache.
2. **Build.** The compiler checks source and produces Jet bytecode plus one Jetpack application package/record: exact library versions, requested host services and limits, and source/debug links. The portable record does not silently include the user's photographs, host-local paths, secrets, a guest OS, or arbitrary native compatibility.
3. **Send.** The package is sent to a second computer. That computer needs a compatible Jetpack runtime and the declared services. It checks integrity, bytecode, required features/limits, semantic identity, and local policy before starting. Missing support refuses activation rather than selecting weaker semantics or a trusted-native fallback.
4. **Authorize local folders.** The receiving operator maps logical `input` and `output` to chosen local folders and approves only input read and output write, plus any separately declared ordinary I/O. The portable program contains logical names, not host paths. The [permission-change](#permission-change) projection shows the editable host setting.
5. **Run.** The worker executes the checked program. The broker resolves the approved roots and checks each file operation against the granted scope and remaining budget. The worker cannot pass a host pointer or invent a grant. The expected converted image appears in `output`; inspection can trace it to the exact program/source/permissions and action record.
6. **Denied request versus trap.** If this application attempts a network request without permission, the host returns a typed `denied` result; signing the package does not grant network access, and the application may handle that ordinary error. A machine safety/limit trap is a separate hard-containment path that stops the whole isolated instance and its tasks. Neither path promises to undo external writes already completed.
7. **Update.** A new version stages beside the old usable version. A source conflict, activation/readiness failure, or external-effect-unknown outcome follows the distinct branches in the [failure guide](#failure-guide); code rollback selects an older program version and is not database/file/network rollback.

The machine, host, graph, and lifecycle contracts behind this story remain below. This front section gives the program journey first so the wire names do not masquerade as the product.

<a id="permission-change"></a>
## Permission change: edit one host path without inventing a config file

This is the proposed receiving computer's app-permission screen/flow, not current `package.jet` syntax, a new configuration file, or a registered CLI command. The operator chooses the input-folder entry, selects `/work/images`, and approves read access for the next activation. Exact UI labels and CLI spelling remain unratified.

The Before/After blocks below illustrate that selection, not source or configuration text to paste:

```text
host_settings_projection {
  input:   { host_path: /work/input,  access: read  },
  output:  { host_path: /work/output, access: write },
  network: denied
}
```

The operator's proposed selection is:

```text
host_settings_projection {
  input:   { host_path: /work/images, access: read  },  # changed field
  output:  { host_path: /work/output, access: write },
  network: denied
}
```

Only the input selection changes. The host checks `/work/images`, obtains a fresh permission decision, and records that decision for the next activation. Output and network remain unchanged; a sibling folder requires a fresh grant.

Internal permission records are generated by the host; see the [host permission contract](#capabilities).

<a id="failure-guide"></a>
## Failure guide: “update failed” is not one answer
Cutover is the handover from the old version to the new: changing where requests go, who may write, or the shared data the version uses. These changes can happen separately. Reconcile means checking the host/service's recorded and actual state before trying again.

| situation | what remains true | next safe meaning |
|---|---|---|
| Source conflict or concurrent edit | The existing editable source and active lock remain visible; the staged candidate is kept for resolution and is not silently installed. | Resolve or discard the candidate. A backend without safe source snapshot/write exclusion refuses automatic replacement. |
| New version fails verification, readiness, or pre-cutover handover | Before any handover, the old version and shared targets remain unchanged. If a change may already have happened, preserve old code, report uncertainty, and restrict further conflicting work; never claim the old service is live/writable without checking the real target. | Before handover, inspect/repair/rebuild. After handover, check which version gets requests, which may write, whether the selected version can use the current data format, and whether its permissions are still valid; follow the [lifecycle recovery rules](#lifecycle). |
| Remote route, volume, secret, or device change is uncertain | The new candidate is fenced/quarantined; old code is retained, but service/target status is unknown until host state is checked. There is no automatic rollback or old-live promise. | Reconcile the actual target, then complete or restore only after checking request routing, writer authority, data format, and permission state; follow the [lifecycle recovery rules](#lifecycle). |
| External migration committed but its reply was lost | The outcome is **unknown**, not a clean failure. Data/file/network effects may already exist. | Reconcile the external effect before retry, restore, or migration rollback. Do not blindly roll back. |
| Code rollback | An older program generation is selected again. | This changes code selection only; it does not undo database, file, network, secret disclosure, or device effects already committed. |

Here, **quiescence** means proof that old work really stopped using a resource; a **storage fence** means backend enforcement that prevents old writers from committing. Local safety, already-committed or unknown outside effects, and remote uncertainty remain separate. Exact lifecycle records and recovery rules are below.

<a id="name-map"></a>
## Name map: words before wire notation

| plain word | meaning here |
|---|---|
| **Jetpack** | The proposed product/platform for building, sharing, running, inspecting, and updating applications. |
| **Jet** | The language and compiler foundation used by the product; not a second runtime product. |
| **application/program** | Instructions that perform the user's task. **Source** is the human-editable form; a build turns checked source into executable instructions. |
| **portable** | The same defined program can run on supported computers with a compatible Jetpack runtime and declared services. It does not mean every old binary or every device runs unchanged. |
| **Jet bytecode** | Portable instructions for the Jetpack engine. `JPM/1` expands to **Jet Portable Machine, version 1**, the technical contract for those bytes. |
| **host interface** | The exact request/result agreement between an application and host services. `JPH/1` expands to **Jet Portable Host, version 1**; **ABI** means application binary interface, the exact data and call agreement. |
| **runtime/engine** | Software that actually runs the instructions. Jetpack's engine is distinct from an optional language-specific runtime used by an adapted JVM/CLR/JS/BEAM program. |
| **sandbox** | Enforced restrictions on memory, outside access, and resource use: worker checks, broker checks, and the selected OS/process/VM containment boundary together. It is not only a permission list; kernel, administrator, driver, and hardware compromise remain outside the guarantee. |
| **native** | Machine instructions for one CPU/OS. The Jetpack engine may compile verified Jet bytecode into native instructions without leaving the sandbox. A foreign native plugin/helper is a separate trust/containment transition. |
| **tier** | One way to execute the same program meaning, not another product: an interpreter runs portable instructions directly; JIT (just-in-time) compilation translates while running; AOT (ahead-of-time) compilation translates before running. Browser and embedded are host profiles as well as possible paths, not permission to change meaning or security. |
| **package/artifact** | A package is the application record tying its program to exact dependencies and requirements. An artifact is a produced payload/file plus the metadata needed to identify and check it. |
| **graph/lock/receipt/generation** | The graph is linked records, not a file the user draws. It points an application to source, exact dependencies, program, and permissions; a run/update points back to that record and its outcome receipt. A lock records exact selected versions/content, a receipt records what an action did/refused/left unresolved, and a generation is a prepared or active app version kept distinct from others. |
| **authority** | Permissions actually granted by the host. Package-requested permissions, signatures, names, and effects are requests/evidence, not grants. |
| **broker** | Jetpack's host-side software, normally a separate service or process, that checks and performs approved outside operations. It is not a person and is not an ambient permission source. |
| **adapter** | An explicit translation/integration path for another execution environment, such as a JVM or WebAssembly component. It is not the default Jetpack engine hiding another runtime. |
| **owned source** | Editable dependency code belonging to this project. A failed update must not silently overwrite it; `.jet/deps/` is a proposed ownership convention, not a command. |

Requirement IDs (`M-`, `H-`, `P-`, `C-`), hex bytes, opcode tables, sigils, IDL, and JSON blocks are specification notation. They become useful after this map; they are not names of additional products or current registered commands.

<a id="summary"></a>
## Summary

**PROPOSED:** Jetpack should replace the *jobs* of a portable runtime, a package/deployment graph, and a sandbox boundary with one Jet-owned execution graph. It should not promise to run every existing JAR, CLR assembly, JavaScript package, BEAM release, Wasm component, or OCI image without a source or process adapter. The replacement is semantic and operational: new Jet programs acquire one content identity, one authority request, one verified portable representation, one host-service contract, and one lifecycle/receipt history from source through execution. Literal binary compatibility remains a separate adapter concern.

The selected direction is the architecture in the [architecture contract](#architecture): stable, owned Jet bytecode under the **JPM/1** portable-machine contract, carried by the existing Jetpack package/lock/Hangar/receipt graph. JPM/1 is typed block-local SSA/register code with explicit block parameters and control-flow edges. Its scalar core is `i32`, `i64`, `f32`, `f64`, and `v128`; function references and resource references are typed; guest memory is bounded, byte-addressed, and never a host pointer. The verifier, reference interpreter, native JIT/AOT engines, browser lowering, and debugger all consume the same verified meaning. A separate host broker owns authority and effects through the proposed [host ABI](#host-abi). A package cannot manufacture a host handle, silently acquire ambient authority, or fall back from an untrusted portable program to trusted native code.
`Int` remains Jet's arbitrary-precision source type. JPM `i64` is a low-level primitive, not permission to narrow `Int`; exact `Int`, `Fraction`, checked/floor/mod/remainder/shift operations, and their CoreLib lowering remain shared semantic operations. A portable exact-`Int` implementation may use one guest big-number representation/library in portable memory without imposing a tracing collector on all programs, and it MUST NOT expose host pointer identities as exact-number semantics.

This direction is not a claim that JPM/1 is universally faster, smaller, safer, or more compatible. Standard WebAssembly already provides a mature core validation boundary, Component Model/WIT interfaces, capability-oriented WASI, source-level DWARF debugging paths, and practical interpreters and AOT/JIT runtimes. JVM and CLR provide standardized managed execution, mature collectors, debuggers, profilers, reflection, and AOT variants. OCI and container systems provide mature image distribution, variant selection, volumes, secrets, readiness, and rollout operations; rootless and VM-backed configurations are real alternatives. eBPF, NaCl/SFI, and portable RISC-V demonstrate other useful verifier or code-generation boundaries. JPM/1 is justified only if it closes the particular gap between Jet semantic authority, host authority, package identity, and deployment lifecycle without losing required jobs. The [performance kill gate](#performance) remains open until paired evidence exists.

The proposal preserves incumbent strengths instead of pretending that a new bytecode makes them disappear:

* Existing Jet source semantics, Prelude/CoreLib meaning, package/lock/Hangar/receipt ownership, source preservation, and current native/plugin contracts remain current law until explicitly amended.
* A Wasm or Component Model projection remains available for browsers and existing Wasm ecosystems; OCI remains a distribution projection; JVM/.NET/JS/BEAM and native process boundaries remain explicit adapters.
* Managed-language frontends retain their language GC, reflection, exception, scheduler, and library costs. JPM's bounded memory is not a claim that a Java, CLR, JS, or BEAM runtime no longer needs its own managed heap.
* Native and trusted FFI remain expert trust transitions. They are not the same security mode as hostile portable execution.
* Existing debugger, profiler, and service strengths are retained where an adapter can make their authority, source maps, and lifecycle semantics explicit.

Jonathan Blow's captured criticism is handled narrowly. In a 2026 Standup #39 passage he argues for a cross-platform, well-defined machine-like substrate with real debuggers and says WebAssembly does not fully reach that goal or become the default. In a 2024 LambdaConf passage he asks why generalized libraries are not both compiled and run from a VM-like bytecode representation. The recordings do **not** establish that “the JVM fails,” and this draft does not attribute that claim to him. The useful design response is to make analyzability, source/debug identity, host boundaries, and one reference meaning explicit, while acknowledging that current Wasm and JVM tooling already addresses parts of the request.

No empirical comparison was run for this draft. The conformance vectors, workload matrix, receipt format, and gates below define what evidence would be needed. Until each required gate is closed, the only defensible status is **proposed, unratified, and empirically unproved**.

<a id="goals"></a>
## Goals, replacement boundary, and comparison law

### G-1. Replace jobs, not every incumbent binary

The target jobs are source compilation/package, artifact identity/distribution, pre-execution validation, multi-host execution, mediated files/network/time/randomness/process/GUI/audio/GPU/ML/embedded access, debugging, updates, readiness, rollout, failure, migration, and rollback. A JVM, container, Wasm runtime, or microVM performs only some. A comparison MUST name its layer; format, runtime, isolation, image store, and orchestrator are not interchangeable.

`C-001` **Layered comparison.** Every comparison claim MUST identify whether it concerns representation, language runtime, host ABI, isolation, package/distribution, or lifecycle/orchestration. A claim about one layer MUST NOT be used as proof about another layer.

`C-002` **Replacement meaning.** A claimed replacement MUST state whether it replaces a user workflow, a portable artifact, a host API, an isolation boundary, or an unchanged binary. JPM/1 replaces the first four for new Jet artifacts; it does not promise unchanged execution of arbitrary foreign binaries.

### G-2. Preserve one Jet meaning

A source program has one checked semantic meaning. The current boundary is checked Source/AST -> typed TIR -> canonical MIR (schema 3) -> adapters, with JPM/1 downstream as stable distribution machine. Every tier MUST preserve it; profiles may change capabilities/limits, not arithmetic/type/trap/CoreLib meaning. TIR/MIR remain private representations, not public wire law or a second authority; see [existing contracts](#existing-contracts).

`C-003` **Tier identity.** Every required fixture MUST produce the same defined result or the same specified failure category across the reference interpreter, JIT, AOT/cache, browser path, and each claimed host profile. A missing tier is an uncovered comparison, not a pass.

### G-3. Cover the finite comparator closure

The finite closure is: JVM/HotSpot/Graal Native Image; .NET CLR/NativeAOT; core Wasm, Components/WIT, WASI, Wasmtime, Wasmer, WAMR, Spin, and wasmCloud; Docker/OCI/containerd including rootless, gVisor, Kata, and Firecracker; V8 isolates; BEAM/OTP; Linux eBPF and Solana SBPF; NaCl/PNaCl and SFI; raw/profiled RISC-V; LLVM bitcode and MLIR bytecode; and SPIR-V/PTX/WebGPU as device-only representations. This is a representative family closure, not a claim to cover every future product.

`C-004` **Finite coverage.** A final claim of broad replacement MUST include a row for every family and every required workload cell in [coverage](#coverage), with either evidence, a named unknown, or a failing/unavailable gate. Omitting a family because its strengths are inconvenient is non-conforming.

### G-4. State the universal-optimum impossibility

The requirements conflict. One artifact cannot be unconstrained native code, a small independent verifier target, a managed object machine, a browser component, a complete OS image, and a zero-copy device program without trade-offs. Native breadth conflicts with hostile containment; closed-world closure with reflection; isolation with unrestricted devices; replay with external I/O; tiny bytes with rich debug/runtime metadata; global rollback with committed effects.

`C-005` **No universal optimum.** The final specification MUST list these conflicts and MUST NOT use a weighted average to declare a universal winner. A recommendation may choose a coherent point in the trade space, but each non-winning requirement and residual cost remains visible.

### G-5. Use evidence labels and conservative claims

`EVIDENCE` means a cited specification, current source, or documented implementation fact. `ANALYSIS` means a comparison derived from those facts. `PROPOSED` means a design rule, not current behavior. `UNKNOWN` means no captured source or execution establishes the claim. No prose, signature, cache, or card status can be labeled a passing result.

`C-006` **Evidence discipline.** A final comparison MUST cite exact source version/date or repository revision, distinguish source inspection from execution, and use `UNKNOWN` when the required observation was not made. Historical measurements from NaCl, for example, MUST NOT become JPM measurements.

### G-6. Define the ratification gate before measuring

`C-007` **Correctness and security precede speed.** A candidate comparison result is admissible only after artifact parsing, verifier soundness, authority enforcement, lifecycle behavior, and all claimed execution tiers pass the relevant conformance vectors. A faster configuration that does less work, grants more authority, loses cancellation, or weakens the security policy is not a win.

`C-008` **Required metric gate.** For each named peer/profile/workload cell whose comparison layer is `comparable`, and for each `adapter-required` cell after its adapter proof closes, the prespecified uncertainty bound MUST support the orientation-correct `normalized_ratio < 1.00` for non-Rust peers. `representation-only` and `device-only` cells are not valid complete-stack denominators; they remain visible with their layer and reason. `unknown`, `unavailable`, and `failing` cells are not passes and cannot be removed by naming a new profile. For Rust, `normalized_ratio <= 1.05` is parity only and MUST name the fixed control configuration.

`C-009` **Format kill gate.** If JPM/1 cannot meet the correctness/security/tier gates, or cannot beat the strongest standard substrate on the required performance cells under equal work, the format recommendation MUST be rejected or narrowed to the cells it can prove. “JIT later,” a permanent compatibility shim, a warm-JPM/cold-peer comparison, and a profile name that excludes a losing workload are not acceptable substitutes.

<a id="terminology"></a>
## Terminology

Each term owns one fact. Terms from another chapter MUST NOT be reused with a second meaning.

| Term | Exact meaning in this comparison |
|---|---|
| **Artifact** | A produced payload/file plus the metadata needed to identify, check, and connect it to source and dependencies. For the portable path this is a Jet bytecode payload with JPM/1 metadata; a foreign JAR, Wasm component, OCI image, or native binary remains its own representation until adapted. |
| **Package** | A node in the existing Jetpack package/lock/Hangar graph. It owns source, dependencies, trust metadata, projections, and lifecycle references; it is not just a byte array. |
| **Program** | The instructions that perform the user's task, compiled from checked source or an explicitly adapted foreign input. A program has imports, exports, resource requests, and a declared entry/lifecycle contract. |
| **Jet bytecode** | Portable instructions for Jetpack's engine. `JPM/1` means Jet Portable Machine, version 1; `/1` is the contract version, not another product or source-language version. |
| **Host interface / ABI** | The exact request/result agreement between an application and host services. `JPH/1` means Jet Portable Host, version 1; ABI means application binary interface, not a separate runtime. |
| **Runtime / engine** | Software that executes instructions. Jetpack's engine is distinct from an optional language-specific runtime carried by an adapted JVM/CLR/JS/BEAM program. |
| **Sandbox** | Combined worker checks, broker checks, and selected OS/process/VM containment restricting memory, outside access, and resource use. Kernel, administrator, driver, and hardware compromise remain outside the guarantee. |
| **Native** | Machine instructions for one CPU/OS. Verified Jet bytecode may be JIT/AOT-compiled into sandboxed native instructions; a foreign native plugin/helper is a separate trust transition. |
| **Tier** | One way to execute the same program meaning: interpreter, JIT, or AOT. Browser and embedded are host profiles as well as possible paths, not permission to change meaning or security. |
| **Instance** | One activated execution of a program generation with its own memory, tasks, handles, quotas, channels, and receipt lineage. |
| **Authority domain** | The broker-enforced set of principals, handles, rights, and revocation state for an instance. A signature, package name, type effect, or compile-time annotation is not itself a grant. |
| **Host** | The OS, browser, embedded kernel, device runtime, or service process that supplies a profile and enforcement boundary. |
| **Profile** | A declared capability, limit, device, tier, and enforcement envelope. It MAY reject a program before activation; it MUST NOT alter the program's scalar/type/trap meaning. |
| **Resource** | A host or guest object subject to ownership, generation, rights, limits, and lifecycle: a file handle, socket, stream, task, volume, GPU object, secret, or memory reservation. |
| **Receipt** | An authenticated record binding source/package/artifact digests, authority decisions, environment, engine tier, resource events, outputs, errors, and lifecycle transitions. A receipt is evidence of an observed run, not a proof merely because it is signed. |
| **Generation** | An immutable content/configuration revision selected for activation. A rollout may expose more than one generation while old instances drain. |
| **Compiler IR** | A private compiler representation in the checked Source/AST -> TIR -> canonical MIR path (current MIR schema 3). It can change without changing JPM/1; it is not public wire law or an authority policy. |
| **Comparator** | A named family/configuration compared on a specific overlapping job. A family representative does not prove every product in the family. |
| **Paired workload** | One fixed source, dataset, correctness oracle, failure schedule, policy, limits, and environment run through two or more comparable stacks. |
| **Unknown** | A claim for which the required source or observation is absent. Unknown is not solved, mitigated, or a pass. |

<a id="existing-contracts"></a>
## Existing contracts

### Boundary and status

Jetpack already has a package and environment model; it is not an empty package-manager slot. The package-owned proposal therefore extends one model instead of introducing a second manifest, resolver, trust database, deployment manifest, or sandbox configuration. The sources inspected for this chapter are the current repository files `crates/jet-pkg-model/src/Manifest.rs`, `crates/jet-pkg-model/src/Lock.rs`, `crates/jet-foundation/src/Authority.rs`, `crates/jet-pkg-model/src/Authority.rs`, `crates/jet-env-model`, `crates/jetpack`, and `Docs/spec/architecture.md`, all inspected on 2026-09-19. They establish source contracts and code shape only. No compiler, runtime, build, test, or deployment was exercised for this draft.

The following table is the current-law ledger that this proposal must preserve unless an explicit amendment is accepted later.

| Surface | Current contract | Ownership and evidence status |
|---|---|---|
| Package authoring | `package.jet` is the canonical package file. A parser does not accept a legacy package alias. `env.jet` is optional and names source aliases/environment facts; other `.jet` files are configuration. | Current syntax/model law; `crates/jet-pkg-model/src/Manifest.rs` and syntax decision S52. |
| Project lock | One `.jet/lock` is the sole project lock. Locked records carry source kind/version, source and tree/content hashes, dependencies, effects/grants/authority, platform and variant data, output identity, receipt digest, and provenance. Canonical writes are atomic; stale or malformed lock data is a trust failure, not an ordinary cache miss. | Current model and syntax law; `crates/jet-pkg-model/src/Lock.rs`, syntax decisions D-SHAPE-MERGEPROVENANCE1 and D-ECP-RECEIPT2. |
| Graph and store | The typed package/action/environment graph feeds package outputs, environment plans, images, fleets, and JetOS generations. Hangar is the shared content-addressed store. Realization, cache admission, leases, quarantine, repair, and GC are engine concerns under the read-only model boundary. | Current ownership boundary; `jet-pkg-model`, `jet-env-model`, `jetpack`, and Hangar code. |
| Receipts | A successful action receipt joins exact inputs, planned actions, output digests, activation proof, parent generation, and rollback facts. Producer/WAL closure, reachability, explain, repair, and GC use this lineage. | Current accepted direction (#655, #420, #1019/#1020, #517); observed code and Tower evidence, not a claim that every path has runtime proof here. |
| Authority | There is one authority carrier and one rights tree/holds relation. `ApplicationAuthority` combines semantic effects, package loader policy, and interactive replacement. `SandboxAuthority` lends a narrower scope, resolves no-follow roots, and checks imports against declared needs. | Current law; `crates/jet-foundation/src/Authority.rs` and `crates/jet-pkg-model/src/Authority.rs`. |
| Compiler seam | `jet-pkg-model` is read-only and has no provider/network/shell dependency; `jet-env-model` is a pure plan model; `jetpack` owns provider, network, shell, realization, and JetOS. Ordinary `jet` does not link or initialize Wasmtime. | Current crate boundary; `Docs/spec/architecture.md` and `AGENTS.md`. |
| Execution meaning | The current source boundary is checked Source/AST -> typed TIR -> canonical MIR (schema 3) -> backend/adapters. The portable machine is downstream of semantic MIR, not another source-language authority. Current adapters and Prelude/CoreLib preserve one meaning across applicable tiers. | Current law in code; some architecture prose still says TIR is the sole seam, which is stale relative to the current MIR boundary. |
| Numeric and concurrency law | `Int` is exact arbitrary precision; fixed-width arithmetic and `Int`/`Fraction`/floor/mod/remainder/shift operations belong to shared Prelude/CoreLib semantics. Source inspection found a TIR/MIREval `Int / Int` Fraction path and direct truncating helper branches, but did not establish reachability or an observed user-facing failure. `Atomic<T>` has a closed carrier set, no public order argument, and current operations use SeqCst; lock-free Atomic64 is an explicit target requirement and there is no silent lock fallback. | Current law plus explicit integration gates; [Sources](#sources) and [Sources](#sources). Exact scalar FP details remain incomplete and are not silently normalized by this proposal. |
| Native code | A native `Library` is trusted in-process code with an exact C-safe scalar/text surface. It is not a second sandbox. Host owns signals, thread creation, waiting, free, unmapping, and failure isolation; a panic cannot unwind through C. | Current law; `Docs/spec/architecture.md`, `Docs/spec/spec.md`, and native-library sources. |
| Plugin code | `core.plugin` is an untrusted Wasm Component path with deny-by-default imports, typed host facts, explicit authority, no-follow roots, fuel 10,000,000, memory 16 MiB, table 10,000, wire 16 MiB, and a two-second timeout in the inspected implementation. Compiler extensions use a separate world and remain in `jetpack`. | Current law and implementation constants; `Prelude/Plugin.rs`, compiler-extension sources, and syntax decisions. The constants are source-inspected, not an exercised guarantee. |
| CLI | `jet run`, `jet build`, and `jet package` are registered under `jet`. `jet inspect` has registered planes including `types`, `rights`, `claims`, `shapes`, `accel`, `decisions`, `structure`, `build`, and `gates`. `jetpack` dispatches package operations including `doctor`, `env`, `use`, `config`, `trust`, `hangar`, `audit`, `import`, `add`, `remove`, `update`, `lock`, `outdated`, `search`, `info`, `explain`, `why`, `logs`, service probes, overrides, push/image/bridge/OS/profile/browser operations. `jetpack run`, `build`, `test`, and `fmt` are retired with redirects to `jet`. | Current command registry; `crates/jet-cli/src/CLI.rs` and `crates/jetpack/src/CLI/parse.rs`. A registry entry is not proof that its path was exercised. |
| Profile and generation | Profiles are immutable generation directories and pointers. Realization, digest verification, complete markers, activation proof, and atomic publication are shared with JetOS generation work. `jet push` currently returns an unavailable/gated path rather than pretending remote rollout exists. | Current bounded implementation/status evidence; #425, #655, `crates/jetpack`, and the JetOS generation sources named in `Docs/spec/architecture.md`; no rollout execution is claimed. |
| Existing source forms | Core provider accepts local/Git source and records source content identity. `components/` is a copy-in-and-own path that refuses overwrite. `vendor/` is a Hangar export/copy path with hash sidecars. No inspected source establishes `.jet/deps`, an upstream baseline, or a three-way update protocol. | Current facts and explicit gap. `.jet/deps` below is a proposed extension, not retroactive current law. |

The observed Hangar path has a documented divergence: older architecture text names `/etc/jet/hangar/`, while executable code and the per-user layout decision use a user-owned data root with environment overrides. This draft does not choose a path. A host installation resolves the configured Hangar root and records its identity in the receipt. A package record MUST NOT infer authority from a pathname.

### Normative preservation rules

**P-EXIST-001 (one graph).** The package manifest, `.jet/lock`, Hangar objects, trust records, receipts, profiles, environment plans, service records, images, and fleet assignments MUST be projections or records of one content-linked graph. An implementation MUST NOT create a parallel deploy lock, plugin lock, sandbox policy file, or hidden install database that can disagree with `.jet/lock`.

**P-EXIST-002 (one authority carrier).** Package resolution MAY calculate requested rights, but it MUST hand the canonical authority facts to the JPH/1 host boundary and the execution tier. A package signature, source path, profile name, cache location, or compiler effect MUST NOT itself grant a host right.

**P-EXIST-003 (seam direction).** `jet-pkg-model`, `jet-env-model`, compiler front ends, codegen, and seam crates MUST NOT link provider, network, shell, deployment-engine, or Wasmtime implementation dependencies merely to parse or check a package. Runtime-only components may own an engine as already allowed by the plugin contract. The current semantic path is checked Source/AST -> TIR -> canonical MIR -> adapters; the compiler can lower checked semantic facts to JPM/1 and graph facts without initializing an engine or contacting a provider.

**P-EXIST-004 (semantic identity).** Package selection, artifact verification, generation activation, and service execution MUST preserve the one Prelude/CoreLib meaning required by the current execution law. `Int` remains arbitrary precision even though JPM has an i64 primitive; exact `Int`/`Fraction` behavior, fixed-width overflow policy, division/remainder distinctions, shifts, and the closed SeqCst atomic carrier law cannot be narrowed by a target profile. The lock-free Atomic64 target requirement cannot be replaced by a hidden mutex fallback. A native cache, foreign runtime, browser adapter, or host profile cannot silently define a second arithmetic, error, authority, or cancellation meaning. JPM/1 and JPH/1 are the portable contracts; they do not authorize a semantic fork.

**P-EXIST-005 (clean amendment).** Replacing current Wasm plugin defaults, changing native-library trust, changing package reference syntax, or changing the current `.jet/lock` authority requires an explicit future amendment. This proposal names the extension and its migration boundary but does not claim that amendment has happened.

**P-EXIST-006 (source evidence honesty).** A source-inspected schema, registered command, or accepted Tower card MUST be described as such. No example in this chapter is an observed command output, benchmark, successful install, or security result. Proposed protocol traces are explicitly labelled `PROPOSED TRACE`.

**P-EXIST-007 (semantic boundary).** A package record MUST identify source/AST provenance, TIR facts, canonical MIR schema and digest, and downstream JPM/1 output as distinct identities. Compiler-private MIR is not a public ABI merely because a package stores its digest. A graph action that cannot bind these identities MUST refuse portable admission.

**P-EXIST-008 (numeric adoption gate).** A future exact JPM floating-point baseline MAY recommend IEEE widths, nearest-even elementary arithmetic, gradual underflow, explicit FMA, no implicit reassociation/contraction, and specified NaN behavior, but that is a proposed all-tier amendment. The package lock MUST carry a numeric-semantics digest and MUST NOT claim tier parity from portable-only FP normalization. The unresolved direct-helper reachability question is an integration gate, not an observed failure and not permission to choose a new source law.

**P-EXIST-009 (failure ownership).** A hard containment trap terminates the affected authority-isolated JPM instance and guest tasks; it MUST NOT promise that an interrupted shared heap can be resumed. Ordinary host operation failures such as `Denied`, `NotFound`, `QuotaExceeded-before-side-effect`, `WouldBlock`, and operation-level `Cancelled` remain typed operation errors and need not kill a sound instance. Reusing the current plugin path after a containment trap requires an explicit lifecycle amendment and caller cutover.

**P-EXIST-010 (host input and transfer).** A JPH/1 host call MUST own an immutable snapshot before full semantic validation unless an immutable buffer or exclusive lease is held for the whole use. It MUST NOT validate guest bytes and then re-read mutable bytes. Mutable zero-copy transfer is ownership, not transaction rollback: cancellation or trap may leave partial mutation unless copy-on-write/commit was explicitly selected.
**P-EXIST-011 (semantic error ownership).** Semantic import and source checks remain sema-owned. Invalid portable bytes supplied as an external artifact are a user-facing artifact/runtime failure after implementation; malformed JPM generated by the same checked Jet compilation is an internal compiler failure (the current internal-failure boundary is exit 101), not a source type error. The symbolic package error names in this proposal are not claims about registered diagnostic rows. The JPM hard-trap lifetime rule is a proposed amendment for the new portable machine; current Wasm plugin caller behavior cannot be assumed to satisfy it until an explicit plugin-lifetime amendment and complete caller cutover.

### What this package contract does not own

JPM/1 defines the verified portable code bytes, instructions, traps, and portable execution identity. JPH/1 defines host ABI layouts, handle lifecycle, asynchronous completion, and host service interfaces. This package chapter owns graph identity, source ownership, trust, install/update/deployment state, and receipts that bind those contracts. It does not redefine a JPM opcode, assign a host pointer layout, or claim that an OCI image is executable portable code. OCI, Kubernetes, Nix, JVM artifacts, and foreign package ecosystems can be imported or projected through providers; their identities remain subordinate to the Jet graph when represented as Jet dependencies.

<a id="architecture"></a>
## Architecture
### A.1 Boundary and execution flow

**M-ARCH-001.** JPM/1 is one stable, typed, block-local SSA machine. It is a distribution and execution contract, not compiler-private TIR/MIR, LLVM bitcode, a source-language type checker, or a host pointer ABI. A producer may lower canonical MIR schema 3 to JPM, but an engine MUST NOT infer source semantics from a JPM byte sequence.

A portable run has this order:

1. The Jet compiler parses and checks source, selects Prelude/CoreLib implementations, and records source semantic identity.
2. The compiler produces canonical MIR schema 3 and a complete source semantic bundle. The semantic bundle is required for a source-bound claim; a bare JPM file is machine-only.
3. A writer emits canonical JPM bytes, source/debug identity in the package graph, and a verification input record. JPM has no custom executable extension section.
4. The existing package/lock/Hangar/receipt graph binds JPM content, dependencies, JPH interface identities, requested authorities, profile, limits, semantic bundle, and generation.
5. A bounded loader snapshots the immutable bytes, parses canonical sections, verifies types/control flow/references/segments/imports, and emits a verification receipt. No instruction or initializer runs before verification finishes.
6. A linker resolves machine imports and JPH/1 descriptors, checks same-authority limits and adapter contracts, reserves the declared instance envelope, applies local active segments, resolves start, and only then publishes the generation.
7. A reference interpreter, baseline JIT, optimizing JIT, AOT image, browser lowering, or embedded engine executes the same verified JPM meaning. A worker executes guest state; a broker performs authority-bearing host effects.
8. Package lifecycle controls generation, update, drain, rollback, revocation, and cache invalidation. Debugging uses optional JPM debug maps plus package source identity, never optimizer IR as a portable ABI.

**M-ARCH-002.** Representation, execution tier, authority grant, host profile, and resource envelope are orthogonal. A profile MAY reject an operation, lower a quota, or select a different tier, but MUST NOT change the meaning of an accepted scalar, reference, trap, atomic order, memory access, or FP bit pattern. Unsupported host capability is a pre-activation refusal, not a semantic fallback.

**M-ARCH-003.** The portable TCB is the bounded parser, verifier, linker, reference semantics, selected engine/tier, cache-admission checker, broker authorization path, and process/VM boundary selected by the profile. A guest runtime owns its object layout, collector or allocator, exceptions, scheduler, and source-level temporal rules. Kernel, hypervisor, device, and hostile-root compromise remain outside this proposal.

**M-ARCH-004.** JPM has no computed native address, guest-to-native pointer conversion, executable-memory construction, or bytecode `eval`. JIT/AOT code generated from verified JPM remains inside the selected sandbox and preserves the same JPH/1 authority and instance-generation boundary. Loading a foreign native library/plugin/helper is a separate explicit trust and containment transition with its own profile admission. An untrusted artifact MUST NOT silently fall back to that foreign native path when its portable tier is unavailable.

### A.2 Worker, broker, and reference authority

The worker owns instruction cursors, guest stacks, linear memories, tables, globals, and private typed reference tables. The broker owns host objects, OS handles, authority grants, quotas for broker-side allocations, JPH/1 descriptors and wire conversion. A guest resource value is an opaque typed alias to a private binding; it is not a token, integer, pointer, serialized attachment, or authority grant.

**M-ARCH-005.** A guest MUST NOT manufacture `resref<T>` by integer conversion, memory load, bitcast, NaN payload, table index, or function reference. Only a typed import result, typed function result, a table value already present in the instance, or `ref.null` produces one. JPH/1 owns attachment encoding, ownership transfer, cancellation, revocation, and resource wire layout. JPM refers to those contracts by exact digest.

A host call snapshots every guest range that it inspects or retains before semantic validation unless JPH/1 has established an immutable sealed buffer or an enforced exclusive lease for the complete use. The host MUST NOT validate, suspend or re-enter, and later re-read a concurrently writable range. A mutable mode-1 output is valid only while an exclusive lease is held through validation and commit, or when the broker writes an owned staging result and performs an atomic exclusive commit. Revalidation without exclusion is not a safety rule.

**M-ARCH-006.** `MemorySlice`, attachment, aggregate, error, callback, and asynchronous call wire formats are JPH/1 facts. JPM passes typed machine values to an adapter; it does not copy a second host serializer. A `BrokerToken` never appears in JPM, guest memory, a guest result, or a log.

### A.3 Exact JPM-to-JPH projection

**M-ARCH-017.** Every `provider=JPH` import binds this exact descriptor identity:

```
(interface_digest:Digest256, function_id:u32-le, signature_digest:Digest256,
adapter_version:u8, adapter_mode:u8, adapter_contract_digest:Digest256)
```

`interface_digest` selects the JPH interface namespace; `function_id` selects one descriptor within it; `signature_digest` authenticates the canonical descriptor signature; `adapter_version` is `01` for the first projection; `adapter_mode` is one of `00=sync`, `01=async`, `02=callback_register`, `03=callback_delivery`; and `adapter_contract_digest` identifies the versioned projection record. All other adapter-mode values reject at link. The JPM import's display `module` and `name` are diagnostics and graph labels only; they MUST NOT select a function.

The JPH-owned adapter record for version 1 has these canonical obligations, in this order, even though its bytes are not duplicated in JPM:

| record | required mapping |
|---|---|
| parameter map | each machine parameter slot, exact machine type, JPH `TypeId`, conversion kind, and direction; slots are ordered by machine signature |
| scalar | bit width and signed/unsigned interpretation are explicit; no host ABI inference |
| `resref<T>` | a JPH attachment entry with the exact resource type, generation, transfer mode, and authority check; no raw token |
| `funcref<S>` | a callback registration/delivery value only when the descriptor permits that callback mode; otherwise link rejection |
| `MemorySlice` | memory id/generation, checked offset/length, and mode are made by the adapter from a machine argument mapping; JPH H-ABI owns its 32-byte record and lease/snapshot rules |
| aggregate | a JPH `TypeId` and canonical JPH aggregate codec; guest linear bytes are copied by the adapter under snapshot/lease rules, never decoded by an ad-hoc JPM codec |
| result | direct JPH success values map to the descriptor's ordered machine result slots; an aggregate uses its JPH `TypeId` mapping. Every returned `resref<T>`/resource attachment is validated against the authenticated session, principal, channel, object generation, interface/type, rights, transfer mode, and ownership policy before a guest alias is installed |
| error | a JPH `Result<T,Error>` maps to the descriptor's declared machine result/error representation. It is an ordinary typed result; a machine signature that has no declared error representation cannot link |
| async | the adapter returns the exact accepted control result and call id required by JPH, then delivers one terminal completion with the descriptor's result mapping; every returned resource attachment is validated/bound/owned before publication; acceptance is not a guessed success value |
| callbacks | registration and delivery use the descriptor's callback prefix, call id, callback generation, and reentrancy/affinity rules; callback return values use the exact declared callback `Result<T,Error>` mapping and apply the same returned-resource attachment validation, binding, transfer, and ownership checks; an unavailable or forbidden callback mode rejects |
| suspension | borrowed ranges do not survive suspension/re-entry; sealed buffers or exclusive leases are required, and resumption returns through the same verified call identity |
Every synchronous terminal reply and every asynchronous/callback terminal completion is exactly the descriptor-declared JPH `Result<T,Error>` variant. The call-frame outcome discriminant and the numeric error domain/code MUST agree with that variant; a bare `Error` payload is not a terminal reply. `AcceptedReply` is control-only and is never substituted for the terminal result. A callback delivery uses the exact declared callback result variant.

For every success result, terminal completion, and callback return that contains a resource or attachment, the adapter first validates the returned attachment against the authenticated session tuple, principal, channel, instance generation, isolation domain, authority/revocation epoch, object generation, interface/resource `TypeId`, rights, transfer mode, and ownership/retain rule. It creates the guest's private typed alias only after that validation and records the binding/ownership transition; a failed check returns the descriptor's typed `stale`/`closed`/`denied`/`protocol_error` result without exposing a partially bound value. Callback returns use this same path, not a weaker callback-only path.

At link, the verifier recomputes the machine signature digest, checks every parameter/result `TypeId`, adapter mode, callback/async state, MemorySlice mode, input and returned attachment transfer, aggregate/error map, and descriptor version. Any missing or mismatched map rejects activation as `bad_host_projection`. The adapter delegates value encoding to JPH/1; it never selects a descriptor by text, and no second JPM serializer is permitted.

A JPH ordinary failure (denied, stale, closed, quota, unsupported, timeout, or provider-defined error) is returned through the linked typed error representation. A JPM trap is only used for a machine-defined safety/containment failure or a machine-only resource operation explicitly listed in the trap table.

### A.4 Source semantic bundle and machine-only artifacts

**M-ARCH-018.** A source-bound activation MUST carry the complete content-identified `SemanticBundle` object defined once by `P-GRAPH-001`. Its exact fields and preimage are the package graph's `SemanticBundleBody/1`: format `01`, Prelude/CoreLib implementation/content digest, numeric-semantics digest, MIR schema `3`, and sorted complete required-module and required-interface digest arrays. `SemanticBundleDigest` is the graph object identity computed from that fixed preimage. The source-bound node, lock, artifact, verification receipt, and admission receipt reference that digest; each verifier resolves and recomputes the complete object before link, cache, or activation. They MUST NOT repeat a partial semantic record. The bundle does not choose a new Jet `Int /` law.

**M-ARCH-019.** A raw standalone JPM file without a source semantic bundle is explicitly **machine-only**. It may execute its own JPM scalar/control/memory contract when its imports and profile are satisfied, but it MUST NOT claim Jet source-language meaning, exact `Int`, Fraction, exception, or Prelude behavior. Source-bound activation with a missing or mismatched bundle fails at the activation phase as `semantic_bundle_missing` or `semantic_bundle_mismatch`, not as a malformed byte result.

A Prelude/CoreLib or JPH helper MAY be inlined only when its exact implementation/content digest and provider identity are bound in the NativeCacheKey below. Otherwise the tier MUST preserve the call boundary.

### A.5 Lifecycle, dynamic code, and traps

A verified module is immutable. A new module or function is new package content and generation. A JPH/package load request is re-admitted through trust, dependency, size, verifier, authority, profile, and generation checks; mobile, browser, and JIT-forbidden profiles may reject it.

**M-ARCH-020.** Every JPM trap terminates the authority-isolated instance and all guest tasks in that instance. There is no recover-and-continue shared-heap guarantee and no rollback promise for a mutation or external effect. A child trap cannot become a status observed by a still-running same-instance parent `thread.join`, because the parent is terminated with the child. `thread.join` has no `trapped` result. Ordinary guest exceptions and JPH errors are explicit typed values and do not become traps. `thread.cancel` is an advisory request, observed only by `thread.check_cancel`, `atomic.wait`, `thread.join`, an imported cancellation boundary, or instance termination; it does not asynchronously unwind a shared heap.

Fuel exhaustion, deadline, failed safety invariant, engine fault, hard host containment failure, and the explicit machine traps in C.12 all follow the same instance scope. A helper that continues in a separately killable broker worker remains charged and retains storage until terminal completion or an enforced kill/device-loss fence proves quiescence; a device-loss notification alone never releases charges or storage.

### A.6 Adoption boundary

JPM's own-format costs include a new parser/verifier, every tier, JPH adapters, debug tooling, package integration, and a second conformance corpus beside standard Wasm. The recommendation is falsifiable: paired comparisons against the strongest standard substrate must cover all required correctness, security, tier, and workload cells. An uncovered or losing required cell blocks a performance claim; no arithmetic, host, or source law is selected by a benchmark sentence.


<a id="artifact-format"></a>
## Artifact format
### B.1 Envelope, sections, and encodings

**M-FMT-001.** A JPM/1 file begins with exactly eight bytes:

```
4A 50 4D 00 01 00 00 00
```

The first four bytes are `JPM\0`; byte 4 is major `01`; byte 5 is minor `00`; bytes 6–7 are reserved flags and MUST be zero. A different major, unsupported minor, nonzero reserved flag, or truncated header is `bad_header`.

All integers are little-endian. `uLEB32`, `sLEB32`, and `sLEB64` are minimal-width little-endian base-128 encodings; overlong, unterminated, or out-of-range encodings are `bad_varint`. Fixed-width `u16/u32/u64` immediates are little-endian. A byte offset/address/length is `u64`; a wire count/index/section length is a checked canonical `uLEB32` unless a row says otherwise. No guest instruction accepts a host pointer.

The section stream is ordered and has no duplicates:

| tag | section | requirement |
|---:|---|---|
| `01` | types | required |
| `02` | imports | optional |
| `03` | functions | required |
| `04` | tables | optional |
| `05` | memories | optional |
| `06` | globals | optional |
| `07` | elements | optional |
| `08` | data | optional |
| `09` | code | required |
| `0A` | exports | required |
| `0B` | start | optional |
| `0C` | debug | optional |

Each section is `tag:u8, payload_length:uLEB32, payload[length]`. Unknown tags, missing required sections, duplicate tags, wrong order, trailing bytes, or payload overrun are `bad_section`. JPM has no custom executable section; package metadata is outside the artifact.

Canonicalization rules are part of the digest. Signatures are interned by first occurrence; resource rows are sorted by canonical resource identity with the built-in thread row first; imports are sorted by `(module,name,kind)`; exports by UTF-8 name. Names are unique and use the existing ASCII identifier grammar for JPM labels. JPH descriptor names are UTF-8 `Text` owned by JPH and do not select imports.

### B.2 Types, index spaces, and imports

The types payload is:

```
signature_count:uLEB32
signature[signature_count]:
  parameter_count:uLEB32
  parameter[parameter_count]:ValType
  result_count:uLEB32
  result[result_count]:ValType
resource_count:uLEB32
resource[resource_count]:
  kind:u8
  if kind==00: builtin_id:u8       # 00 = jpm.thread only
  if kind==01: jph_resource_type_id:Digest256
```

`ValType` is one byte: `01=i32`, `02=i64`, `03=f32`, `04=f64`, `05=v128`, `06=funcref` followed by `sigidx:uLEB32`, or `07=resref` followed by `resourceidx:uLEB32`. No other type exists. `funcref<S>` and `resref<T>` are nullable, typed aliases. Type references are checked before use.

Function, table, memory, and global index spaces each contain imports followed by definitions. Function index zero is not special. Imports are:

```
module_len:uLEB32, module:utf8[module_len]
name_len:uLEB32, name:ascii[name_len]
kind:u8
if kind==00:
  sigidx:uLEB32
  provider:u8                 # 00=JPM, 01=JPH
  if provider==01:
    interface_digest:32 bytes
    function_id:u32-le
    signature_digest:32 bytes
    adapter_version:u8        # 01 only
    adapter_mode:u8           # 00 sync, 01 async, 02 callback_register, 03 callback_delivery
    adapter_contract_digest:32 bytes
if kind==01: TableType
if kind==02: MemoryType
if kind==03: ValType, mutable:u8
```

Any provider other than `00/01`, an all-zero JPH interface/signature/contract digest, an unsupported adapter version/mode, or a provider-specific field in a JPM import is `bad_host_projection`. For a JPM provider the interface/function/adapter fields are absent and the digest is not inferred. JPH descriptor identity is the complete tuple from M-ARCH-017.

`TableType` is `elem_type:ValType` (only `funcref` or `resref`), `min:uLEB32`, `max:uLEB32`, and `flags:u8`; bit 0 is mutable and all other bits are zero. `max <= 0xFFFF_FFFE`; `0xFFFF_FFFF` is reserved as the table-grow failure bit pattern and MUST NOT be declared. `min <= max`.

`MemoryType` is `min_pages:u64-le`, `max_pages:u64-le`, `flags:u8`; page size is exactly 65,536 bytes, `min <= max`, bit 0 is shared, all other bits are zero, and shared memory has a finite maximum. Host profile limits may be lower but never alter the artifact's meaning.

The functions payload is `defined_count:uLEB32` followed by `sigidx:uLEB32` per defined function. Tables and memories are `defined_count:uLEB32` followed by their type rows. Globals are:

```
global_count:uLEB32
for each: type:ValType, mutable:u8 (00/01), init:ConstExpr
```

A `ConstExpr` is exactly one constant followed by `FF`. Its opcode bytes are `00=i32.const:sLEB32`, `01=i64.const:sLEB64`, `02=f32.const:u32-le`, `03=f64.const:u64-le`, `04=v128.const:16 bytes`, `05=ref.null:RefType`. `global.get` is forbidden in an initializer.

### B.3 Segments, code, exports, and debug

An element row is:

```
mode:u8                  # 00 active, 01 passive
if mode==00: tableidx:uLEB32, offset:ConstExpr(i64)
elem_type:ValType        # funcref or resref
count:uLEB32
repeat count: RefInit
```

`RefInit` is `00=ref.null` or `01=funcidx:uLEB32,sigidx:uLEB32`; static non-null resource entries are forbidden. A data row is:

```
mode:u8                  # 00 active, 01 passive
if mode==00: memidx:uLEB32, offset:ConstExpr(i64)
byte_length:uLEB32
bytes[byte_length]
```

Active and passive indices are checked by kind. Active rows are not in a drop state. Passive data/element state is per instance and cloned for each child; a child drop does not mutate a parent state.

The code payload has one body per defined function in function-section order:

```
body_count:uLEB32
body[body_count]:
  block_count:uLEB32
  block[block_count]:
    block_parameter_count:uLEB32
    block_parameter[...]:ValType
    instruction_count:uLEB32
    instruction[instruction_count]:Instruction
```
**M-FMT-CODECOUNT-003.** The `code` payload is `body_count:uLEB32` followed by bodies in defined-function index order. Verification first checks `body_count == defined_function_count`; either mismatch is `bad_section` before body allocation or instruction validation. Each body corresponds to exactly one defined function; no body is ignored and no function is implicitly empty.

Block zero is the reachable entry. Each block ends with one terminator and has no instruction after it. Branch edges name an explicit target block and its parameter arguments; there is no label-depth encoding.
**M-SSA-ENTRY-002.** A function body has block zero with **zero block parameters**. Function parameters are initialized exactly once in function-parameter slots and are referenced only with kind `00` refs in parameter order. A kind-00 ref index is `< function parameter count`; a block-zero parameter count other than zero is `bad_cfg`. Every non-entry block parameter receives exactly one explicit edge argument from every predecessor with exact type and arity. There is no implicit zero, undefined, or parent-heap value for an entry block parameter.

A wire reference is canonical `packed:uLEB32` with:

```
kind  = packed & 0x3
index = packed >> 2
packed = (index << 2) | kind
kind 00 = function parameter
kind 01 = current block parameter
kind 10 = earlier result slot in this block
kind 11 = reserved and rejected
```

`index <= 0x3FFF_FFFF`; a value whose decoded index exceeds that bound is `bad_ssa`. Result slots use one flat prefix numbering: for instruction ordinal `j`, `result_base[j]` is the sum of result arities of instructions with ordinal `< j`; the `k`th result of instruction `j` is slot `result_base[j]+k`. A result reference is visible only after its producer completes and only at a later instruction ordinal. The same packing and point-in-time bound apply to debug `value_ref` rows.
**M-SSA-BOUND-003.** `packed` is canonical `uLEB32`; decode `index = packed >> 2` and `kind = packed & 3`. The verifier's decoded index bound is `index <= 0x3fffffff`; literal packed `0x40000000` decodes to index `0x10000000` and is valid or `bad_ssa` solely according to the target slot bound. An unrepresentable decoded index requires packed `0x1_0000_0000` and therefore rejects as `bad_varint` before SSA checking. No five-byte uLEB32 value can encode a decoded index above `0x3fffffff`.

Exports are `count:uLEB32` followed by `(name_len:uLEB32,name:utf8,kind:u8,index:uLEB32)`, with kind `00=function`, `01=table`, `02=memory`, `03=global`. The start section is one `funcidx:uLEB32` whose signature is `()->()`.

Debug is optional, non-executable, and uses the enclosing JPM/1 version and primitive encodings. There is no separate debug-version byte.

```
file_count:uLEB32
file[file_count]: path_len:uLEB32, path:utf8[path_len]
name_count:uLEB32
name: entity_kind:u8, funcidx:uLEB32, blockidx:uLEB32,
      value_ref:uLEB32, text_len:uLEB32, text:utf8[text_len]
map_count:uLEB32
map: funcidx:uLEB32, blockidx:uLEB32, instruction_ordinal:uLEB32,
     fileidx:uLEB32, line:uLEB32, column:uLEB32,
     span_start:uLEB32, span_length:uLEB32
```

**M-FMT-DEBUG-002.** `entity_kind` is exactly `00=function`, `01=block`, `02=value`. For `00`, `blockidx` MUST be zero and `value_ref` MUST be `0xFFFF_FFFF`; for `01`, `blockidx` MUST name a block and `value_ref` MUST be `0xFFFF_FFFF`; for `02`, `blockidx` MUST name a block and `value_ref` MUST be a statically valid packed reference to an SSA definition in that complete block, and MUST NOT be the sentinel. Name rows are labels, not point-in-time availability claims: they do not carry an instruction ordinal and the verifier does not reject a valid definition merely because it is produced later in the block. Actual value-location/liveness metadata is checked against the mapped instruction point or live range; before a named definition executes, or outside its live range, the debugger MUST NOT present a value. Unknown kinds, invalid function/block/file, invalid SSA provenance, invalid sentinel/unused fields, invalid UTF-8, or duplicate/unsorted rows are `bad_debug`.

Files sort by path bytes and names/maps sort by their complete canonical encoded rows. Map rows have no duplicate key. `line`/`column` are one-based when nonzero. Every function, block, instruction, file, span, and debug reference is cross-bound by the verifier; absent or optimized-away mappings are legal, while malformed present mappings reject.

### B.4 Content identity and linking

`JPMDigest` is SHA-256 over complete canonical JPM bytes, including the header. It is content identity, not signer identity or authority. A package lock separately binds JPM digest, graph/dependency closure, JPH interface/resource digests, source identity, semantic bundle, profile, and limits.

**M-FMT-002.** Direct same-authority memory/table linking requires exact declared maximum equality, exact element/value type, exact mutability and shared identity, `provider.current >= import.min`, and `provider.current <= import.max`. For a table or memory, `provider.max == import.max`; there is no hidden cap, view, or growth adapter. A current size is a state, not a promise that a future grow succeeds. Reservation failure still yields the operation's specified failure result.

Function links require exact signatures. Global links require exact type and mutability. JPH links additionally require M-ARCH-017's descriptor identity and adapter checks. Cyclic machine imports are linked by checked instance shells, but active segment writes are restricted as stated below; no initializer observes a partially published module.


<a id="instruction-set"></a>
## Instruction set
### C.1 Common instruction grammar

Every instruction starts with exactly `family:u8, subopcode:u8`. Valid families are `00` control, `01` integer, `02` scalar FP, `03` conversion, `04` SIMD, `05` reference/table, `06` linear memory, `07` atomics, `08` thread, `09` constants, and `0A` globals/resources. Every table below specifies all immediate bytes, reference order/types, result arity/types, and dynamic failure. An absent immediate is encoded as zero bytes; an immediate is never inferred from an operand.

A reference operand is canonical `uLEB32` in the order shown. `MemArg` is `memidx:uLEB32` followed by `offset:u64-le`; an address operand follows it. An effective address is checked as `address + offset + access_size` without wrap before any byte access. A result slot is allocated only after the instruction's effect and is never speculative. `select` has refs `(condition:i32,true:T,false:T)` and one result `T`; only zero is false.

### C.2 Control family `00`

| sub | exact encoding after family/sub | refs in order | results | dynamic rule |
|---:|---|---|---|---|
| `00` | none (`nop`) | none | none | no effect |
| `01` | `trap_code:u16-le` | none | none | code MUST be in C.12 legal set; instance trap |
| `02` | `target:uLEB32` then `args` | `args[target.params]` | terminator | typed edge transfer; bad target/type is `bad_cfg`/`bad_type` |
| `03` | `true_target:uLEB32,false_target:uLEB32` | `cond:i32`, true args, false args | terminator | zero chooses false; both edge shapes checked |
| `04` | `case_count:uLEB32`, each target, then default target | selector `i32`, each edge args | terminator | selector is unsigned; out-of-range selects default |
| `05` | none | function result refs in result order | terminator | return exact function results |
| `06` | `funcidx:uLEB32` | signature parameter refs | signature results | direct call; callee trap terminates instance |
| `07` | `tableidx:uLEB32,sigidx:uLEB32` | `index:i32`, signature parameter refs | signature results | index unsigned; OOB/null/signature traps |
| `08` | `funcidx:uLEB32` | parameter refs | terminator | tail-call result signature equals caller |
| `09` | `tableidx:uLEB32,sigidx:uLEB32` | `index:i32`, parameter refs | terminator | same checks as indirect tail call |
| `0A` | `sigidx:uLEB32` | `funcref<S>`, parameter refs | signature results | null/signature traps; no computed native jump |
| `0B` | `sigidx:uLEB32` | `funcref<S>`, parameter refs | terminator | tail-call form of `call_ref` |
| `0C` | none | `cond:i32,true:T,false:T` | `T` | no implicit conversion |
| `0D` | none (`unreachable`) | none | terminator | `unreachable` trap |
| `0E` | none (`safepoint`) | none | none | scheduler/debug point; no guest result |

`br_if` and `br_table` encode every edge's argument vector in the listed edge order; counts are derived from target block parameters and are not duplicated. Calls recurse only within the bounded stack envelope. Tail calls replace the current frame.

### C.3 Integer family `01`

After family/sub, every integer row has `width:u8` exactly `20` (32 bits) or `40` (64 bits), then references as shown. A `W` operand is an `i32` for width `20` and `i64` for width `40`. Integer bit patterns are modulo `2^W`; signed interpretations are two's complement. For `shl`, `shr_s`, and `shr_u`, the count is the unsigned bit pattern of the declared width `W`; a count `>= W` traps `shift_oob` and is never host-masked. For `rotl` and `rotr`, the same unsigned count is reduced modulo `W` before rotation.

| sub | refs in order | results | exact rule |
|---:|---|---|---|
| `00 add_wrap` | `a:W,b:W` | `W` | low W bits of sum |
| `01 sub_wrap` | `a:W,b:W` | `W` | low W bits of difference |
| `02 mul_wrap` | `a:W,b:W` | `W` | low W bits of product |
| `03 add_checked_s` | `a:W,b:W` | `W` | signed overflow traps |
| `04 sub_checked_s` | `a:W,b:W` | `W` | signed overflow traps |
| `05 mul_checked_s` | `a:W,b:W` | `W` | signed overflow traps |
| `06 add_checked_u` | `a:W,b:W` | `W` | unsigned overflow traps |
| `07 sub_checked_u` | `a:W,b:W` | `W` | unsigned underflow traps |
| `08 mul_checked_u` | `a:W,b:W` | `W` | unsigned overflow traps |
| `09 add_sat_s` | `a:W,b:W` | `W` | signed clamp |
| `0A sub_sat_s` | `a:W,b:W` | `W` | signed clamp |
| `0B add_sat_u` | `a:W,b:W` | `W` | unsigned clamp |
| `0C sub_sat_u` | `a:W,b:W` | `W` | clamp below zero |
| `0D neg_wrap` | `a:W` | `W` | low W bits of signed negation |
| `0E neg_checked_s` | `a:W` | `W` | signed minimum traps |
| `0F neg_sat_s` | `a:W` | `W` | signed minimum maps to signed maximum |
| `10 div_s_trunc` | `a:W,b:W` | `W` | toward-zero quotient; zero or min/−1 traps |
| `11 div_u` | `a:W,b:W` | `W` | unsigned quotient; zero traps |
| `12 rem_s_trunc` | `a:W,b:W` | `W` | dividend-sign remainder; zero traps |
| `13 rem_u` | `a:W,b:W` | `W` | unsigned remainder; zero traps |
| `14 div_s_floor` | `a:W,b:W` | `W` | floor quotient; zero/unrepresentable traps |
| `15 rem_s_floor` | `a:W,b:W` | `W` | `a-b*q` for floor q |
| `16 div_s_euclid` | `a:W,b:W` | `W` | quotient paired with nonnegative remainder |
| `17 rem_s_euclid` | `a:W,b:W` | `W` | `0 <= r < abs(b)` |
| `18 and` | `a:W,b:W` | `W` | bitwise AND |
| `19 or` | `a:W,b:W` | `W` | bitwise OR |
| `1A xor` | `a:W,b:W` | `W` | bitwise XOR |
| `1B not` | `a:W` | `W` | bitwise complement |
| `1C shl` | `a:W,count:W` | `W` | unsigned count `<W`; otherwise `shift_oob`; zero-fill |
| `1D shr_s` | `a:W,count:W` | `W` | unsigned count `<W`; otherwise `shift_oob`; sign-fill |
| `1E shr_u` | `a:W,count:W` | `W` | unsigned count `<W`; otherwise `shift_oob`; zero-fill |
| `1F rotl` | `a:W,count:W` | `W` | unsigned count reduced modulo W |
| `20 rotr` | `a:W,count:W` | `W` | unsigned count reduced modulo W |
| `21 clz` | `a:W` | `W` | all-zero gives W |
| `22 ctz` | `a:W` | `W` | all-zero gives W |
| `23 popcnt` | `a:W` | `W` | count in [0,W] |
| `24 bitreverse` | `a:W` | `W` | reverse W bits |
| `25 eq` | `a:W,b:W` | `i32` | bit equality |
| `26 ne` | `a:W,b:W` | `i32` | bit inequality |
| `27 lt_s` | `a:W,b:W` | `i32` | signed comparison |
| `28 lt_u` | `a:W,b:W` | `i32` | unsigned comparison |
| `29 le_s` | `a:W,b:W` | `i32` | signed comparison |
| `2A le_u` | `a:W,b:W` | `i32` | unsigned comparison |
| `2B gt_s` | `a:W,b:W` | `i32` | signed comparison |
| `2C gt_u` | `a:W,b:W` | `i32` | unsigned comparison |
| `2D ge_s` | `a:W,b:W` | `i32` | signed comparison |
| `2E ge_u` | `a:W,b:W` | `i32` | unsigned comparison |

`div_s_floor`/`rem_s_floor` preserve `a = b*q+r` and divisor-sign remainder; `div_s_euclid`/`rem_s_euclid` preserve a nonnegative remainder. These are fixed-width machine operations, not Jet arbitrary-precision `Int` operations.
**M-ISA-DIV-004.** For width `W`, signed operands are interpreted as two's-complement values. For every `div_s_trunc`, `rem_s_trunc`, `div_s_floor`, `rem_s_floor`, `div_s_euclid`, and `rem_s_euclid` row, divisor zero is checked first and traps `integer_divide_by_zero (0002)`. The mathematical quotient is truncation toward zero, floor, or the unique Euclidean quotient with `0 <= r < abs(b)`; the remainder is `a - b*q` (trunc/floor) or the nonnegative Euclidean remainder. Quotient rows trap `integer_overflow (0003)` only when the mathematical quotient is outside signed W-bit range. Remainder rows never trap quotient overflow when the representable remainder is defined; `INT_MIN % -1` is zero.

### C.4 Scalar FP family `02`

After family/sub, `width:u8` is `20` for f32 or `40` for f64. The payload is an IEEE binary bit pattern. All arithmetic is round-to-nearest-even, gradual-underflow, no ambient rounding mode, no implicit contraction/reassociation, and canonical quiet NaN output (`0x7FC00000` or `0x7FF8000000000000`) for any NaN input or invalid arithmetic operation. Signaling NaNs are treated as NaNs and canonicalized by arithmetic.

| sub | refs | results | exact rule |
|---:|---|---|---|
| `00 add` | `a:F,b:F` | F | IEEE correctly rounded |
| `01 sub` | `a:F,b:F` | F | IEEE correctly rounded |
| `02 mul` | `a:F,b:F` | F | IEEE correctly rounded |
| `03 div` | `a:F,b:F` | F | IEEE zero/infinity rules; no divide trap |
| `04 sqrt` | `a:F` | F | `sqrt(+0)=+0`, `sqrt(-0)=-0`, negative nonzero including `-inf` => canonical NaN, `sqrt(+inf)=+inf` |
| `05 abs` | `a:F` | F | clear sign; NaN canonical |
| `06 neg` | `a:F` | F | invert sign; NaN canonical |
| `07 ceil` | `a:F` | F | NaN canonical, infinities unchanged, signed zeros preserved, finite correctly rounded integral value |
| `08 floor` | `a:F` | F | same special table, floor direction |
| `09 trunc` | `a:F` | F | same special table, toward zero |
| `0A nearest_even` | `a:F` | F | same special table, ties to even |
| `0B min_num` | `a:F,b:F` | F | one NaN returns other; both NaN canonical; equal zero chooses −0 |
| `0C max_num` | `a:F,b:F` | F | one NaN returns other; both NaN canonical; equal zero chooses +0 |
| `0D copysign` | `a:F,b:F` | F | first magnitude, second sign; NaN canonical |
| `0E fma` | `a:F,b:F,c:F` | F | one final correctly rounded `a*b+c` |
| `0F eq` | `a:F,b:F` | `i32` | ordered; NaN false; ±0 equal |
| `10 ne` | `a:F,b:F` | `i32` | NaN true; ±0 equal |
| `11 lt` | `a:F,b:F` | `i32` | ordered; NaN false |
| `12 le` | `a:F,b:F` | `i32` | ordered; NaN false |
| `13 gt` | `a:F,b:F` | `i32` | ordered; NaN false |
| `14 ge` | `a:F,b:F` | `i32` | ordered; NaN false |
| `15 is_nan` | `a:F` | `i32` | any NaN encoding |
| `16 is_inf` | `a:F` | `i32` | either infinity |
| `17 is_finite` | `a:F` | `i32` | not NaN/infinity |
| `18 sign_bit` | `a:F` | `i32` | stored sign, including NaN and zero |

The table is exhaustive for ±0, ±infinity, qNaN, sNaN, and ordinary finite values. `reinterpret` is in C.5 and preserves all bits. A source `round` law that differs from `nearest_even` is a Prelude contract and carries its numeric digest.

### C.5 Conversion family `03`

Each row has one reference and one result of the named type. Integer reinterpret rows preserve bits. Trapping truncation rejects NaN, infinity, or a value outside the exact interval below; saturating truncation clamps.

| sub | operation | refs/results | dynamic rule |
|---:|---|---|---|
| `00` | `i32.wrap_i64` | `i64 -> i32` | low 32 bits |
| `01` | `i64.extend_i32_s` | `i32 -> i64` | sign extension |
| `02` | `i64.extend_i32_u` | `i32 -> i64` | zero extension |
| `03` | `i32.trunc_f32_s` | `f32 -> i32` | valid iff `-2^31 <= x < 2^31`; trunc toward zero |
| `04` | `i32.trunc_f32_u` | `f32 -> i32` | valid iff `0 <= x < 2^32` |
| `05` | `i32.trunc_f64_s` | `f64 -> i32` | same signed interval |
| `06` | `i32.trunc_f64_u` | `f64 -> i32` | same unsigned interval |
| `07` | `i64.trunc_f32_s` | `f32 -> i64` | valid iff `-2^63 <= x < 2^63` |
| `08` | `i64.trunc_f32_u` | `f32 -> i64` | valid iff `0 <= x < 2^64` |
| `09` | `i64.trunc_f64_s` | `f64 -> i64` | same signed interval |
| `0A` | `i64.trunc_f64_u` | `f64 -> i64` | same unsigned interval |
| `0B` | `i32.trunc_sat_f32_s` | `f32 -> i32` | NaN→0; clamp to signed endpoints |
| `0C` | `i32.trunc_sat_f32_u` | `f32 -> i32` | NaN→0; clamp to unsigned endpoints |
| `0D` | `i32.trunc_sat_f64_s` | `f64 -> i32` | NaN→0; clamp to signed endpoints |
| `0E` | `i32.trunc_sat_f64_u` | `f64 -> i32` | NaN→0; clamp to unsigned endpoints |
| `0F` | `i64.trunc_sat_f32_s` | `f32 -> i64` | NaN→0; clamp to signed endpoints |
| `10` | `i64.trunc_sat_f32_u` | `f32 -> i64` | NaN→0; clamp to unsigned endpoints |
| `11` | `i64.trunc_sat_f64_s` | `f64 -> i64` | NaN→0; clamp to signed endpoints |
| `12` | `i64.trunc_sat_f64_u` | `f64 -> i64` | NaN→0; clamp to unsigned endpoints |
| `13` | `f32.demote_f64` | `f64 -> f32` | correctly rounded; finite overflow→signed infinity; ±0/sign and subnormals preserved; NaN→canonical f32 NaN |
| `14` | `f64.promote_f32` | `f32 -> f64` | exact finite/zero/subnormal; NaN→canonical f64 NaN |
| `15` | `f32.convert_i32_s` | `i32 -> f32` | correctly rounded |
| `16` | `f32.convert_i32_u` | `i32 -> f32` | correctly rounded |
| `17` | `f32.convert_i64_s` | `i64 -> f32` | correctly rounded |
| `18` | `f32.convert_i64_u` | `i64 -> f32` | correctly rounded |
| `19` | `f64.convert_i32_s` | `i32 -> f64` | correctly rounded |
| `1A` | `f64.convert_i32_u` | `i32 -> f64` | correctly rounded |
| `1B` | `f64.convert_i64_s` | `i64 -> f64` | correctly rounded |
| `1C` | `f64.convert_i64_u` | `i64 -> f64` | correctly rounded |
| `1D` | `i32.reinterpret_f32` | `f32 -> i32` | bit-preserving |
| `1E` | `f32.reinterpret_i32` | `i32 -> f32` | bit-preserving |
| `1F` | `i64.reinterpret_f64` | `f64 -> i64` | bit-preserving |
| `20` | `f64.reinterpret_i64` | `i64 -> f64` | bit-preserving |
| `21` | `i32.extend8_s` | `i32 -> i32` | sign extend low 8 bits |
| `22` | `i32.extend16_s` | `i32 -> i32` | sign extend low 16 bits |
| `23` | `i64.extend8_s` | `i64 -> i64` | sign extend low 8 bits |
| `24` | `i64.extend16_s` | `i64 -> i64` | sign extend low 16 bits |
| `25` | `i64.extend32_s` | `i64 -> i64` | sign extend low 32 bits |

For trapping truncation, NaN and infinity are `invalid_conversion`; a boundary exactly equal to the signed minimum is valid, exactly equal to the positive signed endpoint is invalid, exactly `2^W-1` is valid unsigned, and values in `[2^W-1,2^W)` truncate to the largest unsigned integer. Saturating results are deterministic and never trap for a numeric input.

### C.6 SIMD family `04`

`v128` is 16 bytes, lane zero at the low-order end. Lane codes are `01=i8x16`, `02=i16x8`, `03=i32x4`, `04=i64x2`, `05=f32x4`, `06=f64x2`; unsupported combinations reject. A lane index is one immediate `u8` and is less than the lane count. Binary rows use refs `(a:v128,b:v128)` unless noted; unary rows use `(a:v128)`; all results are `v128` except extraction/predicates.

| sub | immediate bytes | refs in order | result/rule |
|---:|---|---|---|
| `00 splat` | `lane_code:u8` | scalar: i32 for i8/i16/i32, i64 for i64, F for FP | replicate; i8/i16 take low bits |
| `01 extract_lane` | `lane_code:u8,lane:u8` | `a:v128` | scalar; i8/i16 zero-extend to i32 |
| `02 extract_lane_s` | `lane_code:u8,lane:u8` | `a:v128` | i8/i16 sign-extend to i32; others as ordinary |
| `03 extract_lane_u` | `lane_code:u8,lane:u8` | `a:v128` | only i8/i16; zero-extend to i32 |
| `04 replace_lane` | `lane_code:u8,lane:u8` | `a:v128,scalar` | replace; i8/i16 low bits |
| `05 not` | none | `a:v128` | bitwise complement |
| `06 and` | none | `a:v128,b:v128` | bitwise AND |
| `07 or` | none | `a:v128,b:v128` | bitwise OR |
| `08 xor` | none | `a:v128,b:v128` | bitwise XOR |
| `09 bitselect` | none | `a:v128,b:v128,mask:v128` | `(mask&a)|(~mask&b)` |
| `0A any_true` | none | `a:v128` | i32 1 iff any bit set |
| `0B all_true` | `lane_code:u8` | `a:v128` | i32 1 iff every lane is nonzero |
| `0C shuffle` | 16 raw `u8` indices | `a:v128,b:v128` | index 0..31 from concatenated `(a,b)` |
| `0D swizzle` | `lane_code:u8` | `a:v128,b:v128` | each index byte selects a byte of a or zero if >=16; integer lanes only |
| `0E eq` | `lane_code:u8` | `a:v128,b:v128` | all-zero/all-one lane mask; lane codes 01..06 |
| `0F ne` | `lane_code:u8` | `a:v128,b:v128` | all-zero/all-one lane mask; lane codes 01..06 |
| `10 lt_s` | `lane_code:u8` | `a:v128,b:v128` | signed integer mask; lane codes 01..04 |
| `11 lt_u` | `lane_code:u8` | `a:v128,b:v128` | unsigned integer mask; lane codes 01..04 |
| `12 le_s` | `lane_code:u8` | `a:v128,b:v128` | signed integer mask; lane codes 01..04 |
| `13 le_u` | `lane_code:u8` | `a:v128,b:v128` | unsigned integer mask; lane codes 01..04 |
| `14 gt_s` | `lane_code:u8` | `a:v128,b:v128` | signed integer mask; lane codes 01..04 |
| `15 gt_u` | `lane_code:u8` | `a:v128,b:v128` | unsigned integer mask; lane codes 01..04 |
| `16 ge_s` | `lane_code:u8` | `a:v128,b:v128` | signed integer mask; lane codes 01..04 |
| `17 ge_u` | `lane_code:u8` | `a:v128,b:v128` | unsigned integer mask; lane codes 01..04 |
| `18 add_wrap` | `lane_code:u8` | `a:v128,b:v128` | integer modulo lane width; lane codes 01..04 |
| `19 sub_wrap` | `lane_code:u8` | `a:v128,b:v128` | integer modulo lane width; lane codes 01..04 |
| `1A mul_wrap` | `lane_code:u8` | `a:v128,b:v128` | only lane codes 02,03,04; 01 rejects |
| `1B add_sat_s` | `lane_code:u8` | `a:v128,b:v128` | signed saturating; lane codes 01,02 |
| `1C add_sat_u` | `lane_code:u8` | `a:v128,b:v128` | unsigned saturating; lane codes 01,02 |
| `1D sub_sat_s` | `lane_code:u8` | `a:v128,b:v128` | signed saturating; lane codes 01,02 |
| `1E sub_sat_u` | `lane_code:u8` | `a:v128,b:v128` | unsigned saturating; lane codes 01,02 |
| `1F min_s` | `lane_code:u8` | `a:v128,b:v128` | signed integer min; lane codes 01..03 |
| `20 min_u` | `lane_code:u8` | `a:v128,b:v128` | unsigned integer min; lane codes 01..03 |
| `21 max_s` | `lane_code:u8` | `a:v128,b:v128` | signed integer max; lane codes 01..03 |
| `22 max_u` | `lane_code:u8` | `a:v128,b:v128` | unsigned integer max; lane codes 01..03 |
| `23 shl` | `lane_code:u8` | `a:v128,count:i32` | unsigned count per lane; count >= width gives zero |
| `24 shr_s` | `lane_code:u8` | `a:v128,count:i32` | unsigned count per lane; count >= width gives sign fill |
| `25 shr_u` | `lane_code:u8` | `a:v128,count:i32` | unsigned count per lane; count >= width gives zero |
| `26 abs` | `lane_code:u8` | `a:v128` | integer signed abs (minimum unchanged) or FP abs |
| `27 neg` | `lane_code:u8` | `a:v128` | integer two's-complement negation or FP sign inversion |
| `28 popcnt` | `lane_code:u8` | `a:v128` | per-lane integer population count; lane codes 01..04 |
| `29 fadd` | `lane_code:u8` | `a:v128,b:v128` | FP add; lane codes 05,06 |
| `2A fsub` | `lane_code:u8` | `a:v128,b:v128` | FP subtract; lane codes 05,06 |
| `2B fmul` | `lane_code:u8` | `a:v128,b:v128` | FP multiply; lane codes 05,06 |
| `2C fdiv` | `lane_code:u8` | `a:v128,b:v128` | FP divide; lane codes 05,06 |
| `2D fsqrt` | `lane_code:u8` | `a:v128` | FP sqrt; lane codes 05,06 |
| `2E fmin_num` | `lane_code:u8` | `a:v128,b:v128` | scalar min_num; lane codes 05,06 |
| `2F fmax_num` | `lane_code:u8` | `a:v128,b:v128` | scalar max_num; lane codes 05,06 |
| `30 fma` | `lane_code:u8` | `a:v128,b:v128,c:v128` | one-rounding FP multiply-add; lane codes 05,06 |
| `31 trunc_sat_s` | `lane_code:u8` | `a:v128` | f32x4/f64x2 to low i32 lanes, NaN zero, signed clamp; lane codes 05,06 |
| `32 trunc_sat_u` | `lane_code:u8` | `a:v128` | same with unsigned clamp; lane codes 05,06 |
| `33 convert_s` | `lane_code:u8` | `a:v128` | low i32 lanes to f32x4/f64x2; lane codes 05,06 |
| `34 convert_u` | `lane_code:u8` | `a:v128` | unsigned low i32 lanes to f32x4/f64x2; lane codes 05,06 |
| `35 extend_low_s` | `lane_code:u8` | `a:v128` | signed low-half i8→i16, i16→i32, or i32→i64; lane codes 01..03 |
| `36 extend_high_s` | `lane_code:u8` | `a:v128` | signed high-half extension; lane codes 01..03 |
| `37 extend_low_u` | `lane_code:u8` | `a:v128` | unsigned low-half extension; lane codes 01..03 |
| `38 extend_high_u` | `lane_code:u8` | `a:v128` | unsigned high-half extension; lane codes 01..03 |

The exact two-vector order for `shuffle` is `(a,b)`, and `bitselect` order is `(a,b,mask)`. Scalar extraction extension and scalar insertion low-bit rules are part of decoding, not host SIMD choice. Shift counts are unsigned i32 bit patterns and are not masked. Unsupported lane combinations are malformed, not scalarized with a different result.
**M-SIMD-SEMANTICS-007.** SIMD `f32x4.eq/ne` and `f64x2.eq/ne` use the scalar numeric relation: NaN is unequal to every value including itself, `+0` equals `-0`; `eq` emits all-zero/all-one raw lane bits and `ne` emits the inverse. `all_true` tests raw lane bits: every lane must be nonzero, so `+0` is false and `-0` is true. For `trunc_sat_*` lane code 05, source lanes 0..3 become i32 lanes 0..3; for lane code 06, source lanes 0..1 become i32 lanes 0..1 and i32 lanes 2..3 are zero. For `convert_s/u` lane code 05, i32 lanes 0..3 become f32 lanes 0..3; for lane code 06, i32 lanes 0..1 become f64 lanes 0..1. Every unused output bit is zero; NaN saturation yields zero per C.5.

### C.7 References and tables family `05`

| sub | immediate bytes | refs in order | results/effect |
|---:|---|---|---|
| `00 ref.null` | `RefType` | none | null typed reference |
| `01 ref.is_null` | none | `r:Ref` | i32 1 only for null; closed/stale is non-null |
| `02 ref.eq` | none | `a:Ref,b:Ref` | i32 identity equality, including null; operands MUST have the identical resolved `RefType` (`funcref<S>` with the same signature or `resref<T>` with the same nominal type); mixed or unresolved types reject `bad_type`; rights do not affect equality |
| `03 ref.func` | `funcidx:uLEB32,sigidx:uLEB32` | none | resolved function ref; signature exact |
| `04 table.get` | `tableidx:uLEB32` | `index:i32` | element; unsigned OOB traps |
| `05 table.set` | `tableidx:uLEB32` | `index:i32,value:elem` | typed store; OOB before effect |
| `06 table.size` | `tableidx:uLEB32` | none | current size as i32 unsigned bit pattern |
| `07 table.grow` | `tableidx:uLEB32` | `fill:elem,delta:i32` | old size as i32; failure `0xFFFF_FFFF`, no mutation |
| `08 table.fill` | `tableidx:uLEB32` | `start:i32,value:elem,count:i32` | unsigned ranges, then fill |
| `09 table.copy` | `dst:uLEB32,src:uLEB32` | `dst:i32,src:i32,count:i32` | same element type; overlap-safe |
| `0A table.init` | `tableidx:uLEB32,elemidx:uLEB32` | `dst:i32,src:i32,count:i32` | passive segment only; range before writes |
| `0B table.drop` | `elemidx:uLEB32` | none | passive only; repeated drop no-op |

Table size/delta/start/source/count are unsigned i32 bit patterns. `max` is at most `0xFFFF_FFFE`, so `0xFFFF_FFFF` is unambiguous failure. Zero delta succeeds after ordinary admission and returns the old size. Active indices in `table.init`/`table.drop` are verifier `bad_segment_mode`. A table entry stores an alias; it does not implicitly retain a resource binding. Removing, replacing, dropping, or growing a table/value never invokes JPH close and never closes a broker binding; only the explicit resource close/drop transition does so.
**M-ISA-MUT-005/M-ISA-TABLE-006.** `table.set`, `table.grow`, `table.fill`, `table.copy` when used as destination, and `table.init` require a statically mutable destination table; a nonmutable destination is `bad_type` before dynamic range/effect. `table.copy` also requires exact source/destination element TypeId. `table.init` with an active segment, or an active target that is imported/shared, is `bad_segment_mode`/`bad_segment_target` at verification. Dynamic-write preflight is table index/type, destination mutability, dropped/source mode, unsigned ranges, then effect. Any range failure traps `table_oob (0008)` with unchanged table; `table.grow` failure preserves its all-ones result sentinel and unchanged table.

### C.8 Linear memory family `06`

Every row with a memory access starts with `MemArg(memidx:uLEB32,offset:u64-le)` and then address/value refs:

| sub/operation | refs after immediate | result/effect |
|---|---|---|
| `00 i32.load` | `addr:i64` | i32 from 4 bytes |
| `01 i32.load8_s` | `addr:i64` | i32 sign-extended from 1 byte |
| `02 i32.load8_u` | `addr:i64` | i32 zero-extended from 1 byte |
| `03 i32.load16_s` | `addr:i64` | i32 sign-extended from 2 bytes |
| `04 i32.load16_u` | `addr:i64` | i32 zero-extended from 2 bytes |
| `05 i64.load` | `addr:i64` | i64 from 8 bytes |
| `06 i64.load8_s` | `addr:i64` | i64 sign-extended from 1 byte |
| `07 i64.load8_u` | `addr:i64` | i64 zero-extended from 1 byte |
| `08 i64.load16_s` | `addr:i64` | i64 sign-extended from 2 bytes |
| `09 i64.load16_u` | `addr:i64` | i64 zero-extended from 2 bytes |
| `0A i64.load32_s` | `addr:i64` | i64 sign-extended from 4 bytes |
| `0B i64.load32_u` | `addr:i64` | i64 zero-extended from 4 bytes |
| `0C f32.load` | `addr:i64` | f32 bits from 4 bytes |
| `0D f64.load` | `addr:i64` | f64 bits from 8 bytes |
| `0E v128.load` | `addr:i64` | v128 from 16 bytes |
| `0F i32.store` | `addr:i64,value:i32` | none; writes 4 low bytes |
| `10 i32.store8` | `addr:i64,value:i32` | none; writes low byte |
| `11 i32.store16` | `addr:i64,value:i32` | none; writes low 2 bytes |
| `12 i64.store` | `addr:i64,value:i64` | none; writes 8 bytes |
| `13 i64.store8` | `addr:i64,value:i64` | none; writes low byte |
| `14 i64.store16` | `addr:i64,value:i64` | none; writes low 2 bytes |
| `15 i64.store32` | `addr:i64,value:i64` | none; writes low 4 bytes |
| `16 f32.store` | `addr:i64,value:f32` | none; writes f32 bits |
| `17 f64.store` | `addr:i64,value:f64` | none; writes f64 bits |
| `18 v128.store` | `addr:i64,value:v128` | none; writes 16 bytes |
| `19 memory.size` | `memidx:uLEB32` | none; current pages as i64 |
| `1A memory.grow` | `memidx:uLEB32`, `delta:i64` | old pages; failure all-ones i64 |
| `1B memory.fill` | `memidx:uLEB32`, `dst:i64,value:i32,count:i64` | low-byte fill |
| `1C memory.copy` | `dstmem:uLEB32,srcmem:uLEB32`, `dst:i64,src:i64,count:i64` | overlap-safe memmove |
| `1D memory.init` | `memidx:uLEB32,dataidx:uLEB32`, `dst:i64,src:i64,count:i64` | passive data only |
| `1E data.drop` | `dataidx:uLEB32` | passive drop only |

All ranges are checked before effect; multi-byte loads/stores are little-endian and may be unaligned except atomics. Memory size/grow deltas and counts are unsigned i64 bit patterns. Growth zero-initializes pages, cannot shrink, and returns all-ones on maximum/reservation failure with no change. Active segment writes are handled only during activation as specified in verification/memory chapters.
**M-SEG-DROP-003.** `data.drop` is valid only for a passive data segment; the first drop marks it dropped and repeated drops are successful no-ops. `memory.init` validates passive mode, then checks the dropped bit, then unsigned source/destination ranges; a dropped segment traps `segment_dropped (0010)` before range checks or writes. Active data indexes in `memory.init/drop` are verifier `bad_segment_mode`. No operation resurrects a dropped segment.

### C.9 Atomic family `07`

Atomics use the same `MemArg` bytes and address order as C.8. They are valid only on shared memory, naturally aligned, and sequentially consistent in one explicit event order. No relaxed/acquire/release immediate exists.

| sub/operation | refs after immediate | results |
|---|---|---|
| `00 i32.atomic.load` | `addr:i64` | old i32 |
| `01 i64.atomic.load` | `addr:i64` | old i64 |
| `02 i32.atomic.store` | `addr:i64,value:i32` | none |
| `03 i64.atomic.store` | `addr:i64,value:i64` | none |
| `04 i32.atomic.add` | `addr:i64,value:i32` | old i32; modulo 2^32 |
| `05 i64.atomic.add` | `addr:i64,value:i64` | old i64; modulo 2^64 |
| `06 i32.atomic.sub` | `addr:i64,value:i32` | old i32; modulo 2^32 |
| `07 i64.atomic.sub` | `addr:i64,value:i64` | old i64; modulo 2^64 |
| `08 i32.atomic.and` | `addr:i64,value:i32` | old i32 |
| `09 i64.atomic.and` | `addr:i64,value:i64` | old i64 |
| `0A i32.atomic.or` | `addr:i64,value:i32` | old i32 |
| `0B i64.atomic.or` | `addr:i64,value:i64` | old i64 |
| `0C i32.atomic.xor` | `addr:i64,value:i32` | old i32 |
| `0D i64.atomic.xor` | `addr:i64,value:i64` | old i64 |
| `0E i32.atomic.exchange` | `addr:i64,value:i32` | old i32 |
| `0F i64.atomic.exchange` | `addr:i64,value:i64` | old i64 |
| `10 i32.atomic.compare_exchange` | `addr:i64,expected:i32,desired:i32` | `(old:i32,success:i32)` |
| `11 i64.atomic.compare_exchange` | `addr:i64,expected:i64,desired:i64` | `(old:i64,success:i32)` |
| `12 atomic.fence` | none | none |
| `13 atomic.wait32` | `addr:i64,expected:i32,timeout:i64` | status i32 |
| `14 atomic.wait64` | `addr:i64,expected:i64,timeout:i64` | status i32 |
| `15 atomic.notify` | `addr:i64,count:i32` | woken count i32 |


**M-ATOMIC-WAIT-004/M-ATOMIC-RESULT-005.** Atomic scalar memory representations are always little-endian, including load/store/RMW/CAS values. Compare-exchange atomically loads old, compares bit patterns, stores desired only on equality, and returns `(old, success)` where `success` is exactly i32 `1` on equality and `0` on mismatch. `atomic.wait32/64` timeout is signed i64 nanoseconds measured from the successful `W-register` event on the profile's declared monotonic clock (`ClockInfo.clock_kind=1`, `unit=0`, epoch fixed by profile and included in replay). `-1` removes only the per-wait deadline; zero emits an immediate timeout after W-check/registration if no notify is already ordered; values `<-1` and checked deadline overflow trap `invalid_argument (0011)`. Replay records clock identity, registration event, computed deadline (or none), and winner. Notify uses unsigned i32 count and requires a 4-byte-aligned address; a wait64 waiter additionally requires 8-byte alignment at registration. Notify selects both wait32 and wait64 waiters at the exact address in ascending registration-event order; width is not an eligibility filter. No spurious wake is permitted.
### C.10 Threads, globals, and resources
`08/00 thread.spawn` encodes `funcidx:uLEB32`; refs are scalar/function arguments in signature order and result is `resref<jpm.thread>`. The function has `()->()` results after its argument types. `08/01 thread.join`, `08/02 thread.detach`, `08/03 thread.cancel`, and `08/06 thread.check_cancel` each have one `threadref` ref and result respectively `i32 status`, none, `i32 accepted`, and `i32 pending`. `08/04 thread.yield` and `08/05 thread.current` have no refs; current returns the typed current thread alias. Join statuses are `0=completed`, `1=cooperatively_cancelled`; there is no trapped status because any trap terminates the instance. Cancellation is observed at join/wait/check/call boundaries, not an asynchronous heap unwind.

`0A/00 global.get` has immediate `globalidx:uLEB32`, no refs, one exact global result. `0A/01 global.set` has the same immediate and ref `value:global_type`, no result; mutable only. `0A/02 resource.retain` has no immediate and ref `r:resref<T>`, result `resref<T>`; it creates one independent same-scope JPH binding and may trap `out_of_memory` before effect. `0A/03 resource.close` has no immediate and ref `r:resref<T>`, no result; it closes/releases that binding under JPH close/drop rules. Repeated close of the same locally closed binding is idempotent. There is no static `ref.drop` linear-consumption rule.

SSA copies, edge arguments, `select`, calls, and table storage copy an alias value without implicit retain. A resource must be explicitly retained when an independent lifetime is required. A function reference is an immutable resolved function identity owned by the linked instance generation; it is not retained/closed by a guest opcode.
**M-MEM-THREAD-006.** At activation, each module records immutable per-module private-state templates: memory page bytes and max, table element values/max, global scalar values, null reference values, passive segment bytes/elements, and segment drop bits. `thread.spawn` reserves a complete child envelope before enqueue and initializes the child from those templates. It does not copy the parent's live private heap, globals, tables, aliases, or segment-drop bits, and it does not run module start again. Explicitly shared memories refer to the existing same-instance shared object; private memories are child-local. Tables and globals are never implicitly shared. Templates and spawn arguments containing aliases require broker validation of the same session/principal/domain, instance generation, authority/revocation epoch, TypeId, rights, and binding state; invalid or missing non-null references reject, never substitute null. Copying an alias does not implicitly retain it, and cross-authority, stale, closed, or foreign arguments reject with the existing typed outcomes.

**M-THREAD-007.** `thread.cancel` checks its binding first: locally closed yields `resource_closed`, and stale or revoked yields `resource_stale`. A valid completed target returns i32 `0` and has no effect. A valid runnable or parked target sets a sticky advisory cancellation flag and returns i32 `1`; a duplicate request also returns `1` without allocating or changing state. `thread.check_cancel` reads the calling task's own sticky flag as exactly i32 `1` or `0` and never clears it. JPM threads never detach from their owning isolated instance, and closing a `Thread` binding does not stop its worker; JPH task/process detach is separate. No asynchronous heap unwind occurs. Join/wait/call-return boundaries may report the existing cooperative-cancelled status `1`; committed effects retain their existing effect/quiescence receipts.

### C.11 Constants family `09`

| sub | immediate bytes | result |
|---:|---|---|
| `00` | `sLEB32` | i32 bit pattern |
| `01` | `sLEB64` | i64 bit pattern |
| `02` | `u32-le` | exact f32 bits, including NaN payload |
| `03` | `u64-le` | exact f64 bits |
| `04` | 16 raw bytes | exact v128 |
| `05` | `RefType` | null reference |

### C.12 Trap registry and exact failure classes

The `trap` immediate is one `u16-le` from this legal registry:

| code | name | source |
|---:|---|---|
| `0001` | unreachable | explicit/terminator |
| `0002` | integer_divide_by_zero | integer division/remainder |
| `0003` | integer_overflow | checked integer operation |
| `0004` | shift_oob | scalar shift count |
| `0005` | invalid_conversion | trapping conversion |
| `0006` | memory_oob | range/alignment-independent memory bound |
| `0008` | table_oob | table bound |
| `000A` | null_ref | null call/reference operation |
| `000B` | indirect_signature | indirect signature mismatch |
| `000C` | invalid_function | stale/unresolved function identity |
| `000D` | invalid_memory | invalid memory identity |
| `000E` | atomic_unaligned | atomic alignment |
| `000F` | atomic_not_shared | atomic private memory |
| `0010` | segment_dropped | passive segment after drop |
| `0011` | invalid_argument | invalid machine argument |
| `0012` | resource_revoked | machine-only revoked-resource operation |
| `0013` | cancelled | explicit machine cancellation trap only |
| `0014` | stack_overflow | instance stack envelope |
| `0015` | thread_limit | child admission |
| `0016` | fuel_exhausted | semantic fuel reservation |
| `0017` | deadline | enforced instance deadline |
| `0018` | host_fault | provider/containment violation, not ordinary JPH error |
| `0019` | engine_fault | impossible verified-state failure |
| `001A` | out_of_memory | guest/host-helper reservation with no typed result |
| `001B` | resource_closed | machine-only use of closed binding |
| `001C` | resource_stale | machine-only use of stale generation |

`0000`, `0007`, `0009`, and `001D..FFFF` are reserved and reject as `bad_type` when present in `trap`; no producer-defined trap range exists in JPM/1. Core `memory.grow` and `table.grow` never use OOM trap codes: they return their unambiguous all-ones failure bits. A JPH function's ordinary denial/stale/closed/quota/unsupported result is its typed result, not `host_fault` or `resource_revoked` unless its descriptor explicitly declares a machine-only operation.

Every listed runtime trap terminates the whole authority-isolated instance and all guest tasks. Frame cleanup releases worker-local aliases; it does not roll back memory, table writes, host effects, or broker commits. Engine fault additionally quarantines the affected engine instance. The receipt records the first trap code, instruction identity, and cleanup/termination phase.


<a id="verification"></a>
## Verification
### D.1 Bounded parser and exact limits

**M-VER-001.** Parsing uses one bounded cursor over an immutable byte snapshot. Minimum limits before dependent allocation are:

| object | limit |
|---|---:|
| complete artifact | 64 MiB |
| section payload | 64 MiB and remaining-file bound |
| signatures | 1,048,576 |
| parameters/results per signature | 1,024 each |
| resource rows | 65,536 |
| imported+defined functions | 1,048,576 |
| tables/memories | 64 each |
| declared table max | `0xFFFF_FFFE` |
| globals | 1,048,576 |
| blocks/function | 1,048,576 |
| instructions/function | 16,777,216 |
| passive segments | 1,048,576 |
| one data segment | 16 MiB |
| identifier | 128 bytes |
| debug map rows | 8,388,608 |
| ordinary stack | profile bound, never above 4,096 frames |

Counts are checked before multiplication/allocation with checked arithmetic. Lower host limits are admission failures, never semantic reinterpretations.

### D.2 Ordered verification algorithm

1. Check the eight-byte header and complete-file limit.
2. Scan section tags/lengths once; reject noncanonical varints, unknown/duplicate/out-of-order tags, payload overrun, and trailing bytes.
3. Parse types, interned signatures, resource rows, and every `ValType`; reject an illegal type byte or a reference to an absent type as `bad_type` (a bounded numeric index into an existing space is `bad_index`).
4. Parse imports, declarations, segments, code, exports, start, and debug. Validate flags, canonical sorting, identifiers, JPH descriptor fields, table max cap, memory limits, and constants without executing an initializer.
5. Resolve index spaces and direct same-authority links. Check exact max equality, element/mutability/shared identity, and provider current/min/max inequalities. Check JPH interface/function/signature/adapter identity.
6. Evaluate only constant expressions with checked i64 arithmetic. Reject negative/overflowing offsets and impossible declaration ranges.
7. For each function, require block zero, bounded block IDs, one final terminator per block, no post-terminator instruction, valid edge target, and exact edge parameter count/types. Mark reachability; an unreachable block is `bad_cfg`.
8. Decode every instruction's exact immediate bytes. Check the complete opcode table, width/lane combinations, reserved values, operand references, exact operand types/order, result arity/types, and dynamic index spaces.
9. Compute each block's `result_base[j]` prefix sum before resolving result refs. A result ref must have kind `10`, an index below the block's total result-slot count, and a producer ordinal strictly earlier than the use. Function/block parameter refs use their corresponding bounds. `kind=11`, future result, cross-block result, or index over `0x3FFF_FFFF` is `bad_ssa`.
10. Treat reference values as non-forgeable aliases, not a linear type state. Verify only static type/provenance: a resource operation receives `resref<T>`, a function operation receives matching `funcref<S>`, table element types match, and `resource.retain/close` appear only on resources. No path-dependent hold merge or implicit retain/drop is inferred. Runtime closed/stale status is handled by JPH or C.12 machine-only rules.
11. Validate memory/table access widths/ranges, passive-only init/drop indices, active segment target restrictions, atomic shared/alignment rules, thread built-in resource/type rules, and table size cap. Active rows may target only locally defined, nonshared state; a target imported/shared memory or table is `bad_segment_target`.
**M-VER-OUTCOME-003.** After verification and linking, activation reserves the complete initial instance envelope, creates private memories/tables/globals and passive segment state, applies active element/data segments in section order, resolves start, calls start, and publishes the generation. Active writes are only to local nonshared state. Failure before publication leaves no active generation. Active segment range failure or initial reservation failure returns `activation_failed` with a bounded reason (`segment_out_of_bounds`, `reservation_exhausted`, or `link_conflict`), not a runtime trap.

The malformed/activation/runtime outcome table is phase ordered:

| condition | phase | exact outcome |
|---|---|---|
| wrong/truncated header | parse | `bad_header` |
| unknown/duplicate/out-of-order section, missing required section, trailing bytes, invalid UTF-8/identifier, duplicate/unsorted row, body-count mismatch | parse | `bad_section` |
| overlong/unterminated/out-of-range integer, including packed value requiring more than uLEB32 | parse | `bad_varint` |
| unknown family/subopcode, illegal immediate shape, extra immediate bytes, reserved flag/lane/width, invalid debug entity/sentinel/location row | verify | `bad_type` for instruction/type bytes; `bad_debug` for debug bytes |
| bounded index outside its index space | verify | `bad_index` |
| invalid CFG/edge/terminator | verify | `bad_cfg` |
| future/cross-block/kind-11/decoded index over bound | verify | `bad_ssa` |
| passive operation uses active segment, or active target is imported/shared | verify | `bad_segment_mode` / `bad_segment_target` |
| JPH descriptor, TypeId, adapter map, Result kind, attachment, mode, or canonical AdapterContract mismatch | link | `bad_host_projection` |
| missing/mismatched source bundle | activation | `semantic_bundle_missing` / `semantic_bundle_mismatch` |
| start invocation traps | activation | `activation_failed { reason:start_trap, trap_code:u16, instruction_identity }`, no active generation |
| start returns a declared typed failure | activation | `activation_failed { reason:start_result, result_type_id, error_code }`, no active generation |
| runtime safety trap after publication | runtime | exact C.12 trap, whole-instance termination |

Earlier phases always win. A host typed denial/stale/closed/quota/unsupported result is never rewritten as a JPM verifier category or `host_fault`; a malformed host projection is never exposed as an ordinary host operation error.

### D.4 Required adversarial vectors

The corpus MUST include: nonzero header flags; overlong section length; unknown/duplicate/out-of-order section; count multiplication overflow; illegal resource index; each `kind=11`, packed index `0x40000000`, future result, and cross-block result; wrong edge shape; instruction after terminator; every invalid width/lane/immediate; every reserved trap code; active init/drop index; active shared/imported target; table max `0xFFFF_FFFF`; table grow zero-delta at `0xFFFF_FFFE`; provider max/current mismatch; JPH descriptor/adapter/TypeId/result-attachment projection mismatch yielding `bad_host_projection`; all FP special values and conversion boundaries; SIMD mul lanes `01/02/03/04`; scalar shift counts `W-1`, `W`, and `0xFFFF_FFFF` with `shift_oob` at/above width; rotate counts `W` and `0xFFFF_FFFF` reduced modulo W; wait notify/timeout/cancel event races; host snapshot/lease/commit races; absent/mismatched semantic bundle; cache helper/provider/policy identity changes; out-of-bound debug refs; forged resource bytes; stale/closed alias calls; and attempted dynamic/native-address opcodes.


<a id="memory-model"></a>
## Memory model
### E.1 Linear storage and growth

A memory is a zero-initialized byte array of `pages * 65,536` bytes. Private memories belong to one execution context; shared memories are explicitly declared and visible only to same-instance child tasks. Memory is not a host virtual-address promise. Engines may use guard pages, segmented storage, copying, or pools, but all implement the same checked u64 address/range rules.
**M-MEM-001.** Addresses, offsets, lengths, segment positions, page-to-byte conversions, and growth calculations are unsigned u64. `base+offset`, `start+count`, and page multiplication must be checked before host narrowing. No uninitialized byte is observable. Failed growth leaves size and bytes unchanged.

The language runtime owns arrays, strings, object headers, exact integers, exceptions, and heap layout in linear memory. JPM does not mandate a collector, tracing barrier, pointer representation, or GC. A managed frontend may implement moving GC and roots at calls/safepoints; an unmanaged frontend may use an allocator and explicit checked failure. Neither may place a host pointer in guest memory. No compulsory language GC is implied.

### E.2 OOM, reservations, and host-helper charges

**M-MEM-002.** The worker/broker separately charges committed private/shared memory, tables, stacks, typed reference bindings, queued calls/callbacks, JPH staging/leases, and JIT/AOT code. Charges are reserved before any externally visible effect and use saturating arithmetic. `memory.grow`/`table.grow` return all-ones on ordinary growth reservation failure.
**M-OOM-003.** There is no per-tier OOM choice. JPM engine bookkeeping/allocation reservation failure uses the declared machine `out_of_memory (001A)` trap and terminates the whole isolated instance. JPH host-service quota or staging/allocation failure uses the existing canonical `Error` category `exhausted` (domain 4, code 1) through the descriptor's typed Result; it is not a JPM trap. `memory.grow` and `table.grow` preserve their explicit all-ones failure sentinels and unchanged state, and do not become generic `table_oob`/OOM traps. Source-language heap allocation and error behavior remains the exact pinned Prelude/semantic-bundle adapter contract, not engine discretion. Interpreter, JIT, AOT, browser, and embedded tiers use these same rules; no OOM policy field or tier-specific fallback is added.
A future source/Prelude policy that needs a distinct allocation meaning must be one complete semantic-bundle identity before activation; this machine clause does not create a second schema.

Host helpers retain their reservation and charge until terminal completion, cancellation at a defined boundary, or an enforced kill/device-loss fence proves quiescence. A device-loss notification alone never releases retained charges or storage. A trap does not erase committed external effects or pretend rollback. A collector that cannot obtain a stop-the-world reservation reports its declared result/trap; it does not continue with stale roots.

**M-MEM-ALIAS-004.** Extend M-MEM-003's no-implicit-retain/no-implicit-close rule to mutable and immutable globals and imported-global initialization. `global.get` copies the opaque alias value; `global.set` replaces the slot after mutability/type checks and does not retain the new value or close the old value. A later operation through either SSA/global/table copy observes the same live/closed/stale state. Explicit `resource.retain` is required for independent lifetime; explicit `resource.close` is the only local release.

**M-REF-LIFETIME-004.** Machine checks type/provenance, null, binding state, and reservation in that order. `resource.retain(null)` and `resource.close(null)` use the existing `null_ref` outcome. `resource.retain` on a locally closed or host-closed binding yields `resource_closed`; stale or revoked yields `resource_stale`; it never creates or rebinds a binding. Retain reservation failure yields `out_of_memory` before effect. `resource.close` on a live binding releases that binding; close of any known binding, including locally closed, host-closed, stale, or revoked, is cleanup-idempotent success and can never close a replacement generation. Authorization/use remains `resource_closed` or `resource_stale` as appropriate. Foreign/invalid references cannot be manufactured and fail validation; cleanup status is distinct from authorization to use or retain.
### E.3 Tables, segments, and aliases

Table sizes, starts, counts, and deltas are unsigned i32 bit patterns with maximum `0xFFFF_FFFE`. Grow zero-delta returns the current size after admission. Passive segment bytes/elements and drop state are instance-local and cloned in child templates. Active segments are applied before publication and target only local nonshared tables/memories in section order. No imported/shared table or memory receives active initialization writes.

**M-MEM-003.** JPM reference values are non-forgeable aliases. A copy through SSA, branch edge, block parameter, select, call argument, return, or table storage copies the alias value without implicit retain. `resource.retain` makes an independent same-scope binding; `resource.close` releases that binding. Every duplicate value of the released binding subsequently observes closed/stale. An independently retained or delegated binding follows JPH lifetime rules. Closing an already locally closed alias is idempotent and cannot close a newly reused generation. Function refs are immutable resolved identities owned by the linked instance generation.

A resource alias contains an internal binding identity, not bytes. Typed JPH use of a closed/revoked binding returns its declared `closed`/`stale` result. Machine-only operations use `resource_closed` or `resource_stale` traps. `ref.is_null` is true only for null. `ref.eq` first requires identical resolved `RefType`: the same `funcref<S>` signature or the same nominal `resref<T>` type; a mixed `funcref`/`resref`, differing function signatures, differing nominal resource types, or unresolved type is `bad_type`. After that check it compares immutable identity and may compare stale/closed aliases without revealing authority. Rights, principals, channels, and attenuation do not change equality once the same object identity is established.

Resource identity is:

```
ResourceIdentity = (
  broker/session-epoch namespace,
  object_id,
  object_generation,
  nominal_resource_type_id
)
```

Retained/delegated aliases compare equal when this tuple is equal even if rights differ. Distinct opens may have distinct object IDs even for the same OS file. Function identity is `(linked provider-instance generation, function identity)`; import aliases and `ref.func` compare equal only after resolving to that tuple. Null is a separate value; closed/stale is non-null. No equality operation exposes a broker token or grants authority.

### E.4 Sequential consistency, waits, and host memory events

Each task has an instruction cursor, stack, private globals/tables/memories, and reference aliases. Shared memory is explicit. All shared ordinary operations and all atomics participate in one sequentially consistent event order `E`; there is no tearing, poison, relaxed order, or undefined race result. The schedule may vary live, but replay records the order needed to reproduce an outcome.

A wait performs these ordered events:

1. `W-check`: atomically load the word and compare with expected. Mismatch returns status 1 and never registers.
2. `W-register`: if it matched, append a waiter record `(instance_generation,task_id,waiter_id,address,width,registration_event)`. A notify ordered before `W-register` cannot wake it.
3. `W-terminal`: the first ordered eligible event among notify, timeout, or cancellation wins. Notify selects the lowest registration event among eligible waiters up to unsigned count; timeout is the exact deadline event; cancellation is the ordered cancellation event. No unrecorded spurious wake exists.
4. The waiter is removed once and returns status 0/2/3 according to the winning event. Replay stores waiter id, registration event, selected notify event (if any), competing terminal events, and winner.

Live fairness is not promised. `thread.cancel` creates a cancellation event; it does not rewrite an already committed notify winner. `thread.check_cancel` observes the pending advisory flag at an explicit guest point.

A JPH `MemorySlice` operation also participates in `E`:

* a copy-read snapshot is one atomic `H-snapshot` event for the complete range;
* a sealed-read range is immutable for its lease;
* an exclusive lease is an `H-lease-acquire` event, excludes guest writes/overlapping leases for its lifetime, and has a release event;
* mode-1 output uses an exclusive lease through validation and commit, or broker-owned staging followed by one atomic `H-commit` replacement event and lease release.

Concurrent guest writes to a borrowed host range are forbidden unless the lease/snapshot event establishes the required ordering. Revalidation after suspension without exclusion is insufficient. Callback and imported-call visibility follows the same event order and JPH call lifecycle; a tier cannot invent a fence or reorder a host commit.

Jet source concurrency remains a stricter compiler concern: ownership/sendability, immutable snapshots, channels, and `Shared<T>` do not become less strict merely because JPM gives unsafe races a defined schedule. Atomic64 and exact-`Int` atomic behavior remain Prelude contracts, not claims about a JPM i64 word.

### E.5 Safety limits and managed runtimes

The verifier proves JPM type, range, table, segment, and reference provenance. It does not prove that a language index names a live object, that a source borrow survives a call, that an unsafe FFI struct matches its ABI, or that a source race is absent. A managed guest retains its GC, reflection, exception, module-loader, and scheduler semantics in guest code. A source exception lowers to a checked result, tagged record, continuation, or runtime control record; it is not native unwinding and cannot catch a JPM hard trap.


<a id="execution-tiers"></a>
## Execution tiers
### F.1 Reference semantics and parity

**M-TIER-001.** The reference interpreter is the executable definition of JPM/1. It consumes only verified bytes and linked instance state and implements exact operand order, result slots, FP bits, memory/table effects, alias identity, event outcomes, trap point/category, and host adapter state. Baseline JIT, optimizing JIT, AOT, browser, mobile, and embedded tiers are differential-tested against it on positive, malformed, boundary, replay, and denial vectors. Scalar SIMD fallback is conforming only when bytes match.

A tier identity is receipt metadata, not guest semantics. Tier transitions preserve function/block/instruction maps, live value locations, alias identities, pending host calls, event order, and cancellation state. Deoptimization is required when an optimization cannot reconstruct the verified point; refusing source stepping is preferable to inventing a value.

### F.2 Published fuel schedule and safepoints

**M-TIER-002.** One complete byte-encoded `FuelSchedule/1` is shared by the reference interpreter and compiled tiers. Its canonical record is:

```
fuel_schedule_format:u8 = 01
base_cost_default:u32-le = 1
opcode_count:uLEB32
opcode_cost[opcode_count]: (family:u8,sub:u8,width_or_lane:u8,base:u32-le)
bulk_quantum:u32-le = 64
straight_line_quantum:u32-le = 4096
schedule_flags:u32-le = 0
```

`fuel_schedule_format=01` identifies the fixed formulas and event placement for this complete record. Rows are sorted and unique; in JPM/1 every accepted shape's fixed `base` is `1`. Changing a formula or event-placement rule requires a new schedule format and `FuelScheduleDigest`; there is no cost-table-only identity. `FuelScheduleDigest = H("FuelSchedule/1", complete_record)`.

Only executed JPM instruction flow and the listed dynamic boundary, admission, and completion charges consume semantic fuel; unexecuted instructions, static artifact bytes, and other unexecuted metadata do not. Each executed fixed-shape instruction charges its row base plus the fixed immediate/reference-byte formula. Dynamic charges are: `memory.grow`/`table.grow` by declared growth; `thread.spawn` by its row plus child-template/argument work; `atomic.wait` for registration and one terminal completion; and imported calls for input codec/attachment admission and output codec completion. Host/broker/native/device CPU, IO, wall, and byte quotas remain separately measured/accounted, not fictional deterministic fuel. Tier transition consumes no semantic fuel. Extra implementation safepoints do not add semantic fuel.

**M-SC-BULK-008/M-FUEL-009.** Scalar shared loads, stores, and atomics remain indivisible width-sized sequentially consistent events. For shared byte memories, `memory.fill`, `memory.copy`, and `memory.init` preflight and reservation cover the full request, and the entire computed bulk fuel cost is reserved before the first byte event; insufficient fuel produces no bulk writes. Bulk execution is not a whole-range atomic transaction: finite byte events participate in the same SC order as other guest events. Each copy byte read precedes its matching write, overlapping source bytes are read before overwrite, and fill/init perform one destination write per byte with immutable initialized source bytes. The same full preflight/reservation rule applies to private table fill/copy/init, without implying shared byte-event interleaving.

`bulk_quantum` and `straight_line_quantum` are safepoint/work quanta only, never a pay-per-chunk fuel mode. A canonical implicit check occurs exactly at the declared quantum boundary and at each listed logical boundary; coincident boundaries coalesce to one check/charge. Compiled batching preserves executed instruction charges, byte-event order, and the first failed instruction as interpretation; a deadline or instance trap may stop at a declared boundary after partial byte work. Preflight/type/range/segment/drop rejection leaves state unchanged. A compiler cannot batch across the point at which the interpreter would exhaust fuel; it must preserve that point or deopt before the next effect.

The engine reserves each required charge before its visible guest effect. If an imported output charge is insufficient after an external effect commits, the result remains committed and the instance traps `fuel_exhausted`; the receipt records both. `FuelScheduleDigest` is part of SemanticBundle and cache identity.

### F.3 Native cache identity and admission

**M-TIER-003.** Execution-tier and package admission use exactly the `NativeCacheKey/2` schema owned by `P-CACHE-001`. This machine chapter defines no `CacheSemanticBundle`, no second tuple, and no alternate field names or types. A source-bound key carries the `semantic_bundle_digest` required by that schema; cache admission resolves and validates the complete graph-owned `SemanticBundle` before accepting the key. The canonical key explicitly covers the provider and shim implementation identities, JPM/JPH versions and interfaces, foreign runtime/adapter identity, fuel schedule and every imported-call/input-codec/output-codec/host-effect charge identity, compiler/engine/target/features/ABI, optimizer and mitigation flags, debug/deopt schema, cache format, and specialized policy. A Prelude/CoreLib/JPH helper may be inlined only when the exact implementation/content and provider identities are present in that one key; otherwise the call remains an adapter boundary. A stale, corrupt, revoked, or key-mismatched cache is `bad_cache`, not a cache miss.

### F.4 Browser, restricted, and plugin/native boundaries

A browser backend may lower JPM to browser Wasm/JS, but browser machinery is an implementation backend, not JPM identity. It preserves u64 checks, FP baseline, alias/reference separation, trap scope, event order, JPH origin, and attachment authority. A denied browser API is an explicit JPH result or pre-activation refusal, never ambient access.

Mobile and JIT-forbidden profiles use a verified interpreter or pre-signed AOT path, subject to the same cache and semantic identities. No-MMU profiles declare finite stacks, memories, queues, device authorities, and contiguous-allocation failures; they do not claim process isolation. GPU/ML/device services are JPH device interfaces with typed queues, lease/transfer accounting, asynchronous validation, and device-loss outcomes.

Existing trusted-native and isolated-native-helper/plugin contracts remain in force until a separate ratified clean cutover. This machine amendment adds one lifetime rule: a plugin/native helper call is bound to the linked instance generation and its JPH call id; if a JPM hard trap terminates that instance, no helper may re-enter or publish a guest result to the dead generation. A separately killable helper remains charged and retains storage until terminal completion/kill or an enforced device-loss fence proves quiescence; a device-loss notification alone is not release. External effects are not rolled back. A new plugin generation requires normal package trust, exact provider/ABI/cache identity, and authority admission; no compatibility alias silently widens lifetime or authority.

**M-TIER-004.** A restricted host either executes verified JPM with the same semantics or fails closed before activation. It cannot skip bounds checks, change NaN/atomic behavior, replace a denied service with ambient access, or execute a forbidden native cache.

### F.5 Resource accounting, termination, and dynamic admission

Worker and broker account guest/private/shared memory, stacks, tables, aliases, imports in flight, callbacks, code cache, semantic fuel, CPU, wall deadline, IO, leases, and child tasks. Reservations precede operations that can allocate. Cooperative cancellation takes effect at explicit check/wait/call-return/thread boundaries. A noncooperative helper runs in a separately killable boundary or the host refuses the advertised bound.

No `eval` exists. A package/JPH load request is treated as untrusted new content and re-enters trust, dependency, JPH negotiation, parser, verifier, profile, semantic-bundle, cache, and generation checks. Existing active frames keep their generation; update does not mutate function tables in place.

### F.6 Required parity and proof vectors

Conformance covers canonical encoding, all instruction/FP/conversion/SIMD/trap boundaries, packed SSA/debug point-in-time checks, memory/table/segment limits, SC atomics and wait races, host snapshot/lease/commit, stale/closed/forged references, JPH import/link/version failures, semantic bundle/cache invalidation, fuel exhaustion points, deopt/maps, browser/restricted denials, managed Prelude mappings, plugin/helper termination, and package generation. These are proposed proof obligations, not executed evidence.


<a id="host-abi"></a>
## Host ABI: JPH/1

### H-ABI-001 — One canonical boundary

JPH/1 is the language-neutral, typed boundary for every verified JPM/1 instance. The reference interpreter, JIT, AOT/cache, browser adapter, embedded adapter, and native broker adapter consume the same type IDs, resource identities, error categories, ownership transitions, effect states, and cancellation rules. A transport may be local IPC, a browser message channel, an in-process broker call, or an embedded queue, but transport choice does not alter the wire meaning.

JPH is downstream of checked Source/AST -> typed TIR -> canonical MIR -> adapters. It is not source ABI, public MIR, or a second authority language. Shared Prelude/CoreLib remains the owner of exact `Int`, `Fraction`, checked/floor/mod/remainder/shift operations, and current `Atomic<T>` meaning. JPM `i64` does not narrow source `Int`. No host pointer, native address, managed object identity, or borrowed guest pointer crosses this boundary.

### H-ABI-002 — Authenticated bootstrap, envelope, and frame kinds

A broker creates a session before it accepts a JPH frame. Bootstrap is out of band from guest payloads and has one state machine:

```
TransportSeen -> ProofChecked -> SessionIssued -> ChannelIssued -> Active
TransportSeen -> Refused
Active -> Closing -> Closed
```

The launch path supplies a `LaunchContext` to the broker, not to the guest:

```
LaunchContext {
  transport_binding: Digest256,       # authenticated endpoint/handle identity
  principal_id: Digest256,            # broker-derived, never guest asserted
  instance_generation: Digest256,
  isolation_domain_id: Digest256,
  authority_digest: Digest256,        # SHA-256 of canonical typed authority
  revocation_epoch: u64,
  proof_kind: u8,
  proof: AuthProof/1             # broker-only bounded proof
}
```

`proof_kind` values are `0=linux_local_ipc`, `1=macos_local_ipc`, `2=windows_local_ipc`, `3=browser_origin_worker`, `4=embedded_provisioned_image`, `5=explicit_external_attestor`. The broker verifies the exact `AuthProof/1` kind/material schema: local IPC binds the OS-authenticated peer credential and the exact transport handle/endpoint; browser binds the browser-mediated tuple origin, worker identity, and browser-issued channel token; embedded binds the signed image/content digest and provisioned device identity; an external attestor binds a pinned attestation identity and expiry. A same-user process name, URL text, PID text, or guest-supplied digest is not proof. A profile with no proof mechanism refuses activation.

The broker sends a challenge containing `broker_epoch:u64`, `broker_nonce:[u8;32]`, `session_nonce:[u8;32]`, and `challenge_digest:Digest256`, where the digest is SHA-256 over the canonical launch context, proof result, broker epoch, and both nonces. The launcher returns the proof-bound response; the broker verifies it against the transport metadata and creates a non-reused `session_id:[u8;16]` and `channel_id:[u8;16]`. The accepted session tuple is exactly:

```
(session_id, channel_id, transport_binding, principal_id,
 instance_generation, isolation_domain_id, authority_digest, revocation_epoch,
 broker_epoch)
```

Every frame is associated with that tuple by authenticated transport metadata or a session-local channel object. Guest fields cannot select any member of the tuple. A helper, callback route, attachment, suspension resume, and completion must carry the same tuple and is refused on any mismatch. A broker restart increments `broker_epoch`; all old sessions, callbacks, helper channels, and attachment leases become `stale` and cannot reattach. Reattachment requires a new proof and a new session; an old completion is retained only in the old receipt.

Every JPH frame has this exactly packed 64-byte little-endian header. The parser uses byte loads and never casts it to a native struct; `call_id` at offset 44 is intentionally unaligned.

| offset | size | field | rule |
|---:|---:|---|---|
| 0 | 4 | `magic` | `4A 50 48 00` (`JPH` + NUL) |
| 4 | 1 | `major` | `1` |
| 5 | 1 | `minor` | `0` initially; negotiated compatible minor only |
| 6 | 2 | `flags` | bits 0..2 outcome, bits 4..7 frame kind; all other bits zero |
| 8 | 32 | `interface_digest` | SHA-256 canonical interface descriptor |
| 40 | 4 | `function_id` | little-endian interface-local number |
| 44 | 8 | `call_id` | channel-scoped, non-reused `u64`; zero reserved |
| 52 | 4 | `payload_bytes` | exact payload byte count |
| 56 | 4 | `attachment_count` | exact call-local entry count |
| 60 | 4 | `reserved` | zero |

Outcome values are `0=accepted/control`, `1=ok`, `2=error`, `3=cancelled`, `4=interrupted`, `5=revoked`, `6=closed`; values 7 are reserved and reject. Frame kinds are `0=request`, `1=reply`, `2=callback_event`, `3=callback_register`, `4=cancel`, `5=ack`, `6=completion`, and `7=capability`; 8..15 reject. Legal combinations are:

| kind | legal outcome | payload |
|---|---|---|
| request | `0` only | typed parameters |
| reply | `0..6` | `AcceptedReply` for 0, or the exact declared `Result<T,Error>` value for 1..6 |
| callback_event | `0` only | `CallbackPrefix` + declared callback payload |
| callback_register | `0` only | callback descriptor |
| cancel | `0` only | `CancelPayload` |
| ack | `0` only | `AckPayload` |
| completion | `1..6` only | exact declared terminal `Result<T,Error>` value; header outcome must agree with its discriminant/error code |
| capability | `0` only | capability record |

`AcceptedReply` is exactly `{ call_id:u64 }` and has no function result. It is nonterminal. Every terminal reply/completion carries the function descriptor's exact declared `Result<T,Error>` wire value, including an `ok` case or an `error` case. The header outcome is a transport summary: `ok` requires variant case 0; `error`, `cancelled`, `interrupted`, `revoked`, and `closed` require variant case 1 and the corresponding stable Error domain/code. A mismatch between header outcome and Result discriminant/category is `protocol_error`. Callback delivery uses its declared callback `Result<T,Error>` type under the same rule. A synchronous function never emits `accepted` or a completion. A completion never emits `accepted`. Control frames never carry a result payload. `CancelPayload` is `{ target_call_id:u64, reason:u8, reserved:u8[7] }`, with reason `0=caller`, `1=deadline`, `2=revocation`, `3=owner_close`; `AckPayload` is `{ registration_id:u64, event_sequence:u64, callback_call_id:u64, reserved:u32 }`.

Call IDs increase from 1 and event sequences from 1. Neither wraps. The channel closes before `u64::MAX`; an ID remains reserved while its call, callback, completion, acknowledgement, or receipt is live. `payload_bytes` and `attachment_count` are checked before allocation. A frame is at most 16 MiB in the beginner profile and never over 64 MiB in JPH/1. Profile limits may be lower and are checked before activation for mandatory maxima.

### H-ABI-003 — Canonical type, resource, interface, and result grammar

Under H-ABI-005-CANONICAL-001, every function row's `result` TypeId MUST be kind 06 `result`, with exactly two children `[success_type_id, canonical_error_type_id]`. The canonical Error TypeId is one fixed builtin kind-08 descriptor in the interface registry; a function may not choose a second error row. Callback function rows obey the same rule. The `result` child may be `unit`, scalar, aggregate, resource, or list, but the outer Result is mandatory. Link rejects a bare scalar, bare Error, or mismatched error child as `bad_host_projection`.
The primitive JPH value scalars are fixed and never use native layout:

| type | size | alignment | encoding |
|---|---:|---:|---|
| `unit` | 0 | 1 | no bytes |
| `bool` | 1 | 1 | exactly `00` or `01` |
| `i8/u8` | 1 | 1 | bit pattern |
| `i16/u16` | 2 | 2 | little-endian |
| `i32/u32/f32` | 4 | 4 | little-endian / IEEE bits |
| `i64/u64/f64` | 8 | 8 | little-endian / IEEE bits |
| `v128` | 16 | 16 | 16 bytes; lane meaning is operation-defined |
| `TypeId/Digest256` | 32 | 1 | exactly 32 bytes |

There is no `usize`, pointer, native enum, C `long`, or host boolean. Enumerations are descriptor-validated scalar dictionaries or named masks; unknown values and reserved bits reject.

`callback_event` is an invocation payload, not a terminal Result. A callback completion is a distinct terminal grammar and correlation tuple. Ordinary terminal replies/completions remain exactly the declared Result. This rule applies identically to interpreter, JIT, AOT, browser, embedded, and broker adapters.


The interface registry uses this grammar in place of provider-specific resource/interface/function records. Providers publish complete rows and one exact interface digest; they may refuse a family but may not omit a row, infer an ID/default, or select descriptor values per provider. Interface/resource/adapter metadata uses its explicitly named `JPK-C14N/1` record preimage. Ordinary parameter/result values use H-ABI-004.

### H-ABI-005-CANONICAL-001 — explicit descriptor bytes and value grammar

```
TypeDescriptor/1 bytes {
  kind:u8@0
  flags:u8@1
  align_log2:u8@2
  reserved:u8@3 = 0
  fixed_size:u32le@4
  max_size:u32le@8
  child_count:u32le@12
  child_type_ids[child_count]:Digest256@16
  kind_payload:KindPayload
}
```

`TypeDescriptor/1` is an explicitly packed fixed-field-order byte string. It is not a JPK wrapper and does not use ordinary H-ABI record padding. `TypeId = H("JPH-TYPE-ROW/1", exact_type_descriptor_bytes)`; descriptor bounds, reserved bytes, child graph, and kind payload are checked before hashing.

Kind payloads are closed: scalar has `scalar_tag:u8,reserved[3]=0`; resource has32 raw resource-TypeId bytes; list has `max_count:u32le`; record, variant, result, text, bytes, Error and MemorySlice have no payload. Scalar/resource/text/bytes/builtins have no children; list has one; variant has at least one; result has exactly two; record children are positional. Only resource flag bit0 nullable is legal; other flags are zero. Inline type graphs must be acyclic. Text/bytes maximum includes the4-byte length prefix; list maximum/count include the H-ABI framing and declared maximum elements. Fixed aggregates derive offsets/sizes from H-ABI-004, never native struct layout; bounds that overflow u32 reject.

For H-ABI-004 variants, `base_align=max(4,alignment of every case)`. Align the selected payload after the8-byte prefix, then zero-pad the complete value to base_align; payload_bytes includes all bytes after that prefix. A unit has zero semantic payload, but may still require container padding (for example Opt<v128> none has8 padding bytes). This replaces the ambiguous “unit cases have zero payload bytes” shortcut. Records retain their4-byte body_bytes prefix. Callback prefix40 is followed by padding to the callback parameter/result descriptor's base alignment; callback payload_bytes includes that padding and complete argument value. Header payload length covers the whole frame payload.
Ordinary H-ABI-004 values use checked `align_up(x,a)=(x+a-1)&~(a-1)` for power-of-two `a`; padding is zero and part of the exact value length. A record begins with packed `body_bytes:u32` at offset 0, uses `base_align=max(4,field alignments)`, starts fields at `align_up(4,base_align)`, and sets `body_bytes` to the final cursor minus 4, including leading, inter-field, and tail padding. A variant begins with packed `case_index:u32` at offset 0 and `payload_bytes:u32` at offset 4, starts the selected payload at `align_up(8,base_align)`, and includes all payload padding and tail padding in `payload_bytes`; a unit case has zero semantic payload but may have container padding. A fixed list begins with packed `count:u32` and `body_bytes:u32`, aligns elements after the 8-byte prefix, and uses checked aligned stride; variable elements carry exact element lengths. `Bytes` and `Text` carry their exact u32 length prefix and no implicit terminator. Fixed aggregate size, repeated stride, nested bounds, and all additions are checked before allocation or effect.

Kind values are fixed: `00 scalar` (tags `00 unit,01 bool,02 i8,03 u8,04 i16,05 u16,06 i32,07 u32,08 f32,09 i64,0A u64,0B f64,0C v128,0D TypeId,0E Digest256`), `01 record`, `02 variant`, `03 list`, `04 text`, `05 bytes`, `06 result`, `07 resource`, `08 Error`, and `09 MemorySlice`. Kinds 08 and 09 have zero flags, zero children, empty kind payload, and `align_log2=3`.

`kind=06 result` has exactly two children `[success_type_id, canonical_error_type_id]`; its size/alignment/max derive from those children. Ordinary H-ABI-004 `Opt<T>` is variant `[unit,T]` (case 0 none, case 1 value). A JPK metadata Optional is a separate explicitly named presence encoding and is never described as that ordinary variant. Generic nullable flags apply only to resource descriptors.

`H-ABI-RESOURCE-REF-001` defines the missing resource value bytes once: a `ResourceRef<T>` value is exactly `u32le`, alignment 4 (`align_log2=2`), `0xFFFFFFFF` is null, and nonnull indexes `0..0xFFFFFFFE` are legal only after bounds checking. The kind-07 descriptor payload remains the nominal `resource_type_id`; its TypeDescriptor is `fixed_size=max_size=4` with nullable flag bit 0. On a JPH frame the value selects an authenticated `AttachmentEntry`; in guest aggregate bytes it selects an already installed value in the mapped private typed table. These namespaces are distinct and the adapter translates them. An integer is not a resource and never creates authority.

The builtin `Error` descriptor is `kind=08, flags=0, align_log2=3, fixed_size=0, max_size=4128, child_count=0, kind_payload=empty`. Its value is a packed 28-byte prefix at `domain:u32@0, code:u32@4, flags:u32@8, reserved:u32=0@12, retry_after_ns:u64@16, detail_bytes:u32@24`, followed by at most 4096 RFC-3629 UTF-8 bytes at offset 28 and zero tail padding to 8-byte alignment. Reply budget may lower optional detail only after retaining the full mandatory value; a Q4K reply is smaller than 4128 and therefore exposes a shorter detail, never a missing prefix. Truncation is UTF-8 safe; domain, code, and effect flags are never dropped. Error detail is non-authoritative.
The canonical error categories are:

| domain | code | category | classification |
|---:|---:|---|---|
| 1 | 1 | `invalid_argument` | valid frame/type, but value, range, state, or semantic argument is invalid |
| 1 | 2 | `protocol_error` | frame, descriptor, version, attachment, length, padding, reserved field, or canonical encoding is invalid |
| 2 | 1 | `denied` | typed authority, user permission, origin, entitlement, deputy, or policy denies |
| 2 | 2 | `unsupported` | selected implementation/profile cannot provide the operation or declared bound |
| 2 | 3 | `unavailable` | optional facility is absent or temporarily unavailable |
| 3 | 1 | `not_found` | scoped object does not exist |
| 3 | 2 | `already_exists` | create/replace precondition finds an existing object |
| 3 | 3 | `conflict` | sharing, lock, compare, memory-generation, or commit condition conflicts |
| 3 | 4 | `stale` | handle, candidate, bookmark, session, generation, or view is no longer current |
| 4 | 1 | `exhausted` | declared memory, time, byte, queue, handle, task, or device reservation cannot be made |
| 4 | 2 | `timeout` | declared caller deadline expires before effect linearization |
| 4 | 3 | `busy` | operation would wait and caller selected no-wait, or required queue/reentrancy state is busy |
| 5 | 1 | `cancelled` | cancellation wins before effect linearization |
| 5 | 2 | `interrupted` | host shutdown, device interruption, or external interruption occurs |
| 5 | 3 | `revoked` | authority/resource is revoked before completion |
| 6 | 1 | `closed` | resource/channel is closed before admission |
| 6 | 2 | `wrong_affinity` | required worker/main/render/device queue is not the declared live queue |
| 7 | 1 | `device_lost` | device reset/loss invalidates the operation; charges remain until fence/proof |
| 7 | 2 | `validation_failed` | pinned driver/shader/tensor/protocol validator rejects the value |
| 8 | 1 | `host_failure` | host failure has no more specific portable category |

Classification precedence is deterministic: malformed frame/descriptor/attachment or noncanonical bytes are `protocol_error`; session/channel/transport or generation mismatches are `denied`, `stale`, `closed`, or `revoked` by their exact state; checked value failures are `invalid_argument`; scope failures are `denied`; profile absence is `unsupported`/`unavailable`; failed ancestor reservation is `exhausted`; affinity/reentrancy is `wrong_affinity`/`busy`; cancellation/deadline before linearization is `cancelled`/`timeout`; external interruption, revocation, closure, device loss, validation, and host failure use their corresponding category. Earlier phases always win, and a host typed result is never rewritten as a JPM verifier category.

Error flags are normative: bits 0..1 are effect phase (`00=not_started`, `01=accepted`, `10=completed`, `11=unknown`); bit 2 is `not_retractable`; bit 3 is `awaiting_quiescence`; bit 4 is `partial`; bit 5 is `retry_safe`; bit 6 is `receipt_present`; bits 7..31 are reserved zero. Detail text is non-authoritative.

The builtin `MemorySlice` descriptor is `kind=09, flags=0, align_log2=3, fixed_size=max_size=32, child_count=0, kind_payload=empty`, with exact fields `memory_id:u32,memory_generation:u32,offset:u64,length:u64,mode:u8,reserved[7]=0`. Its JPM mapping is the fixed `[i32,i64,i64]` logical group.

```
InterfaceRegistry/1 {
  family:Text<=64
  major:u8 = 1
  minor:u8 = 0
  interface_flags:u16le = 0
  canonical_error_type_id:Digest256
  right_key_count:u32le
  right_keys[...]:RightKey sorted by key_id
  resource_count:u32le
  resources[...]:ResourceDescriptor sorted by resource_type_id
  type_count:u32le
  types[...]:TypeDescriptor sorted by TypeId
  function_count:u32le
  functions[...]:FunctionDescriptor sorted by function_id
}
RightKey {
  key_id:u32le
  name:Text<=128
}
ResourceDescriptor {
  resource_type_id:Digest256 derived from row with ID omitted
  family:Text<=64
  name:Text<=128
  version:u16le = 1
  flags:u16le = 0
}
FunctionDescriptor {
  function_id:u32le
  name:Text<=128
  introduced_minor:u16le = 0
  flags:u16le = 0
  parameter_type_ids:List<Digest256><=128 ordered
  result_type_id:Digest256             # MUST resolve kind=06 result
  required_right_key_id:u32le
  call_mode:u8
  affinity:u8
  reentrancy:u8
  cancellation:u8
  max_request:u32le
  max_reply:u32le
  cost_class:u16le
  effect_point:u8
  terminal_rule:u8
  reserved:u16le = 0
}
```

Metadata preimages are exact: `InterfaceRegistry/1=Record("JPH-INTERFACE",1,listed_fields)`; each registry type entry is `Bytes(exact_TypeDescriptor_bytes)` sorted by its computed TypeId. `ResourceBody/1=Record("JPH-RESOURCE-ROW",1,[family,name,version,flags])`; its resource_type_id is `H("JPH-RESOURCE-ROW/1",body)`. A serialized ResourceDescriptor is `Record("ResourceDescriptor",1,[resource_type_id,family,name,version,flags])`. FunctionDescriptor is `Record("JPH-FUNCTION",1,all17 listed fields including reserved)`. SignatureBody is `Record("JPH-SIGNATURE",1,the first16 FunctionDescriptor fields, excluding only reserved)` and `signature_digest=H("JPH-SIGNATURE/1",SignatureBody)`. `interface_digest=H("JPH-INTERFACE/1",InterfaceRegistry_bytes)`; no enclosing digest occurs in its preimage. Nested metadata tags/counts follow F-ADAPTER-001. Async behavior is call_mode; success is the Result's first child, not a second terminal-type field.

The RightKey table is ID/name only. Operation-required scope derives from the authenticated resource binding plus typed operation arguments under each family's existing scope law; `fs.read`, for example, receives scope through its checked File binding and does not require a redundant scope parameter. A raw RightKey ID/name/digest never grants authority. The fixed table includes `115 Resource.Control` for generated lifecycle control checks and `414 Exec.Shell`:

```
100 FS.Discover       101 FS.Volume        102 FS.Read          103 FS.Open
104 FS.List           105 FS.Write         106 FS.Flush         107 FS.Rename
108 FS.Delete         109 FS.Lock          110 FS.Watch         111 IO.Read
112 IO.Write          113 IO.Seek          114 IO.Close          115 Resource.Control
200 Net.Resolve        201 Net.Connect      202 Net.Listen        203 Net.Accept
204 Net.Receive        205 Net.Send        206 Net.TLS           207 Net.HTTP
208 Net.Proxy
300 Time.Read          301 Time.Sleep       302 Random.Secure     303 Random.Deterministic
400 IO.Stdio           401 Env.Read        402 Secret.Read       403 Exec.Spawn
404 Exec.Read          405 Exec.Wait       406 Exec.Signal       407 Exec.Kill
408 Task.Spawn         409 Task.Join       410 Task.Cancel       411 Task.Detach
412 Task.Yield         413 Exec.Resolve    414 Exec.Shell
500 GUI.Read            501 GUI.Window      502 GUI.Present       503 GUI.Input
504 Input.Read         505 Text.Read        506 Text.Shape        507 Clipboard.Read
508 Clipboard.Write     509 Accessibility.Read 510 Accessibility.Act
600 Audio.Enumerate     601 Audio.Open      602 Audio.Play        603 Audio.Record
604 Audio.Read          605 Audio.Queue
700 GPU.Discover        701 GPU.Open       702 GPU.Memory         703 GPU.Transfer
704 GPU.Readback        705 GPU.Shader     706 GPU.Submit         707 GPU.Wait
708 GPU.Render          709 ML.Tensor      710 ML.Transfer       711 ML.Execute
712 ML.Readback
800 Device.Discover     801 Device.Claim   802 Device.Control     803 Device.MMIO.Read
804 Device.MMIO.Write   805 Device.DMA     806 Embedded.Watchdog 807 Embedded.Power
808 Device.Status
900 FFI.Trusted         901 FFI.Helper     902 FFI.Call           903 FFI.Cancel
904 FFI.Read            905 FFI.Close      906 Callback.Register 907 Callback.Close
```

### H-ABI-009 generic close/query/await contract

The generic API remains exactly:

```
resource.close<T>(ResourceRef<T>,CloseMode) -> Result<CloseResult,Error>
resource.query<T>(ResourceRef<T>) -> Result<ResourceStatus,Error>
resource.await_quiescence<T>(ResourceRef<T>,u64 timeout_ns) -> Result<QuiescenceProof,Error>
CloseMode: 0 close_alias, 1 close_object_if_owner, 2 cancel_then_close
CloseResult { state:u8, alias_closed:Bool, object_state:u8,
              admitted_users:u32, outcome:Opt<OperationOutcomeRef> }
ResourceStatus {
  resource_type_id:Digest256, object_id:Digest256, object_generation:u64,
  state:u8, owner_channel:Digest256, revocation_epoch:u64,
  admitted_users:u32, charged_bytes:u64, charged_work:u64
}
```

`CloseResult` retains its canonical fields/states `0=already_closed,1=closing,2=quiescent_closed,3=revoked`; generic cleanup/query uses the known binding identity and never targets a replacement generation or demands newly granted authority after revocation. `await_quiescence` is an observer and does not count as work blocking its own proof. A real empty closed binding returns the scoped empty proof (`scope_kind=1` binding, zero member rows and matching residual/charge digests), never `unsupported`; physical object reclamation still waits for other bindings/uses.

`Resource.Control` is a control check over the authenticated caller's existing binding, not a new ambient grant or resource creator. Close-alias, status and scoped await remain permitted on that exact retained binding/tombstone after revocation. Object close additionally requires that binding's existing ownership proof; cancel_then_close applies only to its scoped work. A stale generation can report its own closed/stale state but cannot inspect, cancel or close the replacement generation.

For every resource ordinal, generated descriptor rows retain complete parameter/result and control rules:

```
resource.close.<name>:
  params=[ResourceRef<name>,CloseMode], result=Result<CloseResult,Error>,
  required_right=115 Resource.Control, call_mode=00, affinity=04 broker,
  reentrancy=00, cancellation=01, max_request=Q4K, max_reply=Q16K,
  cost_class=1, effect_point=04 commit, terminal_rule=00 ordinary_result
resource.query.<name>:
  params=[ResourceRef<name>], result=Result<ResourceStatus,Error>,
  required_right=115 Resource.Control, call_mode=00, affinity=04 broker,
  reentrancy=00, cancellation=00, max_request=Q4K, max_reply=Q16K,
  cost_class=1, effect_point=00 none, terminal_rule=00 ordinary_result
resource.await_quiescence.<name>:
  params=[ResourceRef<name>,u64 timeout_ns], result=Result<QuiescenceProof,Error>,
  required_right=115 Resource.Control, call_mode=01, affinity=04 broker,
  reentrancy=00, cancellation=01, max_request=Q4K, max_reply=Q16K,
  cost_class=2, effect_point=02 queue_admission, terminal_rule=01 scoped_proof
```

The generated IDs remain `0x1000 + 3*n`, `0x1001 + 3*n`, and `0x1002 + 3*n` for the fixed resource ordinal table. The 63 nominal resource ordinals therefore produce exactly 189 generated lifecycle IDs. No second authority or lifecycle registry is maintained; concrete bindings own authority, active work, and cleanup state.

```
00 VolumeRoot          01 VolumeCandidate     02 Volume       03 Directory
04 File                05 DirCursor           06 RemoveCursor 07 Lock
08 Watch               09 Stream              10 ResolvedEndpoint 11 Tcp
12 Listener            13 Udp                14 Tls          15 Proxy
16 HttpClient          17 HttpResponse        18 BodyStream   19 OpaqueBody
20 Process             21 Task                22 DetachedTask 23 IsolationDomain
24 ExecutableCandidate 25 Executable          26 Window       27 EventStream
28 InputDeviceCandidate 29 InputStream        30 SealedBuffer 31 AudioDeviceCandidate
32 AudioStream         33 CallbackQueue       34 AdapterCandidate 35 GpuDevice
36 GpuBuffer           37 GpuShader           38 GpuPipeline  39 RenderPass
40 GpuFence            41 Tensor              42 ModelArtifact 43 Model
44 DeviceCandidate    45 Device              46 ExclusiveBuffer 47 DmaFence
48 Secret             49 NativeModule        50 NativeHelper 51 CallerContext
52 CallbackRegistration 53 AuthorityGrant    54 Texture      55 TextureView
56 Sampler            57 BindGroupLayout     58 BindGroup    59 PipelineLayout
60 CommandEncoder     61 CommandList         62 ComputePass
```

### H-ABI-004 — AdapterContract/1 and JPM projection

The host-owned adapter contract is this record. The record is outside JPM bytes but is mandatory at link. `adapter_contract_digest` is independently recomputable from the immutable body; it is not an opaque provider assertion.

### AdapterContractBody/1 canonical bytes

The portable body is one explicitly named `JPK-C14N/1` record preimage. It contains no live session, authority, object, revocation, scope, or binding values. Arrays marked `sorted` are sorted by complete encoded item bytes; parameter/result groups preserve source ordinal order and use separate fixed index spaces.

```
AdapterContractBody/1 {
  format:u8 = 01
  jpm_function_id:u32le
  jpm_signature_digest:Digest256
  jph_interface_digest:Digest256
  jph_function_id:u32le
  adapter_version:u8 = 01
  adapter_mode:u8                 # 00 sync, 01 async, 02 callback_register, 03 callback_delivery
  parameter_group_count:u32le
  parameter_groups[...]:SlotMappingGroup ordered by source_ordinal
  result_group_count:u32le
  result_groups[...]:SlotMappingGroup ordered by source_ordinal
  resource_table_count:u32le
  resource_tables[...]:ResourceTableMap sorted by resource_type_id
  aggregate_value_format:u8 = 01 # H-ABI-004 bytes in checked guest memory
  aggregate_max_bytes:u32le
  callback:Opt<CallbackProjection>       # JPK metadata Optional
  continuation:ContinuationProjection
  error:ErrorProjection
  output_arena:Opt<OutputArenaMap>       # JPK metadata Optional
  bounds:AdapterBounds
  reserved:u32le = 0
}
```

`AdapterContractBody/1` is metadata and uses only the named `JPK-C14N/1` record preimage; it is not an H-ABI ordinary value. `adapter_contract_digest = H("JPH-ADAPTER-BODY/1", exact_jpk_c14n_body_bytes)` over every field above except the derived digest. `jpm_signature_digest` is the exact local JPM function/signature identity, and `(jpm_function_id,jpm_signature_digest,jph_interface_digest,jph_function_id,adapter_version,adapter_mode,adapter_contract_digest)` is the complete noncyclic link tuple. There is no import-site binding digest field. A metadata `Opt<X>` means the JPK-C14N presence encoding; an ordinary guest `Opt<X>` always means the H-ABI-004 variant `[unit,X]`.

The adapter body is `Record("JPH-ADAPTER-BODY",1,listed_fields)`. Nested metadata record tags are their exact declared names without `/1`, version1, fields in declaration order. A separately listed `*_count` and following array emit the count once and then the raw ordered items; `List<T>` emits its own count. `Digest256` is exactly32 raw bytes, distinct from JPK's algorithm-tagged `Digest`. `jpm_signature_digest=H("JPM-SIGNATURE/1",signature_row_bytes)` where the row is the exact B.2 parameter count/ValTypes/result count/ValTypes, without its table index. Reference indexes are checked against that module's signature/resource tables at link; no enclosing JPM artifact hash enters this preimage.

`JPM ValType` bytes are reused verbatim in every slot map: `01=i32`, `02=i64`, `03=f32`, `04=f64`, `05=v128`, `06=funcref + sigidx:uLEB32`, and `07=resref + resourceidx:uLEB32`; no duplicate adapter slot enum exists.

```
SlotMappingGroup {
  slot_space:u8             # 00 parameter index space, 01 result index space
  source_ordinal:u32le
  logical_kind:u8            # 00 scalar, 01 MemorySlice, 02 aggregate,
                             # 03 result_case, 04 indirect_output, 05 resource
  first_slot:u32le
  slot_count:u32le
  slot_type_bytes[slot_count]:Bytes<=8
  direction:u8               # 00 guest_to_host, 01 host_to_guest, 02 inout
  jph_type_id:Digest256
  memory_mode:u8             # 00 none, 01 copy_read, 02 exclusive_output,
                             # 03 sealed_read, 04 exclusive_transfer
  reserved:u16le = 0
}
ResourceTableMap {
  resource_type_id:Digest256
  private_table_id:u32le
  resref_type_bytes:Bytes<=8 # exact JPM 07 resref + resourceidx bytes
  nullable:Bool = 01
  capacity_reserved:u32le
  binding_scope:u8           # 00 call, 01 instance, 02 callback, 03 continuation
  reserved:u16le = 0
}
CallbackProjection {
  callback_interface_digest:Digest256
  callback_function_id:u32le
  callback_signature_digest:Digest256
  callback_parameter_type_id:Digest256
  callback_result_type_id:Digest256
  callback_registration_resource_type_id:Digest256
  prefix_format:u8 = 01
  correlation:u8 = 00 originating_call_id_and_callback_call_id
  event_terminal_split:u8 = 00 event_nonterminal_completion_terminal
  direction:u8 = 00 host_to_guest_event_guest_to_host_completion
  ack_required:Bool
}
ContinuationProjection {
  mode:u8                    # 00 none, 01 suspend_task_resume_exact_call
  accepted_payload:u8        # 00 adapter-internal AcceptedReply only for mode=01
  resume_target:u8           # 00 same_task_same_instance_generation
  result_slot_policy:u8      # 00 write_declared_machine_result_slots
  cancellation:u8            # 00 terminal_cancelled_result, 01 instance_termination_no_resume
  retention_rule:u8          # 00 retain work-backed state/charges until actual quiescence/fence
  orphan_completion:u8       # 00 retain terminal completion in canonical receipt; no release policy
}
ErrorProjection {
  result_type_id:Digest256
  canonical_error_type_id:Digest256
  result_case_ok:u32le = 0
  result_case_error:u32le = 1
  header_outcome_map:u8       # 00 exact H-ABI-002 outcome mapping
  mismatch_category:u8        # 00 protocol_error
}
OutputArenaMap {
  parameter_group_ordinal:u32le
  input_first_slot:u32le
  input_slot_count:u32le = 03 # memory_index, offset, length; INPUT parameter space
  case_result_slot:u32le
  success_result_first_slot:u32le
  success_result_slot_count:u32le
  error_length_result_slot:u32le
  error_offset_result_slot:u32le
  memory_mode:u8 = 02         # exclusive_output
  reserved:u16le = 0
}
AdapterBounds {
  max_request:u32le
  max_reply:u32le
  max_attachments:u32le
  max_attachment_bytes:u64le
  max_callback_payload:u32le
  max_codec_depth:u32le
}
```

`SlotMappingGroup` is a logical mapping group, never a new JPM machine type. `logical_kind=01` MUST have `slot_count=3` and exact `slot_type_bytes=[01,02,02]`, the fixed `(i32 memory_index,i64 offset,i64 length)` MemorySlice triple. Parameter groups partition the fixed parameter index space exactly once; result groups partition the separate result index space exactly once. The output-arena triple is an input parameter group; case/success/error slots are result groups. Link checks first-slot, count, exact ValType bytes, widths, arity, and direction with no gaps, overlaps, hidden provider slots, or unaccounted caller arena.

Adapter memory_mode0 means no memory mapping; values1..4 map to JPH MemorySlice.mode0..3 respectively (copy_read, exclusive_output, sealed_read, exclusive_transfer). For the JPM triple, the adapter resolves memory_index in the live instance, supplies its checked generation, and never trusts a guest-supplied generation or native address. An error arena must start at an8-byte-aligned checked offset; error_offset returns that same arena start, and error_length includes complete Error padding.

The aggregate codec is exactly H-ABI-004 value bytes in checked guest memory. At call/activation, the adapter walks the registered H-ABI descriptor recursively: record children in ordinal order, list elements in ascending index, and only the selected variant case. This total traversal handles arbitrary bounded nesting and emits a dense attachment index on first encounter. A repeated resource alias reuses the same dense index when its exact binding identity `(binding_id,session_id,channel_id,principal_id,instance_generation,isolation_domain_id,resource_type_id,object_generation,revocation_epoch,effective_scope_digest,required_right_id)` matches; different identity receives a new index. No static field path, per-instance attachment array, implicit index, or dynamic-list exception exists.

Each nominal resource type maps only through its declared `ResourceTableMap`. In aggregate bytes, a ResourceRef is an H-ABI u32 table value; on a JPH frame the corresponding dense index selects an authenticated `AttachmentEntry`; in guest aggregate memory the u32 selects an already installed value in the mapped private typed table. These namespaces are distinct and the adapter translates them. An integer is never a resource and cannot create authority. Runtime admission checks the existing binding against the current authenticated instance, channel, session, principal, isolation domain, revocation epoch, object generation, operation-required right, and effective scope; these values never enter the portable adapter digest.

Before effect linearization, the broker reserves every referenced table capacity and the caller-provided exclusive output arena. Returned attachments are validated and atomically installed as the complete selected result/ref set in capacity-reserved mapped tables; output H-ABI values contain their u32 table indexes. A failed validation or capacity check installs nothing. The adapter performs no implicit retain, move, close, or release; declared host operation and attachment semantics own those transitions. Terminal reply is not quiescence: work-backed state, storage, and charges remain until their applicable terminal/fence/quiescence proof.

The `Result<T,Error>` branch is explicit in JPM slots: `case_result_slot:i32` is followed by exact success slots, or `error_length_result_slot:i32` and `error_offset_result_slot:i64` into the checked exclusive output arena. Admission reserves the full mandatory Error value (at least the 32-byte padded prefix/empty-detail value) before any effect; inactive scalar slots are zero bits and inactive reference slots are null. Callback wrappers use the same case/arena/table rules with preallocated bounded arenas and typed-table capacity.

### H-ABI-004A — exact continuation rule

For `adapter_mode=01`, the imported JPM instruction is a suspension point. The adapter may first return its internal `AcceptedReply{call_id}` control, but that control is not a guest-visible result or a second result type. `SuspendedCall` below is internal mutable bookkeeping, not immutable content identity:

```
SuspendedCall {
  instance_generation:Digest256
  task_id:u64
  instruction_identity:{funcidx:u32,blockidx:u32,instruction_ordinal:u32}
  result_base:u32
  result_type_id:Digest256
  call_id:u64
  binding_generation:u64
  adapter_contract_digest:Digest256
  caller_context_digest:Opt<Digest256>
  argument_lease_digests:List<Digest256>
  budget_reservation_digest:Digest256
  retained_charge_digest:Digest256
  state:u8                 # 00 suspended, 01 cancel_requested, 02 terminal, 03 orphaned
}
```

Exactly one terminal `Result<T,Error>` with the descriptor's declared result TypeId validates call ID, descriptor, continuation, leases, context, budget, binding, and instance generation, then resumes the same task/generation and writes declared result slots. Other runnable tasks may run while this task is parked. Cancellation before effect yields the declared cancelled Result; cancellation after effect follows typed Result/effect flags. If the instance traps, closes, or is revoked, no guest resume occurs, and retained work-backed state/storage/charges remain until actual terminal, fence, or applicable quiescence proof. A late completion cannot resume discarded bookkeeping. No tier returns an ordinary SSA value at acceptance, blocks the instance, or enqueues an untyped future; synchronous descriptors never produce `AcceptedReply`.

### H-ABI-004B — bounded nested reentry and queue refusal

A callback into the parked origin task may execute only when the descriptor explicitly permits bounded nested reentry. While the original call remains parked, callback exclusively owns that task's private state; no concurrent callback/parent execution accesses it, no implicit shared capture exists, and dispatch is forbidden while a broker authority/resource/allocator lock is held. Non-reentrant/background callbacks use their declared bounded queue. Unavailable/overflowing dispatch returns exact typed `busy` or `exhausted`; parent termination returns exact typed `interrupted` and retains effect/charge state. A late callback is terminal receipt data only and cannot resume the task.

### H-ABI-004C — callback event versus terminal result

`callback_event` is nonterminal and legal only with outcome `0`; its payload is `CallbackPrefix` followed by callback parameters, never a Result. `CallbackPrefix` is exactly 40 bytes, align8:

```
CallbackPrefix {
  registration_id:u64
  event_sequence:u64
  originating_call_id:u64
  callback_call_id:u64       # equal to header call_id
  payload_bytes:u32
  reserved:u32 = 0
}
```

Direction is fixed by frame kind: event is host-to-guest invocation; completion is guest-to-host terminal callback Result. Registration is guest-to-host. Correlation is exact `(registration_id,event_sequence,originating_call_id,callback_call_id,interface_digest,function_id,queue_id,queue_epoch)`.

The handler may send transport `ack` but MUST then send exactly one terminal completion unless cancellation/revocation/interruption is recorded. `CallbackTerminalPrefix` is exactly 40 bytes, align8, followed by the exact declared callback Result:

```
CallbackTerminalPrefix {
  registration_id:u64
  event_sequence:u64
  originating_call_id:u64
  callback_call_id:u64
  reserved:u64 = 0
}
```

Ordinary completion payload is the exact Result only. Callback completion is the 40-byte prefix plus that Result. `ack` never completes a callback or carries a Result. Any correlation mismatch is `protocol_error`; callback state is `Registered -> Queued -> Delivering -> AwaitingCompletion -> Completed|Cancelled|Revoked|Interrupted -> Closed`, at most once.

### H-ABI-006 — Resources, attachments, and exact binding equality

A `ResourceRef<T>` is a private typed table entry. It is produced only by a verified import/result/transfer and cannot be manufactured from an integer, memory bytes, NaN payload, function reference, or attachment index. Call-local attachments use a packed 68-byte entry:

```
binding_id:u64@0
object_generation:u64@8
resource_type_id:Digest256@16
mode:u8@48                  # 0 borrowed, 1 sealed_read, 2 exclusive_lease, 3 move
transport_kind:u8@49        # 0 inline, 1 local_handle, 2 shared_memory
flags:u16@50=0
byte_length:u64@52
transport_ordinal:u32@60   # 0xffffffff for inline
reserved:u32@64=0
```

The table follows the payload exactly. Frame length is `64 + payload_bytes + 68*attachment_count`, plus only authenticated out-of-band handles in ordinal order. Attachment indices are bounds-checked and then checked against session, principal, channel, object generation, interface, rights, mode, and operation scope. A scalar or bytes value that resembles an index has no authority. Duplicate binding/generation pairs, unknown modes/kinds, non-inline entries without an authenticated handle, and nonzero reserved fields reject.

The broker binding state contains at least `(session_id, channel_id, authenticated transport binding, principal_id, instance_generation, isolation_domain_id, authority_digest, revocation_epoch, binding_id, object_id, object_generation, interface_digest, attenuated rights, state)`. Admission requires exact equality for every session/domain field, not merely a live channel and matching object generation:

```
principal_is_live && channel_is_live &&
session_id == request.session_id && channel_id == request.channel_id &&
binding.principal_id == channel.principal_id &&
binding.instance_generation == channel.instance_generation &&
binding.domain_id == channel.domain_id &&
binding.authority_digest == channel.authority_digest &&
binding.revocation_epoch == current_epoch &&
binding.object_generation == object.generation &&
binding.interface == requested_interface &&
covers(binding.rights, required_right) && argument_in_scope && reservation_succeeds && profile_enforces
```

The same predicate runs after every callback, suspension, helper completion, and lease return. A broker token is internal only; a guessed token is invalid outside its session and generation.


### H-ABI-007 — Computable ABI witness

The following fixed example is a complete descriptor/slot witness, not a runtime fixture. It uses `jph.example@1.0`, `function_id=1`, `name="buffer.read_sealed"`, `required_right_key_id=111 (IO.Read)`, `call_mode=00 sync`, `affinity=01 origin_worker`, `reentrancy=00 no_reentry`, `cancellation=03 before_effect`, `max_request=Q16K`, `max_reply=Q16K`, `cost_class=02`, `effect_point=03 external_acceptance`, and `terminal_rule=00 ordinary_result`. It reads/copies a checked source range from an existing sealed buffer and returns a newly sealed buffer; it never writes into a ResourceRef. The signature preimage is the exact JPK-C14N function fields above plus the TypeIds below, and the interface digest is the exact enclosing registry body.

For this witness `introduced_minor=0,flags=0,reserved=0`, parameter_type_ids are `[ResourceRef<SealedBuffer>,U64,U32,MemorySlice]`, and result_type_id is `Result<ReadReply,Error>`. ReadRequest is the derived input-record envelope for those four parameters, not a second nested request parameter. It is a complete function/type/slot witness; a complete interface also includes its canonical generated lifecycle/type closure and is not presented as a second byte vector here.

```
ReadRequest = record[
  source:ResourceRef<SealedBuffer>,
  offset:u64,
  length:u32,
  error_arena:MemorySlice
]
ReadReply = record[
  output:ResourceRef<SealedBuffer>,
  bytes_read:u32
]
Result<ReadReply,Error> = variant[ReadReply, Error]

JPM parameter index space (starts at 0):
  slot 0: nullable typed resref<SealedBuffer> source
  slot 1: i64 offset
  slot 2: i32 length
  slots 3..5: i32 arena_memory_index, i64 arena_offset, i64 arena_length
JPM result index space (starts at 0):
  slot 0: i32 result_case (0=success, 1=error)
  slot 1: nullable typed resref<SealedBuffer> output  # success only
  slot 2: i32 bytes_read                         # success only
  slot 3: i32 error_length                       # error only
  slot 4: i64 error_offset                       # error only, in input arena
```

The parameter mapping partitions `source -> [0]`, `offset -> [1]`, `length -> [2]`, and caller-owned exclusive `error_arena -> [3,4,5]` exactly once. The result mapping partitions `case -> [0]`, success -> `[1,2]`, and error -> `[3,4]` exactly once in the separate result space. Case 0 requires a nonnull output ResourceRef and zero error slots; case 1 requires a null output ResourceRef and zero success slots. All inactive scalar slots are zero and inactive typed reference slots are null. The input arena length must be at least 32 bytes: the 28-byte Error prefix plus zero tail padding to 8-byte alignment is the minimum legal Error value. When the error branch is relevant, the enclosing Result reservation is at least 40 bytes (its result framing plus the 32-byte minimum Error aggregate); a prefix of 28 bytes alone is never accepted.

The exact H-ABI-004 record layout includes its body_bytes prefix. ReadRequest: body_bytes:u32@0=60; zero pad4..7; source:u32@8; zero pad12..15; offset:u64@16; length:u32@24; zero pad28..31; error_arena:MemorySlice@32..63. Total64, alignment8. ReadReply: body_bytes:u32@0=8; output:u32@4; bytes_read:u32@8. Total12, alignment4. A successful Result containing that reply has prefix8 plus reply12 plus tail padding4, total24 at base alignment8; error Result minimum40 and maximum4136. These are H-ABI records, not eight-byte native field slots.
The resulting bounded wire sizes are `Error=32..4128` (28-byte mandatory prefix plus UTF-8 detail and zero 8-byte tail padding), successful `Result<ReadReply,Error>=24` for the fixed reply, and error `Result<ReadReply,Error>=40..4136`; no value field is widened into a fake 8-byte native slot.

The exact packed TypeDescriptor derivation is:

```
I32 = { kind=00, flags=00, align_log2=2, fixed_size=4, max_size=4,
        child_count=0, scalar_tag=06, scalar_reserved=00 00 00 }
U32 = { kind=00, flags=00, align_log2=2, fixed_size=4, max_size=4,
        child_count=0, scalar_tag=07, scalar_reserved=00 00 00 }
U64 = { kind=00, flags=00, align_log2=3, fixed_size=8, max_size=8,
        child_count=0, scalar_tag=0A, scalar_reserved=00 00 00 }
MemorySlice = { kind=09, flags=00, align_log2=3, fixed_size=32, max_size=32,
                child_count=0, kind_payload=empty }
SealedBuffer resource row =
  { family="jph.core", name="SealedBuffer", version=1, flags=0 }
ResourceRef<SealedBuffer> =
  { kind=07, flags=01 nullable, align_log2=2, fixed_size=4, max_size=4,
    child_count=0, kind_payload=resource_type_id(H(row)) }
ReadRequest =
  { kind=01 record, flags=00, align_log2=3, fixed_size=64,
    max_size=64, children=[ResourceRef<SealedBuffer>,U64,U32,MemorySlice] }
ReadReply =
  { kind=01 record, flags=00, align_log2=2, fixed_size=12,
    max_size=12, children=[ResourceRef<SealedBuffer>,U32] }
Error = builtin kind=08 as H-ABI-005-CANONICAL-001
Result<ReadReply,Error> =
  { kind=06 result, flags=00, align_log2=3, fixed_size=0, max_size=4136,
    children=[ReadReply,Error], kind_payload=empty }
```

These TypeDescriptor bytes use their own explicit packed field order and do not use H-ABI ordinary-value padding. TypeId uses `H("JPH-TYPE-ROW/1", exact_type_descriptor_bytes)`; the resource row uses its canonical `JPH-RESOURCE-ROW/1` JPK-C14N preimage; the interface uses `H("JPH-INTERFACE/1", exact JPK-C14N registry body)`; and the signature uses `H("JPH-SIGNATURE/1", exact semantic function fields including effect_point and terminal_rule)`. No literal digest is asserted: the field-by-field values make every digest independently computable. The error arm is an ordinary declared Result arm, not an untyped side channel.

`TrapOrigin/1 { origin:u8, authenticated:u8, origin_digest:Digest256 }` uses `origin=00 guest_emittable` and `origin=01 internal_engine`; `authenticated` MUST be 01 for internal. Guest `trap(code)` can emit only the published C.12 guest code set with origin 00. Internal engine faults use a reserved non-guest origin code and broker/verifier-authenticated origin flag, so guest bytes cannot impersonate an internal fault or cause unrelated cache quarantine.

### H-ABI-010 — Cancellation, reentrancy, and thread safety

Cancellation is a request. The broker sets `CancelRequested`, stops new work in that scope, wakes cooperative waits, and invokes a service hook. Before an irreversible effect and each bounded wait, the service checks cancellation. A native operation that cannot be interrupted is isolated in a killable helper or refused before admission. Cancellation and timeout do not roll back committed writes, packets, signals, disclosed secrets, or submitted device work. A hard trap terminates the affected isolated instance and guest tasks; it cannot resume an interrupted shared heap.

Each callback/function declares affinity and reentrancy. Fresh authority and reservation checks occur for every nested call. Mutable guest storage cannot survive suspension, callback, or scheduler transfer without a sealed/exclusive lease. Callback overrun or illegal effect is a typed callback failure and stream/device-specific non-success, not successful output.

### H-ABI-011 — Negotiation and refusal

Before activation, the broker verifies JPH major/minor, interface and signature digests, complete type/resource descriptors, rights, callback queues, cancellation modes, effect bounds, and every mandatory profile limit. Missing mandatory service, lower maximum, unavailable affinity, unmeasurable CPU/descendant/device bound, or absent kill/fence backend returns activation refusal. The provider capability record is informational until enforcement is proven by the selected backend. A signature, name, content digest, or normalized payload never grants authority.

<a id="capabilities"></a>
## Capabilities and authority

### H-CAP-001 — One Authority carrier and typed canonical selectors

The capability model reuses the existing `ApplicationAuthority`/`Authority` carrier, `covers`, `allows_operation`, `tighten`, `HostImportFact`, and data-only `HostImportDecision`. `FsScope.operation`, `NetScope.operation`, `IpRangeScope.operation`, and `DeviceScope.operation` are RightKey IDs, not free-form integers. A typed scope with an unknown key or wrong scope kind rejects before authority digesting.

```
FsScope { root_object_id:Digest256, root_generation:u64, segments:List<PathSegment><=256, operation:u32 }
NetScope { scheme:u8, host:HostName, port_lo:u16, port_hi:u16, address_policy:u8, proxy_policy:u8, operation:u32 }
IpRangeScope { family:u8, prefix:u8, address:[u8;16], port_lo:u16, port_hi:u16, operation:u32 }
DeviceScope { device_id:Digest256, device_generation:u64, intervals:List<DeviceInterval><=256>, operation:u32 }
DeviceInterval { offset:u64,length:u64,width_mask:u16,access:u8,endianness:u8,ordering:u8 }
```

`DeviceInterval.width_mask` uses only widths 1/2/4/8 (bits 0..3); intervals are sorted by `(offset,length,width_mask,access,endianness,ordering)` and non-overlapping unless access/width/ordering are identical. Path segments use the grammar in H-SVC-001A. Authority digest hashes sorted canonical Right records. `covers` requires exact root/device generation, descendant path relation, scheme/host/port/policy relation, or interval containment; a raw path/name/URL/device index never grants.
`HostSettingsBundle/1` closes the host-settings authority surface without a free-form right list:
```
read_keys  = [100 FS.Discover, 101 FS.Volume, 102 FS.Read, 103 FS.Open,
              104 FS.List, 110 FS.Watch, 111 IO.Read]
write_keys = [100 FS.Discover, 101 FS.Volume, 102 FS.Read, 103 FS.Open,
              104 FS.List, 105 FS.Write, 106 FS.Flush, 107 FS.Rename,
              108 FS.Delete, 109 FS.Lock, 110 FS.Watch, 111 IO.Read, 112 IO.Write]
```
The two sorted key vectors are fixed metadata; callers cannot add a key by text. Discovery, list, and open remain distinct admissions, and each operation checks the candidate/root generation and typed scope before use.

Every mode-2 deputy call carries broker-issued `CallerContext` as an authenticated attachment or equivalent authenticated call metadata. The frame header remains 64 bytes; `caller_context_digest` is carried in the authenticated session-local call envelope and, when crossing a channel/helper/callback, as a typed `CallerContext` attachment whose TypeId and digest are checked. Downstream admission and receipt bind the exact context digest, origin principal/channel/binding/generation, authority digest, deputy function/rule, attenuation digest, parent epoch, and expiry. Context expires/revokes with the caller/delegation epoch; a lost caller channel cannot leave a live unexpired context unless the service-owned rule explicitly retains it under its own fixed expiry and receipt. Mode 0/1 require caller resource/attenuated scope; mode 2 names a fixed service rule and still records original caller. No ambient URL/path/device selector substitutes for CallerContext.
### H-CAP-002 — Principal, channel, domain, generation, and bootstrap equality

The broker authenticates the principal before channel creation as H-ABI-002 specifies. Each binding is bound to session, channel, transport, principal, instance generation, isolation-domain ID, authority digest, revocation epoch, interface, object ID/generation, and attenuated rights. Every admission, callback, suspension resume, lease operation, and helper completion checks all fields. A live channel or equal object generation alone is insufficient. Attachments are looked up in the authenticated call's session, never a global live-channel map.

### H-CAP-003 — Grant, attenuation, one-shot delegation, and deputies

Grant is deterministic: (1) canonicalize typed rights and reject unknown/noncanonical records; (2) obtain the one `ApplicationAuthority` decision for each `HostImportFact`; (3) acquire the object under a parent resource/root; (4) atomically reserve handle/quota/lease budgets; (5) create a binding with exact session tuple and a new generation; (6) return only a typed resource ref and receipt. Attenuation may remove rights, narrow scope, lower quotas, shorten expiry, or restrict operations. It cannot widen a right, change a principal/object generation/signature, or reset an epoch.

A delegation proposal is this exact broker record:

```
DelegationProposal {
  delegation_id:Digest256
  one_shot_nonce:[u8;32]
  sender_binding_id:u64
  sender_channel_id:[u8;16]
  sender_principal_id:Digest256
  sender_authority_digest:Digest256
  receiver_channel_id:[u8;16]
  receiver_principal_id:Digest256
  parent_object_generation:u64
  parent_revocation_epoch:u64
  interface_digest:Digest256
  operation_subset:List<u32> <=256
  right_subset:List<Right> <=256
  quota_subset:BudgetVector
  expires_monotonic_ns:u64
  state:u8                 # 0 proposed,1 accepted,2 active,3 expired,4 revoked,5 closed
}
```

The broker atomically checks sender equality, receiver live channel, parent generation/epoch, subset coverage, quota availability, nonce, and monotonic expiry, then changes `proposed -> accepted -> active` once. Duplicate acceptance of a still-live accepted proposal returns the same child binding and receipt; it never allocates again, refreshes expiry, or consumes quota twice. A duplicate after close/revoke/expiry returns `stale` or `revoked` and never resurrects. The redacted receipt records sender and receiver identities, proposal ID, parent epoch, child binding, and outcome.

A deputy descriptor declares `caller_context_mode`: `0=caller_resource_required`, `1=attenuated_caller_scope`, `2=explicit_service_owned_rule`. Modes 0/1 require an authenticated caller-supplied resource/delegation; an ambient path, host name, device selector, or URL is not a capability. Downstream authorization is `caller_scope ∩ deputy_policy ∩ operation_scope`. Mode 2 is exceptional and must name a ratified service rule, its fixed service-owned scope, and original caller; every downstream receipt binds both deputy and caller. An interface that accepts unscoped selectors while holding broad deputy authority is refused.

### H-CAP-004 — Filesystem roots, TOCTOU, and no-follow

`volume.open` accepts a broker-issued `ResourceRef<VolumeCandidate>` or a bootstrap-supplied `ResourceRef<VolumeRoot>`, never an ambient volume name. Candidate identity includes object and generation; mount replacement, bookmark expiry, picker revocation, or root change returns `stale`/`denied` and cannot select a new volume by index or text. Every later path is a `RelativePath` under that root. Linux adapters should use `openat2(2)` with declared `RESOLVE_BENEATH`, `RESOLVE_IN_ROOT`, `RESOLVE_NO_MAGICLINKS`, `RESOLVE_NO_SYMLINKS`, and `RESOLVE_NO_XDEV`; Windows reparse behavior and desired access/sharing are explicit; macOS bookmarks and browser picker handles remain revocable scoped resources. Lexical `..` removal is not the security proof.

### H-CAP-005 — Revocation, alias ancestry, and current epoch

Delegated bindings retain explicit revocation ancestry `(parent_binding_id,parent_generation,parent_epoch)`. Ordinary alias close is not ancestor revocation. Object/authority/domain revoke increments the epoch, rejects future admissions, invalidates delegated descendants, and records pending/committed effects. Existing admitted units may complete only within their recorded residual bytes/work/time bound. Expiry prevents new calls and never undoes effects. Generation IDs are exact and non-wrapping.

### H-CAP-006 — Adoption/current-law amendments

The current ambient `IO`, `Mem.Alloc`, and `Exec` behavior remains current law until amendment. The beginner argv/environment/stdio path needs a narrow `Env.Read`/`IO.Stdio` compatibility grant or an explicit child of `IO`; it must not imply ambient `FS.*`, `Net.*`, secrets, or devices. `HostImportFact` must carry interface/signature IDs, modes, affinity, cancellation, limits, and resource IDs while retaining one data-only verdict and one Authority carrier. Existing plugin/Wasmtime lifetime and native Library/`--lib` trust remain unchanged until separately amended. Existing `jet run`/`jet build` remain entry points; JPH does not resurrect retired `jetpack run/build/test/fmt` commands.

### H-CAP-007 — Host-settings authoring projection

**H-CAP-007.** A user-facing host-settings editor MAY produce this proposed logical projection for the existing host-policy/action path; it MUST NOT create a second policy database, sidecar configuration file, new package syntax, or unregistered command:

```text
HostSettingsProjection/1 {
  format:u8 = 01,
  application_node:NodeId,
  base_generation:GenerationId | null,
  bindings:List<HostPathBinding> sorted by logical_name,
  network:NetworkSetting
}
HostPathBinding {
  logical_name:Text<=64,       # e.g. "input" or "output"
  host_path:Text<=4096,        # the only user-editable authority input
  access:u32,                  # bit 0 read, bit 1 write; other bits reject
}
NetworkSetting { state:u8 }    # 00 denied, 01 requested; other values reject
```

`host_path` is an authoring request, not a grant and not a serialized `FsScope.root_object_id`, generation, broker token, or authority digest. The host resolves it to a live `VolumeCandidate`/root and then constructs the canonical typed `FsScope { root_object_id:Digest256, root_generation:u64, segments:List<PathSegment><=256, operation:u32 }`. It obtains the one `ApplicationAuthority`/`HostImportFact` decision for the requested operation and records that decision; a sibling path is a new scope and cannot be treated as attenuation of the old root.

The projection enters the existing graph/action path as `SelectionOverrideRecord { source:"host-policy", key:"fs.logical.<logical_name>.host_path", requested_value:host_path, expected_parent_digest, precedence_rank: derive_from_source(host-policy)=0, override_digest, result, action, receipt }`. The action's `input_digests` bind the current graph/authority/generation, `policy_digest` binds the host policy, `override_digest` binds this complete projection, and `idempotency_tuple_digest` binds the next-activation request. The broker emits the typed candidate/root, authority decision, and receipt; expected-old CAS and readiness checks publish a new generation. The change applies to the next activation, not a live hot swap. Revoking the old binding cannot retract data already read. Users never hand-edit root IDs, epochs, digests, or broker tokens.

<a id="host-services"></a>
## Host service registry

### H-SVC-001 — Canonical types, registry rows, and lifecycle

The `jph.*@1.0` registry is one exact interface projection. Every named type is a registered TypeId with canonical H-ABI descriptor bytes; every function row has a complete signature, canonical Result child, right key, reservation, effect point, affinity, cancellation, and terminal rule. Providers publish complete rows and one interface digest or refuse the family; they cannot omit rows, infer defaults, or vary descriptor values by provider. Snapshot bytes, table capacity, leases, and work are reserved before decoding or external effect. Returned values are broker-owned snapshots; resource references have no guest layout beyond `ResourceRef<T>` and all generic cleanup uses the H-ABI lifecycle rows.

All listed types are registered rows, with the exact fields and constraints below. Resource values have no guest layout beyond `ResourceRef<T>`; value records are canonical H-ABI records.
Every named enum or bitmask is a descriptor-validated u32 scalar unless its field explicitly declares u8/u16/u64. Such scalar dictionaries are not H-ABI variants: variants use a u32 case selector plus payload framing. Unknown enum values/reserved mask bits reject; metadata variants follow their named JPK selector.

### Common/core types

```
FunctionRef {
  instance_generation:Digest256
  function_id:u32le
  signature_digest:Digest256
}
TypedValue { type_id:TypeId, value_bytes:Bytes<=Q16M }
Args = TypedValue
ResultValue = TypedValue
TaskBudget = BudgetVector                 # exact alias; no smaller projection
CallerContext {
  context_id:Digest256 derived, omitted from immutable body encoding
  origin_binding_id:u64
  origin_session_id:[u8;16]
  origin_channel_id:[u8;16]
  origin_principal_id:Digest256
  origin_instance_generation:Digest256
  origin_authority_digest:Digest256
  delegation_chain:List<DelegationHop><=64
  deputy_interface_digest:Digest256
  deputy_function_id:u32le
  deputy_rule_id:u32le
  attenuation_digest:Digest256
  revocation_epoch:u64le
  issued_monotonic_ns:u64le
  expires_monotonic_ns:u64le
}
CallerContextView {
  context_id:Digest256
  state:u8                 # 00 live, 01 expired, 02 revoked, 03 closed
  redaction:u8             # 00 full, 01 receipt-redacted
}
DelegationHop {
  parent_context_digest:Digest256
  deputy_binding_id:u64
  operation_key_id:u32le
  scope_digest:Digest256
  parent_epoch:u64le
  child_epoch:u64le
}
QueueOwner = variant {
  00 CurrentAuthenticatedDomain {}
  01 ExplicitDomain { domain:ResourceRef<IsolationDomain> }
}
QueueSpec { affinity:u8, max_entries:u32le, max_payload_bytes:u32le, owner:QueueOwner }
QueueInfo { queue_id:Digest256, queue_epoch:u64le, affinity:u8,
            owner_domain:ResourceRef<IsolationDomain>, owner_session:[u8;16],
            owner_principal:Digest256, max_entries:u32le, max_payload_bytes:u32le }
CallbackQueueRef = ResourceRef<CallbackQueue>
CallbackRegistrationInfo { registration_id:u64, queue:CallbackQueueRef, queue_epoch:u64le, function_id:u32le, signature_digest:Digest256, state:u8 }
```

`TypedValue` is not an opaque authority blob: before use, the broker resolves `type_id`, checks the exact registered descriptor, validates canonical H-ABI-004 padding/length/enum/child types, and rejects bytes that are not that value. A `FunctionRef` is valid only when its function ID is a callable export in the current authenticated instance and generation and its signature digest exactly matches; another instance requires explicit callable/delegation authority and cannot qualify by matching public hashes. `CallerContext` is broker-issued; guest payloads cannot construct or alter it. Its `context_id = H("CallerContext/1", immutable_body_bytes)` excludes state and redaction; those are later authenticated views/receipts only.

### Filesystem/stream types

```
VolumeCandidateInfo { object_id:Digest256, generation:u64, backend:u8, label:Text<=256,
                      logical_name:Text<=128, case_mode:u8, root_right:Digest256,
                      mount_epoch:u64, path_profile_digest:Digest256 }
VolumeOpenMode: 00 read_only, 01 read_write, 02 exclusive
RemoveRequest { path:RelativePath, remove_mode:u8, tree_bound:u64 } # 00 file,01 empty_dir,02 tree
RemoveCursorInfo { snapshot_id:Digest256, volume_generation:u64, next_object_id:Digest256, remaining_count:u64 }
WatchEvent { watch_generation:u64, sequence:u64, kind:u8, object_id:Digest256,
             path:RelativePath, stat:Opt<FileStat> } # kind 00 create,01 remove,02 modify,03 move,04 attribute
ShutdownDirection: 00 read, 01 write, 02 both
```

`PathSegment` is valid nonempty RFC-3629 UTF-8 of length 1..255 bytes. It preserves the caller's valid bytes and rejects NUL, `/`, `\`, control bytes, `.` and `..`, with no ASCII-only restriction and no silent case folding/normalization. A `RelativePath` has 1..256 segments and no empty segment; resolution is handle-relative to a checked `Volume`/directory object. The host records its case/normalization behavior in `case_mode`; host-specific invalid names (including reserved Windows basenames and Windows-invalid `:` names) return an explicit typed refusal rather than narrowing the portable API to ASCII.

`volume.candidates` returns `List<ResourceRef<VolumeCandidate>>`, not value candidates. `volume.candidate_info` consumes that resource and returns `VolumeCandidateInfo`. `fs.remove` uses `RemoveRequest`; `fs.remove_continue` consumes `R<RemoveCursor>` and returns another report/cursor. `fs.watch_next` consumes `R<Watch>` and returns `WatchEvent`; `stream.read/write/seek/shutdown` are the only consumers of `R<Stream>`.
`VolumeCandidateInfo.logical_name` is the operator-approved logical root name and is unique among the operator-provided roots in one candidate set. Image-filter and settings code select these approved candidate resources and their existing `candidate_info/open` operations; a raw path string never becomes a grant.

### Network/proxy/HTTP types

```
HostName { a_label:Text<=253 }              # RFC 5890 lower-case A-labels
IpAddress { family:u8, bytes:[u8;16] }      # 04 or 06; v4 tail zero
ProxySpec { endpoint:ResourceRef<ResolvedEndpoint>, mode:u8, authority_digest:Digest256,
            tls_server_name:HostName, max_connections:u32,
            credential_binding:Opt<ResourceRef<Secret>> }
ProxyInfo { proxy_id:Digest256, generation:u64, endpoint:ResourceRef<ResolvedEndpoint>,
            mode:u8, authority_digest:Digest256, tls_server_name:HostName,
            policy_digest:Digest256 }
ResolvedProxy { proxy_id:Digest256, generation:u64, endpoint:ResourceRef<ResolvedEndpoint>,
                authority_digest:Digest256, tls_server_name:HostName,
                lease_expires_ns:u64 }
HttpMethod: 00 GET,01 HEAD,02 POST,03 PUT,04 PATCH,05 DELETE,06 OPTIONS,07 CONNECT,08 TRACE
HttpVersion: 00 http11, 01 http2, 02 http3
HttpPolicy {
  schemes:u32, hosts:List<HostName><=128, ports:List<PortRange><=32>,
  address_policy:u8, proxy_policy:u8, proxy:Opt<ResourceRef<Proxy>>,
  max_redirects:u32, max_body_bytes:u64, max_header_bytes:u32,
  dns_rebind:u8, credential_forward:u8, replay:u8, timeout_ns:u64,
  loop_key_mode:u8, transition_policy:u8
}
BodySpec { content_type:Text<=128, bytes:Bytes<=Q16M }
HttpRequest { method:u8, url:HttpUrl, headers:List<HttpHeader><=128>,
              body:Opt<ResourceRef<OpaqueBody>>, body_mode:u8, response_limit:u64 }
HttpResponseInfo { status:u16, version:u8, final_url:HttpUrl,
                   headers:List<HttpHeader><=128>, redirect_count:u32, body_bytes:u64 }
```

HostName uses RFC 5890 A-label syntax: labels are 1..63 ASCII bytes, no leading/trailing `-`, total length <=253, lower-case only, no trailing dot, userinfo, wildcard, percent escape, or implicit port. `IpAddress.family=4` requires bytes 4..15 zero and prefix values 0..32; family 6 prefix values are 0..128. Before `public_only` classification, IPv4-mapped IPv6 addresses are canonicalized to their embedded IPv4 address and checked against the IPv4 policy. NAT64/transition forms are accepted only under a trusted declared `transition_policy` with embedded-address checks; otherwise they are denied, never treated as unknown public space. Special-use classification uses the pinned IANA Special-Purpose Address Registry snapshot at `https://www.iana.org/assignments/special-purpose-address-registry/special-purpose-address-registry.xhtml`, a profile-published snapshot digest, and deterministic longest-prefix matching. `IpRangeScope` host bits must be zero, `port_lo<=port_hi`, and family/prefix must match. Unknown/unclassified ranges reject.

`proxy_policy=declared_proxy` requires a live `R<Proxy>` whose `proxy_id,generation,endpoint,authority_digest,tls_server_name` are authenticated and included in request admission. The proxy must preserve the pinned endpoint and original TLS server name; a proxy that cannot enforce the selected target policy is refused. Proxy-side DNS cannot bypass local endpoint/address checks; every redirect repeats local policy, candidate, proxy, and TLS-name checks. `dns_rebind=pin_first_candidate` pins the lowest canonical `(address bytes,port)` candidate; `pin_all_candidates` pins the sorted complete set; `reject_change` returns `conflict/stale`. Address-class policy is not a promise to defeat arbitrary remote routing, NAT, or reverse proxies.

`body_mode`: 0 sealed_read (caller retains sealed body), 1 exclusive_transfer (caller alias invalidated), 2 copy_snapshot. Body bytes are charged before send; `effective_response_limit=min(HttpRequest.response_limit,HttpPolicy.max_body_bytes)` with zero meaning the explicitly supplied limit is zero, not unlimited. A response over the effective limit returns a typed `exhausted` result after headers/effect acceptance and never delivers bytes beyond the limit. `http.request` returns `ResourceRef<HttpResponse>`; `http.response_info` reads metadata; `http.body_read` consumes the response resource. The response/body resource is bound to client/session/generation and closes at EOF or explicit close.

### Process/task/authority types

```
BudgetVector { fuel:u64, guest_cpu_ns:u64, host_cpu_ns:u64, wall_ns:u64,
  memory_bytes:u64, stack_bytes:u64, snapshot_bytes:u64, input_bytes:u64,
  output_bytes:u64, queue_entries:u32, handle_count:u32, task_count:u32,
  descendant_count:u32, device_bytes:u64, device_work:u64, log_bytes:u64,
  callback_bytes:u64 }
ExecutableCandidateInfo { executable_id:Digest256, generation:u64, target_os:Text<=32,
  target_arch:Text<=32, target_abi:Text<=64, content_digest:Digest256,
  argv_policy_digest:Digest256, authority_digest:Digest256 }
ProcessLimits { budget:BudgetVector, open_files:u32, kill_backend:u8,
                 output_capture_bytes:u64, killable:Bool }
HandleTransfer { kind:u8, resource_type_id:Digest256, attachment_index:u32,
                 rights:List<Right><=32 } # 00 borrow,01 retain,02 move,03 delegate
ProcessSpec { executable:ResourceRef<Executable>, argv:List<Text><=256 each<=65536>,
  environment:List<EnvEntry><=1024>, cwd:ResourceRef<Directory>,
  inherited:List<HandleTransfer><=64>, child_rights:List<Right><=256>,
  limits:ProcessLimits, shell:Bool, redaction:u8 }
```

`TaskBudget` is exactly `BudgetVector`, field-for-field; omitted dimensions are not inherited or zeroed. `process.spawn` accepts only a broker-issued `R<Executable>` opened from a generation-bound `R<ExecutableCandidate>`; a digest lookup claim cannot select an executable. `HandleTransfer.kind` values are exhaustive and each attachment index/type/right/generation is checked against the parent and child attenuation proof. `shell=true` requires right key `Exec.Shell`, otherwise invalid/denied. `redaction` is 0 normal, 1 redact logs, 2 redact all debug; unknown values reject.

### GUI/input/text types

```
EventStreamEvent { window_generation:u64, sequence:u64, kind:u8, payload:TypedValue }
InputDeviceCandidateInfo { device_id:Digest256, generation:u64, kind:u8, name:Text<=256, capabilities:u64 }
InputStreamEvent { device_id:Digest256, generation:u64, sequence:u64, time_ns:u64, kind:u8, code:u32, value:i64 }
ClipboardKind: 00 clipboard, 01 primary, 02 selection
ShapeDirection: 00 ltr, 01 rtl, 02 ttb, 03 btt
ShapeFeatures bits: 0 liga,1 kern,2 calt,3 clig,4 rlig,5 frac,6 case,7 mark,8 mkmk,9 locl; other bits reject
```

`input.devices` returns `List<ResourceRef<InputDeviceCandidate>>`; `input.device_info` consumes one. `input.subscribe` requires the candidate resource and exact generation. `gui.events_next` consumes `R<EventStream>` and returns `EventStreamEvent`; event payload TypeId is fixed by the registration descriptor, not a guest-selected blob. `input.next` consumes `R<InputStream>` and returns `InputStreamEvent`. Display/window/frame formats remain 0 rgba8, 1 bgra8, 2 rgba16f; `stride >= width*bytes_per_pixel` and checked multiplication are mandatory. Clipboard kind and shape direction/features reject unknown values.

### Audio/callback types

```
AudioDeviceCandidateInfo { device_id:Digest256, generation:u64, name:Text<=256,
  direction:u8, formats:u32, min_rate:u32, max_rate:u32, max_channels:u16,
  identity_digest:Digest256 }
AudioSpec { device:ResourceRef<AudioDeviceCandidate>, direction:u8, format:u8,
  rate:u32, channels:u16, quantum_frames:u32, buffer_frames:u32,
  deadline_ns:u64, lease_frames:u64, lease_duration_ns:u64 }
AudioLease { stream_generation:u64, remaining_frames:u64, expires_monotonic_ns:u64,
             renewal_epoch:u64, owner_queue:CallbackQueueRef }
CallbackDescriptor { function_id:u32, signature_digest:Digest256,
  effect_closure_digest:Digest256, queue:CallbackQueueRef, payload_type:TypeId,
  buffer_leases:List<ResourceRef<ExclusiveBuffer>><=32,
  max_payload_bytes:u32, max_fuel:u64, max_cpu_ns:u64, overrun_policy:u8 }
```

`audio.devices` returns generation-bound `R<AudioDeviceCandidate>` resources; `audio.device_info` returns the value snapshot. `audio.open` requires that candidate and copies its generation into the stream binding; reset/hotplug makes it stale. `CallbackDescriptor.queue` is a broker-issued `R<CallbackQueue>`, not a raw digest/epoch. Queue ownership binds queue ID/epoch, session, principal, authority domain, and affinity. `audio.start` checks effect closure, callback function signature/result TypeId, queue equality, lease fields, buffer leases, and render-safe deny set before commit and returns the initial finite `AudioLease`. `quantum_frames<=4096`, `buffer_frames<=1048576`, `lease_frames>0`, `lease_duration_ns>0`, and `deadline_ns>0`; all frame*channel*sample arithmetic is checked. `audio.renew` supplies an explicit `AudioLeaseExtension { additional_frames:u64, additional_duration_ns:u64, expected_renewal_epoch:u64 }` and returns a new lease/receipt; omitted renewal is not infinite. Existing queue/lease work remains charged until callback stop plus observed exit or an operations-owned fence/proof.

### GPU/render/model/tensor/device types

```
WebGPUEditorSource/1 {
  repository:Text = "gpuweb/gpuweb"
  commit:Text = "358eebc8e7bf2d6efa41a4b8b3fbc3a715288204"
  webgpu_path:Text = "spec/index.bs"
  webgpu_source_digest:Digest256 = c9675917738bbcde2e7931221f7c8808b1784e8f371aade64d6366a19a1aa19f
  wgsl_path:Text = "wgsl/index.bs"
  wgsl_source_digest:Digest256 = 11b1020a2fec581050599ae8e035a7138de551129c0856e055b41b4342d658fd
  status:EditorSourceStatus = 00 editor_source
}
EditorSourceStatus: 00 editor_source
CommandListState: 00 finished, 01 submitted, 02 invalid
FenceState: 00 pending, 01 signaled, 02 lost
```

The pinned sources are the exact `w3c/ED` editor definitions at the immutable repository commit above, not a 2024 W3C publication. Their primary URLs are `https://gpuweb.github.io/gpuweb/` and `https://gpuweb.github.io/gpuweb/wgsl/`; the two source paths and SHA-256 values are part of the profile identity.

`JPHWebGPUProjection/1` is the explicit render/compute subset below, with validation and shader meaning inherited from the pinned WebGPU/WGSL source. It does not claim every optional WebGPU extension. The listed JPH records, not a second auto-generated dictionary grammar, are the wire contract. Field spelling maps to the corresponding pinned WebGPU member (`bytes→size`, snake_case→camelCase, explicit nested BlendState color/alpha members); enums map to the corresponding WebGPU string; resource refs map only to checked same-device objects. Required JPH fields are always supplied. An `Opt` field is absent when none; native WebGPU defaults apply only where this paragraph fixes omission, never by provider choice. The fixed omissions are labels empty, shader compilation hints empty, pipeline override-constant maps empty, pipeline layout explicit (not auto), pass timestamp writes/occlusion query absent, maxDrawCount50000000. Pipelines containing WGSL override declarations reject in this baseline rather than ignoring a value. External textures, query sets, indirect draws/dispatch and compressed textures are not in this selected contract; a request requiring one returns unsupported, never drops work. Texture/view/sampler/bind-layout/depth-stencil/pass fields present below retain all their pinned validation and cross-field rules. Numeric widths, list bounds and resource types are exactly the listed JPH declarations. Shader source is complete WGSL UTF-8 and is validated before device handoff. The projection identity is `H("JPH-WEBGPU/1",Record("JPHWebGPUProjection",1,[webgpu_source_digest,wgsl_source_digest,u16le(1)]))`; any changed mapping/default/subset law requires a new revision and identity.

```
ShaderFormat: 00 WGSL_editor_source
OpaqueShader { format:ShaderFormat, source_digest:Digest256, bytes:Bytes<=Q4M }
GpuBufferUsage bits: 0 map_read,1 map_write,2 copy_src,3 copy_dst,
                      4 index,5 vertex,6 uniform,7 storage,8 indirect
GpuBufferDesc { bytes:u64, usage:u32, mapped_at_creation:Bool }
Extent3D { width:u32, height:u32, depth_or_layers:u32 }
TextureDimension: 00 d1, 01 d2, 02 d3
TextureAspect: 00 all, 01 stencil_only, 02 depth_only
TextureSampleCount: 01 one, 04 four
TextureFormat: 00 r8unorm,01 r8snorm,02 r8uint,03 r8sint,
  04 r16uint,05 r16sint,06 r16float,07 rg8unorm,08 rg8snorm,
  09 rg8uint,0A rg8sint,0B r32uint,0C r32sint,0D r32float,
  0E rg16uint,0F rg16sint,10 rg16float,11 rgba8unorm,12 rgba8unorm_srgb,
  13 rgba8snorm,14 rgba8uint,15 rgba8sint,16 bgra8unorm,17 bgra8unorm_srgb,
  18 rgb10a2uint,19 rgb10a2unorm,1A rg11b10ufloat,1B rgb9e5ufloat,
  1C rgba16uint,1D rgba16sint,1E rgba16float,1F rgba32uint,20 rgba32sint,
  21 rgba32float,22 depth16unorm,23 depth24plus,24 depth24plus_stencil8,
  25 depth32float,26 depth32float_stencil8
TextureUsage bits: 0 copy_src,1 copy_dst,2 texture_binding,3 storage_binding,
                   4 render_attachment
TextureDesc {
  size:Extent3D, mip_level_count:u32, sample_count:TextureSampleCount,
  dimension:TextureDimension, format:TextureFormat, usage:u32,
  view_formats:List<TextureFormat><=16
}
TextureViewDesc {
  texture:ResourceRef<Texture>, format:Opt<TextureFormat>,
  dimension:TextureViewDimension, aspect:TextureAspect,
  base_mip_level:u32, mip_level_count:u32,
  base_array_layer:u32, array_layer_count:u32
}
AddressMode: 00 clamp_to_edge, 01 repeat, 02 mirror_repeat
FilterMode: 00 nearest, 01 linear
CompareFunction: 00 never,01 less,02 equal,03 less_equal,04 greater,
                 05 not_equal,06 greater_equal,07 always
SamplerDesc {
  address_mode_u:AddressMode, address_mode_v:AddressMode,
  address_mode_w:AddressMode, mag_filter:FilterMode, min_filter:FilterMode,
  mipmap_filter:FilterMode, lod_min_clamp:f32, lod_max_clamp:f32,
  compare:Opt<CompareFunction>, max_anisotropy:u16
}
BufferBindingType: 00 uniform, 01 storage, 02 read_only_storage
SamplerBindingType: 00 filtering, 01 non_filtering, 02 comparison
TextureSampleType: 00 float, 01 unfilterable_float, 02 depth, 03 sint, 04 uint
TextureViewDimension: 00 d1,01 d2,02 d2_array,03 cube,04 cube_array,05 d3
StorageTextureAccess: 00 write_only, 01 read_write, 02 read_only
BindGroupResourceKind: 00 buffer, 01 sampler, 02 texture, 03 storage_texture
BufferBindingLayout {
  type:BufferBindingType, has_dynamic_offset:Bool, min_binding_size:u64
}
SamplerBindingLayout { type:SamplerBindingType }
TextureBindingLayout {
  sample_type:TextureSampleType, view_dimension:TextureViewDimension,
  multisampled:Bool
}
StorageTextureBindingLayout {
  access:StorageTextureAccess, format:TextureFormat,
  view_dimension:TextureViewDimension
}
BindGroupLayoutEntry {
  binding:u32, visibility:u32, kind:BindGroupResourceKind,
  buffer:Opt<BufferBindingLayout>, sampler:Opt<SamplerBindingLayout>,
  texture:Opt<TextureBindingLayout>, storage_texture:Opt<StorageTextureBindingLayout>
}
BindGroupLayoutSpec { entries:List<BindGroupLayoutEntry><=100 }
BindResource = variant {
  00 Buffer { buffer:ResourceRef<GpuBuffer>, offset:u64, size:u64 }
  01 Sampler { sampler:ResourceRef<Sampler> }
  02 TextureView { view:ResourceRef<TextureView> }
}
BindGroupEntry { binding:u32, resource:BindResource }
BindGroupSpec { layout:ResourceRef<BindGroupLayout>, entries:List<BindGroupEntry><=100 }
PipelineLayoutSpec { bind_group_layouts:List<ResourceRef<BindGroupLayout>><=4 }
BlendFactor: 00 zero,01 one,02 src,03 one_minus_src,04 src_alpha,
             05 one_minus_src_alpha,06 dst,07 one_minus_dst,
             08 dst_alpha,09 one_minus_dst_alpha,0A src_alpha_saturated,
             0B constant,0C one_minus_constant
BlendOperation: 00 add,01 subtract,02 reverse_subtract,03 min,04 max
BlendState {
  color_src:BlendFactor, color_dst:BlendFactor, color_op:BlendOperation,
  alpha_src:BlendFactor, alpha_dst:BlendFactor, alpha_op:BlendOperation
}
PrimitiveTopology: 00 point_list,01 line_list,02 line_strip,
                   03 triangle_list,04 triangle_strip
IndexFormat: 00 uint16, 01 uint32
FrontFace: 00 ccw, 01 cw
CullMode: 00 none, 01 front, 02 back
VertexStepMode: 00 vertex, 01 instance
VertexFormat: 00 uint8x2,01 uint8x4,02 sint8x2,03 sint8x4,
  04 unorm8x2,05 unorm8x4,06 snorm8x2,07 snorm8x4,08 uint16x2,
  09 uint16x4,0A sint16x2,0B sint16x4,0C unorm16x2,0D unorm16x4,
  0E snorm16x2,0F snorm16x4,10 float16x2,11 float16x4,12 float32,
  13 float32x2,14 float32x3,15 float32x4,16 uint32,17 uint32x2,
  18 uint32x3,19 uint32x4,1A sint32,1B sint32x2,1C sint32x3,1D sint32x4
VertexAttribute { shader_location:u32, format:VertexFormat, offset:u64 }
VertexBufferLayout {
  array_stride:u32, step_mode:VertexStepMode,
  attributes:List<VertexAttribute><=16
}
PrimitiveState {
  topology:PrimitiveTopology, strip_index_format:Opt<IndexFormat>,
  front_face:FrontFace, cull_mode:CullMode, unclipped_depth:Bool
}
MultisampleState { count:TextureSampleCount, mask:u32, alpha_to_coverage:Bool }
StencilOperation: 00 keep,01 zero,02 replace,03 invert,04 increment_clamp,
                   05 decrement_clamp,06 increment_wrap,07 decrement_wrap
StencilFaceState {
  compare:CompareFunction, fail_op:StencilOperation,
  depth_fail_op:StencilOperation, pass_op:StencilOperation
}
DepthStencilState {
  format:TextureFormat, depth_write_enabled:Bool, depth_compare:CompareFunction,
  stencil_front:StencilFaceState, stencil_back:StencilFaceState,
  stencil_read_mask:u32, stencil_write_mask:u32,
  depth_bias:i32, depth_bias_slope_scale:f32, depth_bias_clamp:f32
}
ColorTargetState { format:TextureFormat, blend:Opt<BlendState>, write_mask:u32 }
RenderPipelineSpec {
  layout:ResourceRef<PipelineLayout>,
  vertex_shader:ResourceRef<GpuShader>, vertex_entry:Text<=128,
  vertex_buffers:List<VertexBufferLayout><=8,
  fragment_shader:Opt<ResourceRef<GpuShader>>, fragment_entry:Opt<Text>,
  color_targets:List<ColorTargetState><=8,
  depth_stencil:Opt<DepthStencilState>, primitive:PrimitiveState,
  multisample:MultisampleState
}
ComputePipelineSpec {
  layout:ResourceRef<PipelineLayout>, shader:ResourceRef<GpuShader>,
  entry:Text<=128
}
PipelineSpec = variant {
  00 render(RenderPipelineSpec)
  01 compute(ComputePipelineSpec)
}
RenderPassColorAttachment {
  view:ResourceRef<TextureView>, resolve_target:Opt<ResourceRef<TextureView>>,
  depth_slice:Opt<u32>,
  clear_value:[f64;4], load:LoadOp, store:StoreOp
}
RenderPassDepthStencilAttachment {
  view:ResourceRef<TextureView>, depth_clear:Opt<f32>,
  depth_load:Opt<LoadOp>, depth_store:Opt<StoreOp>,
  depth_read_only:Bool, stencil_clear:Opt<u32>,
  stencil_load:Opt<LoadOp>, stencil_store:Opt<StoreOp>,
  stencil_read_only:Bool
}
RenderPassSpec {
  encoder:ResourceRef<CommandEncoder>,
  color_attachments:List<Opt<RenderPassColorAttachment>><=8,
  depth_stencil:Opt<RenderPassDepthStencilAttachment>
}
ComputePassSpec { encoder:ResourceRef<CommandEncoder> }
LoadOp: 00 load, 01 clear
StoreOp: 00 store, 01 discard
PassState: 00 open, 01 ended
EncoderState: 00 open, 01 finished, 02 submitted
PipelineState: 00 ready, 01 lost, 02 invalid
GpuPipelineState { device:ResourceRef<GpuDevice>, spec:PipelineSpec, state:PipelineState }
RenderPassState { encoder:ResourceRef<CommandEncoder>, state:PassState }
ComputePassState { encoder:ResourceRef<CommandEncoder>, state:PassState }
ActivePass = variant {
  00 render(ResourceRef<RenderPass>)
  01 compute(ResourceRef<ComputePass>)
}
CommandEncoderState {
  device:ResourceRef<GpuDevice>, state:EncoderState,
  active_pass:Opt<ActivePass>,
  current_pipeline:Opt<ResourceRef<GpuPipeline>>,
  current_bind_groups:List<Opt<ResourceRef<BindGroup>>><=4,
  current_vertex_buffers:List<Opt<ResourceRef<GpuBuffer>>><=8,
  current_index_buffer:Opt<ResourceRef<GpuBuffer>>,
  command_count:u32
}
CommandListInfo {
  device:ResourceRef<GpuDevice>, state:CommandListState,
  command_count:u32, byte_length:u64
}
Command = variant {
  00 set_pipeline(SetPipelineCommand)
  01 set_bind_group(SetBindGroupCommand)
  02 set_vertex_buffer(SetVertexBufferCommand)
  03 set_index_buffer(SetIndexBufferCommand)
  04 draw(DrawCommand)
  05 draw_indexed(DrawIndexedCommand)
  06 dispatch_workgroups(DispatchWorkgroupsCommand)
  07 copy_buffer_to_buffer(CopyBufferToBufferCommand)
  08 copy_texture_to_texture(CopyTextureToTextureCommand)
  09 clear_buffer(ClearBufferCommand)
  0A copy_buffer_to_texture(BufferTextureCopyCommand)
  0B copy_texture_to_buffer(BufferTextureCopyCommand)
  0C set_viewport(ViewportCommand)
  0D set_scissor_rect(ScissorCommand)
  0E set_blend_constant(BlendConstantCommand)
  0F set_stencil_reference(StencilReferenceCommand)
}
SetPipelineCommand { pipeline:ResourceRef<GpuPipeline> }
SetBindGroupCommand { index:u32, group:ResourceRef<BindGroup>,
                      dynamic_offsets:List<u32><=16 }
SetVertexBufferCommand { slot:u32, buffer:ResourceRef<GpuBuffer>,
                         offset:u64, size:u64 }
SetIndexBufferCommand { buffer:ResourceRef<GpuBuffer>, format:IndexFormat,
                        offset:u64, size:u64 }
DrawCommand { first_vertex:u32, vertex_count:u32,
              first_instance:u32, instance_count:u32 }
DrawIndexedCommand {
  first_index:u32, index_count:u32, base_vertex:i32,
  first_instance:u32, instance_count:u32
}
DispatchWorkgroupsCommand { x:u32, y:u32, z:u32 }
CopyBufferToBufferCommand {
  src:ResourceRef<GpuBuffer>, dst:ResourceRef<GpuBuffer>,
  src_offset:u64, dst_offset:u64, bytes:u64
}
CopyTextureToTextureCommand {
  src:ResourceRef<Texture>, dst:ResourceRef<Texture>,
  src_mip_level:u32, dst_mip_level:u32,
  src_origin:[u32;3], dst_origin:[u32;3], extent:Extent3D
}
ClearBufferCommand { buffer:ResourceRef<GpuBuffer>, offset:u64, size:u64 }
BufferTextureCopyCommand {
  buffer:ResourceRef<GpuBuffer>, offset:u64,
  bytes_per_row:Opt<u32>, rows_per_image:Opt<u32>,
  texture:ResourceRef<Texture>, mip_level:u32, origin:[u32;3],
  aspect:TextureAspect, extent:Extent3D
}
ViewportCommand { x:f32,y:f32,width:f32,height:f32,min_depth:f32,max_depth:f32 }
ScissorCommand { x:u32,y:u32,width:u32,height:u32 }
BlendConstantCommand { rgba:[f64;4] }
StencilReferenceCommand { reference:u32 }
GpuFenceStatus { state:FenceState, bytes_transferred:u64,
                 device_generation:u64, fence_epoch:u64 }
```

Texture sample counts are exactly the pinned WebGPU values `{1,4}`; no invented 2/8 values are accepted. The projection preserves buffer/texture upload, copy, and readback paths. It validates every inherited/default/optional field, object-resource type, binding layout access/sample/view/min-size, texture view dimension/layers, stencil/depth bias and attachment fields before creation. Encoder state is `open -> pass open/ended -> finished -> submitted`; render and compute pass state is `open -> ended`, and commands consume the encoder's current state rather than carrying a second conflicting draw state. Wrong owner/generation/state, missing bindings, device loss, fence failure, or unsupported fields are typed refusal/retained-work outcomes. Zero vertex/instance/index counts read no bytes. For non-indexed draws the last vertex/instance is `first + count - 1`; each attribute end is `buffer_offset + last_index*array_stride + attribute.offset + VertexFormat.byte_width`. For indexed draws zero `index_count` reads no index/vertex bytes; otherwise last index bytes use `first_index + index_count - 1` and fetched vertex bounds use actual validated index values plus `base_vertex`. Mutable index pre-scan is not authority; backend robust access is required. Work/device-loss charges remain until terminal/fence/quiescence.

`gpu.encoder_record` below is the only way to append these commands. It reserves/copies the entire bounded batch, validates it against the current encoder/pass state and appends atomically; rejection leaves the encoder unchanged. Set/draw commands require the matching active render pass; compute pipeline/bind/dispatch require an active compute pass; copies/clear require no active pass. Begin/end service calls append the corresponding private pass-boundary events. `CommandList` is an opaque broker-owned immutable recording, not a second public serialization of those events. The encoder keeps full dynamic binding offsets/ranges/formats, not just the resource-ID projection shown in CommandEncoderState. Begin-pass resets state to the pinned WebGPU defaults; no state leaks between passes. Finish requires no open pass and permanently prevents further recording. Submit validates all retained resources and same-device generations, consumes the finished recording once, and creates a fence. Buffer↔texture copies use pinned WebGPU texel/block/alignment/bytes-per-row/rows-per-image and extent validation; write a staging buffer, record buffer-to-texture copy, submit/fence for upload; use the inverse plus buffer_read for readback. No uncallable texture path is implied. Draw/index/instance zero counts skip every corresponding fetch; nonzero indexed access uses backend-enforced bounds, never a mutable pre-scan alone.

### Pinned ONNX model artifact and tensor types

```
OnnxSchemaSource/1 {
  repository:Text = "onnx/onnx"
  release:Text = "v1.16.0"
  proto_path:Text = "onnx/onnx.proto"
  schema_url:Text = "https://github.com/onnx/onnx/blob/v1.16.0/onnx/onnx.proto"
  schema_digest:Digest256
  projection_revision:u16le = 1
}
OnnxMnistArtifact/1 {
  repository:Text = "onnx/models"
  commit:Text = "4f43949841cb55a0b98dc8fcd045431ccafd9f96"
  path:Text = "validated/vision/classification/mnist/model/mnist-12.onnx"
  sha256:Digest256 = 5c688690f8bacf667d4c2074af5ad0646ca328d7ab03eccf944a65b320171bdd
  byte_length:u32le = 26143
  opset:u16le = 12
}
ExternalDataMode: 00 sealed_relative_only
ModelArtifactSpec {
  source:ResourceRef<SealedBuffer>, content_digest:Digest256,
  format_digest:Digest256, external_data_root:Opt<ResourceRef<Directory>>,
  external_data_mode:ExternalDataMode = 00 sealed_relative_only
}
ModelSpec {
  artifact:ResourceRef<ModelArtifact>, format_digest:Digest256,
  inputs:List<TensorSpec><=64, outputs:List<TensorSpec><=64,
  max_work:u64
}
```

The only model wire representation is the exact sealed ONNX protobuf, not JPH copies of TensorProto/NodeProto/GraphProto/ModelProto. ONNX v1.16.0 [`onnx/onnx.proto`](https://github.com/onnx/onnx/blob/v1.16.0/onnx/onnx.proto), [`docs/IR.md`](https://github.com/onnx/onnx/blob/v1.16.0/docs/IR.md), and [`docs/Operators.md`](https://github.com/onnx/onnx/blob/v1.16.0/docs/Operators.md) supply field numbers, wire types, tensor storage, defaults, and operator schemas. JPH additionally rejects unknown fields, duplicate singular fields, duplicate attribute/initializer/output names, missing required graph inputs, malformed dimensions, cycles, external escape and unsupported domains/opsets at load. Limits are64MiB protobuf,256MiB total initializer bytes,4096 nodes/initializers,256 graph inputs/outputs, rank8, name1024 UTF-8 bytes,64 attributes/node and parsing depth64. Protobuf byte identity is hashed as received; no reserialization silently changes content identity.

`format_digest=H("JPH-ONNX-FORMAT/1",Record("JPHOnnxFormat",1,[Text("onnx/onnx"),Text("v1.16.0"),Text("onnx/onnx.proto"),schema_digest,u16le(1)]))` identifies this parser/operator/numeric contract, not model content. `schema_digest` is SHA256 of the exact pinned proto source; it is verified against that source before a profile is published. ModelArtifact identity separately hashes its sealed protobuf content digest, format_digest and sorted external-file `(RelativePath,length,content_digest)` records. External-data location/offset/length follow pinned ONNX decimal field semantics; paths use the canonical no-escape RelativePath grammar. Before publication the broker snapshots every referenced external file from the granted directory into immutable artifact-owned storage and checks all slices against the captured length. Subsequent directory mutations cannot alter a loaded model. Unknown external-data keys or conflicting inline/external storage reject. Artifact retirement waits for every admitted model/device use.

The model content identity is `H("JPH-MODEL-ARTIFACT/1",Record("ModelArtifactBody",1,[content_digest,format_digest,external_files]))`; external_files is a JPK list of `Record("ModelExternalFile",1,[RelativePath,u64le(length),content_digest])` sorted by encoded RelativePath, without duplicate paths. It includes actual snapshot bytes via SHA256, not a mutable Directory resource ID.

The baseline supports default-domain ONNX opset12, IR versions1..10 with their version-specific validation, and `Identity,Constant,Conv,Relu,MaxPool,Flatten,Gemm,Add,Mul,MatMul,Reshape,Transpose,Softmax`. For each name select the highest `since_version<=12` schema in the pinned [operator history](https://github.com/onnx/onnx/blob/v1.16.0/docs/Changelog.md), indexed by Operators.md; all attributes, defaults, required inputs, optional outputs, shape/type inference and error cases come from that exact schema, not a partial attribute list. Nondefault domains, custom functions, training_info, sparse/sequence/map/optional/graph-valued execution and operators outside this set return unsupported before execution. Shape/index initializers may be i32/i64; executable numeric model tensors in this baseline are f32. Other Tensor storage dtypes remain valid for explicit storage/copy, but are not silently coerced into model execution. The real MNIST file is a pinned corpus candidate; its complete operator/type closure must pass this load rule before any compatibility claim.

Model numeric meaning is nearest-even f32 primitive arithmetic, gradual underflow, no implicit FMA/reassociation and lexical reduction-index order; Conv reduces channel then kernel axes, MatMul/Gemm reduce increasing K, and Softmax uses the selected opset12 axis with correctly rounded exponential and increasing-index sums. Integer shape math is checked. All tiers call the same shared Prelude/host model path; a faster kernel that changes these results is nonconforming. A host that cannot enforce this law returns unsupported rather than using a nondeterministic/low-precision fallback. Strict performance remains unmeasured. Tensor storage contracts are:

Conv adds bias after the lexical product sum. Gemm computes `round(round(alpha*sum)+round(beta*C))`; Add/Mul are per-element, no fusion. Relu returns canonical NaN for a NaN input, otherwise IEEE maximum with+0. MaxPool scans kernel indexes lexically, preserves the first equal maximum and canonicalizes any NaN result; requested unsupported index-output types reject at load. Softmax first finds the lexical maximum, subtracts it per element with f32 rounding, evaluates correctly rounded exp, sums in increasing index order and divides once per element; NaN arithmetic yields the machine's canonical quiet NaN. Reshape/Flatten/Transpose/Identity move values without changing bits. Constant follows exact pinned TensorProto data encoding. These choices are part of format revision1, not a provider optimization flag.

```
DType: 00 f32, 01 f16, 02 bf16, 03 i32, 04 i8, 05 u8
ComputeTarget = variant { 00 cpu(unit), 01 gpu(ResourceRef<GpuDevice>) }
CPU `ComputeTarget` uses the declared affinity `05 device_queue` as its explicit host execution queue; GPU `ComputeTarget` additionally names the checked `GpuDevice` resource. `model.open`, `tensor.create`, and `tensor.dispatch` never silently substitute CPU for a requested GPU target or GPU for CPU; an unavailable target returns the declared typed refusal before execution.
TensorLayout: 00 row_major, 01 channels_last, 02 channels_first, 03 explicit_strided
TensorSpec { dtype:DType, rank:u8, shape:[u64;8], layout:TensorLayout,
             strides:[i64;8], byte_length:u64 }
ModelInfo { format_digest:Digest256, model_digest:Digest256,
            inputs:List<TensorSpec><=64, outputs:List<TensorSpec><=64,
            node_count:u32, max_work:u64 }
```

The pinned MNIST artifact declares batch-1 input `1x1x28x28` float32 grayscale scaled to `[0,1]` and output `1x10` float32 logits before softmax; this is source evidence, not an executed result. `tensor.dispatch` accepts only artifact-declared graph tensors and closed operators, with initializer/resource/device reservations and deterministic numeric policy checked before dispatch.

TensorSpec rank is0..8; unused shape/stride slots are zero. DType byte widths are4/2/2/4/1/1 in enum order. A rank0 tensor is one scalar; any zero extent gives zero elements. Shape products and every `(extent-1)*stride + element_width` range use checked u64 arithmetic before allocation; negative strides reject. Row-major strides are the canonical trailing-axis products in bytes. channels_first/last require rank>=3 and name the channel-axis placement; storage is still contiguous in declared shape order. Explicit_strided supplies all nonnegative byte strides and exact covered byte_length; no hidden host pointer or offset exists. Model inputs/outputs must be canonical row_major f32 matching ModelSpec, not implicitly transposed/coerced. tensor.create zeros storage on the explicit ComputeTarget; tensor.write snapshots checked bytes and excludes concurrent admitted readers/writers until transfer completion; tensor.copy requires identical dtype/shape/layout/strides and performs no numeric conversion. Dispatch retains immutable input leases and returns new output tensors only after completion; CPU and GPU work are accounted and fenced under the same lifecycle law.
### FFI types and lifecycle

```
NativeTrust: 00 trusted_in_process, 01 isolated_helper
NativeAbi: 00 c_abi_scalar_v1, 01 jph_typed_v1
NativeUnwind: 00 no_unwind, 01 helper_boundary
NativeCallback: 00 none, 01 registered_queue
NativeKill: 00 process_kill, 01 job_kill, 02 cgroup_kill, 03 queue_reset, 04 unavailable
NativeAccounting: 00 scheduler, 01 cgroup, 02 helper_meter, 03 unavailable
TrustedNativeDescriptor { package_digest:Digest256,target_digest:Digest256,abi_digest:Digest256,
  principal_id:Digest256,rights:List<Right><=256,in_process:Bool,abi:u8,unwind:u8,
  callback_policy:u8,owner_gate:OwnerGrant,typed_import_digest:Digest256 }
OwnerGrant {
  grant_ref:ResourceRef<AuthorityGrant>
  artifact_digest:Digest256
  target_digest:Digest256
  scope_digest:Digest256
  grant_generation:u64
  revocation_epoch:u64
}
IsolatedNativeHelperDescriptor { package_digest:Digest256,target_digest:Digest256,abi_digest:Digest256,
  typed_import_digest:Digest256,kill_backend:u8,account_backend:u8,stack_bytes:u64,
  callback_queue:Opt<CallbackQueueRef>,max_call_ns:u64,authority:List<Right><=256>,
  budget:BudgetVector }
NativeFunction { function_id:u32, signature_digest:Digest256, parameter_types:List<TypeId><=64>,
  result_type:TypeId, effect_closure_digest:Digest256 }
```

`ffi.trusted_open` is admitted only before hostile activation or in an explicitly trusted profile; it returns `R<NativeModule>` and never crosses a raw pointer. Its `OwnerGrant.grant_ref` must resolve to a live broker-issued `R<AuthorityGrant>` whose artifact digest, target digest, scope digest, generation, and revocation epoch exactly equal the descriptor; a public decision hash or `owner_gate` digest alone is never authority. `ffi.helper_open` returns `R<NativeHelper>` only when kill/accounting backends are not unavailable, callback queue ownership is exact, and the full BudgetVector is enforceable. `ffi.call` consumes `R<NativeHelper>`, a registered `NativeFunction`, and canonical `Args`; its ResultValue TypeId must equal the descriptor result. `ffi.cancel`, `ffi.status`, and `ffi.close` expose bounded helper lifecycle; after a hard JPM trap no helper may re-enter or publish a guest result. `TrustedNativeDescriptor` cannot be imported by a hostile profile. Unknown enum/backend values, pointer/address fields, unregistered functions, or a missing live OwnerGrant are activation refusal.

`AuthorityGrant` is issued only by the authenticated owner-control admission path under the concrete binding's control authority; the issuer binds artifact, target, exact scope, grant generation, and revocation epoch before creating the broker resource. Guest code, an adapter, or a public decision hash cannot construct or substitute a grant.

### H-SVC-001A — Remaining service-local records

The following finite masks and value records complete the signatures in the registry. They are ordinary registered H-ABI values; resource results remain typed `ResourceRef<T>` values.

```
ResolveFlags bits: 0 no_follow, 1 beneath_root, 2 no_magic_links,
                  3 no_xdev, 4 directory_only, 5 must_create, 6 must_not_exist
OpenFlags bits: 0 read, 1 write, 2 create, 3 truncate, 4 append, 5 directory
LockMode: 00 shared, 01 exclusive, 02 try_shared, 03 try_exclusive
SeekWhence: 00 set, 01 current, 02 end
WatchMask bits: 0 create, 1 remove, 2 modify, 3 move, 4 attribute
PathSegment { text:Text<=255 }
RelativePath { segments:List<PathSegment><=256 }
VolumeInfo {
  object_id:Digest256, generation:u64, fs_kind:u8, features:u32,
  max_name_bytes:u32, max_file_size:u64, total_bytes:u64, free_bytes:u64,
  consistency_epoch:u64
}
FileStat {
  object_id:Digest256, generation:u64, kind:u8, mode:u32, size:u64,
  modified_ns:u64, changed_ns:u64, link_count:u32, consistency_epoch:u64
}
DirEntry { object_id:Digest256, generation:u64, kind:u8, name:PathSegment }
DirPage {
  snapshot_id:Digest256, directory_generation:u64, order_epoch:u64,
  entries:List<DirEntry><=1024, next:Opt<ResourceRef<DirCursor>>, done:Bool,
  body_bytes:u32
}
RemoveReport {
  effect:EffectView, deleted_count:u64, deleted_ids:List<Digest256><=1024,
  remaining_count:u64, continuation:Opt<ResourceRef<RemoveCursor>>
}
OpaqueBody { content_type:Text<=128, bytes:Bytes<=Q16M, content_digest:Digest256 }
ResolvedEndpoint {
  authority:NetScope, address:IpAddress, port:u16, resolver_id:Digest256,
  resolution_generation:u64, expires_monotonic_ns:u64, candidate_rank:u32
}
Origin { scheme:u8, host:HostName, port:u16 }
PortRange { lo:u16, hi:u16 }
HttpHeader { name:Text<=128, value:Text<=8192 }
HttpUrl { origin:Origin, path:Text<=8192, query:Text<=8192 }
TlsInfo { version:u16, cipher:u16, peer_digest:Digest256, alpn:Text<=64,
          resumed:Bool, verified:Bool }
UdpPacket { source:ResourceRef<ResolvedEndpoint>, destination:ResourceRef<ResolvedEndpoint>,
            payload:ResourceRef<OpaqueBody> }
ClockInfo { clock_kind:u8, unit:u8, epoch:u8, precision_ns:u64,
            monotonic:Bool, suspend_behavior:u8, privacy_quantum_ns:u64 }
EnvEntry { name:Text<=256, value:Text<=65536, secret:Bool }
ProcessStatus {
  process_id:Digest256, generation:u64, state:u8, exit:Opt<ProcessExit>,
  cpu_ns:u64, wall_ns:u64, memory_bytes:u64, output_bytes:u64, descendants:u32
}
ProcessExit { code:i32, signal:u32, core_dump:Bool, output_truncated:Bool,
              effect:EffectView }
DisplayInfo { display_id:Digest256, generation:u64, width:u32, height:u32,
  scale_num:u32, scale_den:u32, refresh_millihz:u32, main:Bool }
WindowSpec { display:Digest256, width:u32, height:u32, format:u8,
  resizable:Bool, visible:Bool, title:Text<=256 }
Frame { width:u32, height:u32, stride:u32, format:u8,
        pixels:ResourceRef<SealedBuffer>, sequence:u64 }
FontInfo { font_id:Digest256, family:Text<=256, style:u32, units_per_em:u32 }
ShapeOptions { direction:u8, script:Text<=32, language:Text<=32, features:u64 }
Glyph { glyph_id:u32, advance_x:i32, advance_y:i32, offset_x:i32, offset_y:i32 }
GlyphRun { font_id:Digest256, glyphs:List<Glyph><=65536, text_hash:Digest256 }
AxNode { node_id:u64, role:u32, name:Text<=4096, value:Text<=4096,
  bounds:[i32;4], children:List<u64><=4096, actions:u32 }
AxTree { window_generation:u64, root:u64, nodes:List<AxNode><=65536, snapshot_epoch:u64 }
AxAction { node_id:u64, action:u32, value:Opt<Text> }
AudioState { state:u8, device_generation:u64, frames_submitted:u64,
  frames_rendered:u64, underruns:u64, overruns:u64, latency_ns:u64 }
AdapterCandidate { adapter_id:Digest256, generation:u64, vendor_id:u32,
  device_id:u32, driver_digest:Digest256, validator_digest:Digest256,
  features:u64, memory_bytes:u64, queue_count:u32 }
AdapterInfo { candidate:ResourceRef<AdapterCandidate>, name:Text<=256,
  architecture:Text<=128, api_versions:List<u32><=32 }
DeviceRequest { required_features:u64, min_memory:u64, max_queues:u32,
  shader_formats:u32, tensor_formats:u32 }
DeviceSelector { candidate:ResourceRef<DeviceCandidate>, expected_generation:u64 }
DeviceInfo { device_id:Digest256, generation:u64, class:Text<=128,
  address_width:u8, driver_digest:Digest256, validator_digest:Digest256,
  ranges:List<DeviceRange><=256, dma_coherent:Bool, max_inflight:u32 }
DeviceRange { offset:u64, length:u64, width_mask:u16, alignment:u32,
  access:u8, endianness:u8, ordering:u8 }
OpaqueDevicePayload { interface_digest:Digest256, operation_code:u32,
  bytes:Bytes<=65536 }
DeviceControlRequest { interface_digest:Digest256, operation_code:u32,
  payload:OpaqueDevicePayload }
DmaDescriptor { direction:u8, buffer:ResourceRef<ExclusiveBuffer>, buffer_offset:u64,
  device_offset:u64, length:u64, alignment:u32, cache_mode:u8, fence_mode:u8 }
DmaFenceStatus { state:u8, bytes_transferred:u64, device_generation:u64,
  fence_epoch:u64 }
```

Unknown mask bits and enum values reject. `PathSegment` and `RelativePath` use the path grammar above. Network, process, GUI, audio, adapter, and device records carry the generation/resource bindings checked by their operation rows; no numeric index or raw pointer is an authority.
### H-SVC-002 — Operation rows and callable lifecycle

The following rows instantiate every existing and newly added operation. Each row is `(function_id, name, parameter types -> declared Result success type, right key, call_mode, affinity, reentrancy, cancellation, max_request/max_reply, cost_class, effect_point, terminal_rule)` and ends at `terminal_rule`. All rows use the canonical Error child; success is always the first child of the declared Result. Callback registration rows use `call_mode=2`, callback delivery rows use `call_mode=3`, and ordinary completion is distinguishable from callback completion by authenticated frame kind, direction, and correlation.

### `jph.core@1.0`

* 10 `callback.queue_open`: `QueueSpec -> Result<R<CallbackQueue>>`, `Callback.Register`, `0,4,0,3,Q16K,Q16K,1,1,0`
* 11 `callback.queue_close`: `R<CallbackQueue> -> Result<unit>`, `Callback.Close`, `0,4,0,1,Q4K,Q4K,1,4,0`
* 12 `caller_context.query`: `R<CallerContext> -> Result<CallerContextInfo>`, `Callback.Register`, `0,4,0,0,Q4K,Q16K,1,0,0`
* 13 `callback.register`: `(R<CallbackQueue>,FunctionRef,CallbackDescriptor) -> Result<R<CallbackRegistration>>`, `Callback.Register`, `2,4,0,3,Q16K,Q16K,2,1,0`
* 14 `callback.unregister`: `R<CallbackRegistration> -> Result<unit>`, `Callback.Close`, `0,4,0,1,Q4K,Q4K,1,4,0`
* 15 `async.cancel`: `(u64 call_id) -> Result<unit>`, `Callback.Close`, `0,1,0,1,Q4K,Q4K,1,2,0`

Concrete resource lifecycle rows are the `0x1000 + 3*n + {0,1,2}` formula above, with the full generated descriptors stated in H-ABI-005-CANONICAL-001. `resource.await_quiescence` returns the genuine empty scoped proof for a closed resource with no admitted work; it never returns `unsupported` merely because the inventory is empty.

### `jph.fs@1.0` (IDs 100–123)

* 100 `volume.candidates`: `() -> Result<List<ResourceRef<VolumeCandidate>>>`, `FS.Discover`, `0,4,0,1,Q4K,Q16M,1,0,0`
* 101 `volume.candidate_info`: `R<VolumeCandidate> -> Result<VolumeCandidateInfo>`, `FS.Discover`, `0,4,0,1,Q4K,Q16K,1,0,0`
* 102 `volume.open`: `(R<VolumeCandidate>,VolumeOpenMode) -> Result<R<Volume>>`, `FS.Volume`, `0,4,0,3,Q4K,Q4K,1,1,0`
* 103 `volume.info`: `R<Volume> -> Result<VolumeInfo>`, `FS.Read`, `0,4,0,1,Q4K,Q16K,1,0,0`
* 104 `fs.open`: `(R<Volume>,RelativePath,OpenFlags,ResolveFlags) -> Result<R<File>>`, `FS.Open`, `0,4,0,3,Q16K,Q4K,2,1,0`
* 105 `fs.list`: `(R<Volume>,RelativePath,ResolveFlags,Opt<R<DirCursor>>,u32) -> Result<DirPage>`, `FS.List`, `0,4,0,1,Q16K,Q16M,2,2,0`
* 106 `fs.stat`: `R<File> -> Result<FileStat>`, `FS.Read`, `0,4,0,1,Q4K,Q16K,1,0,0`
* 107 `fs.read`: `(R<File>,u64,u32) -> Result<Bytes>`, `FS.Read`, `1,1,0,1,Q4K,Q16M,2,2,1`
* 108 `fs.write`: `(R<File>,u64,Bytes) -> Result<u32>`, `FS.Write`, `0,1,0,3,Q16M,Q4K,2,3,0`
* 109 `fs.flush`: `R<File> -> Result<unit>`, `FS.Flush`, `0,1,0,1,Q4K,Q4K,2,4,0`
* 110 `fs.rename`: `(R<Volume>,RelativePath,RelativePath,Bool) -> Result<unit>`, `FS.Rename`, `0,4,0,3,Q16K,Q4K,2,4,0`
* 111 `fs.remove`: `(R<Volume>,RemoveRequest) -> Result<RemoveReport>`, `FS.Delete`, `0,4,0,1,Q16K,Q16M,3,4,0`
* 112 `fs.remove_continue`: `(R<RemoveCursor>,u32) -> Result<RemoveReport>`, `FS.Delete`, `1,4,0,1,Q4K,Q16M,2,4,1`
* 113 `fs.lock`: `(R<File>,LockMode,u64) -> Result<R<Lock>>`, `FS.Lock`, `1,4,0,1,Q4K,Q4K,2,1,1`
* 114 `fs.watch`: `(R<Volume>,RelativePath,WatchMask) -> Result<R<Watch>>`, `FS.Watch`, `0,4,0,3,Q16K,Q4K,2,2,0`
* 115 `fs.watch_next`: `(R<Watch>,u32,u64) -> Result<WatchEvent>`, `FS.Watch`, `1,4,0,1,Q4K,Q16M,2,2,1`
* 116 `stream.read`: `(R<Stream>,u32) -> Result<Bytes>`, `IO.Read`, `1,1,0,1,Q4K,Q16M,2,2,1`
* 117 `stream.write`: `(R<Stream>,Bytes) -> Result<u32>`, `IO.Write`, `0,1,0,3,Q16M,Q4K,2,3,0`
* 118 `stream.seek`: `(R<Stream>,i64,SeekWhence) -> Result<u64>`, `IO.Seek`, `0,1,0,1,Q4K,Q4K,1,1,0`
* 119 `stream.shutdown`: `(R<Stream>,ShutdownDirection) -> Result<unit>`, `IO.Close`, `0,1,0,1,Q4K,Q4K,1,4,0`
* 120 `volume.sync`: `R<Volume> -> Result<unit>`, `FS.Flush`, `0,4,0,1,Q4K,Q4K,2,4,0`

### `jph.net@1.0` (IDs 200–223)

* 200 `dns.resolve`: `(HostName,Opt<u16>,u8 family) -> Result<List<ResourceRef<ResolvedEndpoint>>>`, `Net.Resolve`, `1,4,0,1,Q4K,Q16M,2,2,1`
* 201 `net.tcp_connect`: `(R<ResolvedEndpoint>,u64) -> Result<R<Tcp>>`, `Net.Connect`, `1,1,0,1,Q4K,Q4K,2,3,1`
* 202 `net.tcp_listen`: `(NetScope,u32) -> Result<R<Listener>>`, `Net.Listen`, `0,4,0,3,Q16K,Q4K,2,4,0`
* 203 `net.accept`: `R<Listener> -> Result<R<Tcp>>`, `Net.Accept`, `1,4,0,1,Q4K,Q4K,2,3,1`
* 204 `net.recv`: `(R<Tcp>,u32) -> Result<Bytes>`, `Net.Receive`, `1,1,0,1,Q4K,Q16M,2,2,1`
* 205 `net.send`: `(R<Tcp>,Bytes) -> Result<u32>`, `Net.Send`, `0,1,0,3,Q16M,Q4K,2,3,0`
* 206 `net.udp_open`: `(u8 family) -> Result<R<Udp>>`, `Net.Connect`, `0,4,0,3,Q4K,Q4K,1,1,0`
* 207 `net.udp_send`: `(R<Udp>,R<ResolvedEndpoint>,R<OpaqueBody>) -> Result<u32>`, `Net.Send`, `0,1,0,3,Q16M,Q4K,2,3,0`
* 208 `net.udp_recv`: `(R<Udp>,u32) -> Result<UdpPacket>`, `Net.Receive`, `1,1,0,1,Q4K,Q16M,2,2,1`
* 209 `tls.client`: `(R<Tcp>,HostName,List<Text><=16>) -> Result<R<Tls>>`, `Net.TLS`, `0,1,0,1,Q16K,Q4K,2,1,0`
* 210 `tls.handshake`: `(R<Tls>,u64) -> Result<TlsInfo>`, `Net.TLS`, `1,1,0,1,Q4K,Q16K,2,3,1`
* 211 `tls.send`: `(R<Tls>,Bytes) -> Result<u32>`, `Net.TLS`, `0,1,0,3,Q16M,Q4K,2,3,0`
* 212 `tls.recv`: `(R<Tls>,u32) -> Result<Bytes>`, `Net.TLS`, `1,1,0,1,Q4K,Q16M,2,2,1`
* 213 `proxy.open`: `ProxySpec -> Result<R<Proxy>>`, `Net.Proxy`, `0,4,0,3,Q16K,Q4K,2,1,0`
* 214 `proxy.info`: `R<Proxy> -> Result<ProxyInfo>`, `Net.Proxy`, `0,4,0,1,Q4K,Q16K,1,0,0`
* 215 `http.open_client`: `HttpPolicy -> Result<R<HttpClient>>`, `Net.HTTP`, `0,4,0,3,Q16K,Q4K,1,1,0`
* 216 `http.body_from_bytes`: `BodySpec -> Result<R<OpaqueBody>>`, `Net.HTTP`, `0,1,0,3,Q16M,Q4K,2,1,0`
* 217 `http.request`: `(R<HttpClient>,HttpRequest) -> Result<R<HttpResponse>>`, `Net.HTTP`, `1,1,0,1,Q16M,Q4K,3,4,1`
* 218 `http.response_info`: `R<HttpResponse> -> Result<HttpResponseInfo>`, `Net.HTTP`, `0,1,0,1,Q4K,Q16M,1,0,0`
* 219 `http.body_read`: `(R<BodyStream>,u32) -> Result<Bytes>`, `Net.HTTP`, `1,1,0,1,Q4K,Q16M,2,2,1`
* 220 `http.close`: `R<HttpClient> -> Result<unit>`, `Net.HTTP`, `0,1,0,1,Q4K,Q4K,1,4,0`

### `jph.time@1.0` (IDs 300–306)

* 300 `time.wall`: `() -> Result<u64>`, `Time.Read`, `0,1,0,0,Q4K,Q4K,1,0,0`
* 301 `time.monotonic`: `() -> Result<u64>`, `Time.Read`, `0,1,0,0,Q4K,Q4K,1,0,0`
* 302 `time.boottime`: `() -> Result<u64>`, `Time.Read`, `0,1,0,0,Q4K,Q4K,1,0,0`
* 303 `time.info`: `() -> Result<ClockInfo>`, `Time.Read`, `0,1,0,0,Q4K,Q16K,1,0,0`
* 304 `time.sleep`: `u64 -> Result<unit>`, `Time.Sleep`, `1,1,0,1,Q4K,Q4K,1,0,1`
* 305 `random.secure`: `u32 -> Result<Bytes>`, `Random.Secure`, `0,4,0,3,Q4K,Q16M,1,1,0`
* 306 `random.deterministic`: `(u64,u32) -> Result<Bytes>`, `Random.Deterministic`, `0,1,0,3,Q4K,Q16M,1,4,0`

### `jph.exec@1.0` (IDs 400–424)

* 400 `cli.argv`: `() -> Result<List<Text>>`, `IO.Stdio`, `0,1,0,0,Q4K,Q16M,1,0,0`
* 401 `cli.cwd`: `() -> Result<RelativePath>`, `IO.Stdio`, `0,1,0,0,Q4K,Q16K,1,0,0`
* 402 `env.get`: `Text -> Result<Opt<Text>>`, `Env.Read`, `0,1,0,0,Q4K,Q16M,1,0,0`
* 403 `env.list`: `() -> Result<List<EnvEntry>>`, `Env.Read`, `0,1,0,0,Q4K,Q16M,1,0,0`
* 404 `stdio.stdin`: `() -> Result<R<Stream>>`, `IO.Stdio`, `0,1,0,0,Q4K,Q4K,1,1,0`
* 405 `stdio.stdout`: `() -> Result<R<Stream>>`, `IO.Stdio`, `0,1,0,0,Q4K,Q4K,1,1,0`
* 406 `stdio.stderr`: `() -> Result<R<Stream>>`, `IO.Stdio`, `0,1,0,0,Q4K,Q4K,1,1,0`
* 407 `secret.open`: `Text -> Result<R<Secret>>`, `Secret.Read`, `0,4,0,3,Q4K,Q4K,1,1,0`
* 408 `secret.read`: `(R<Secret>,u32) -> Result<Bytes>`, `Secret.Read`, `0,1,0,3,Q4K,Q16M,2,3,0`
* 409 `process.executable_candidates`: `() -> Result<List<ResourceRef<ExecutableCandidate>>>`, `Exec.Resolve`, `0,4,0,1,Q4K,Q16M,1,0,0`
* 410 `process.executable_info`: `R<ExecutableCandidate> -> Result<ExecutableCandidateInfo>`, `Exec.Resolve`, `0,4,0,1,Q4K,Q16K,1,0,0`
* 411 `process.executable_open`: `R<ExecutableCandidate> -> Result<R<Executable>>`, `Exec.Resolve`, `0,4,0,3,Q4K,Q4K,1,1,0`
* 412 `process.spawn`: `ProcessSpec -> Result<R<Process>>`, `Exec.Spawn`, `0,4,0,3,Q16M,Q4K,3,1,0`
* 413 `process.status`: `R<Process> -> Result<ProcessStatus>`, `Exec.Read`, `0,4,0,1,Q4K,Q16K,1,0,0`
* 414 `process.wait`: `(R<Process>,u64) -> Result<ProcessExit>`, `Exec.Wait`, `1,1,0,1,Q4K,Q16K,2,2,1`
* 415 `process.signal`: `(R<Process>,Signal) -> Result<unit>`, `Exec.Signal`, `0,1,0,3,Q4K,Q4K,1,3,0`
* 416 `process.kill`: `R<Process> -> Result<unit>`, `Exec.Kill`, `0,4,0,2,Q4K,Q4K,2,3,0`
* 417 `task.spawn`: `(FunctionRef,Args,TaskBudget) -> Result<R<Task>>`, `Task.Spawn`, `0,1,0,3,Q16M,Q4K,3,1,0`
* 418 `task.join`: `(R<Task>,u64) -> Result<ResultValue>`, `Task.Join`, `1,1,0,1,Q4K,Q16M,2,2,1`
* 419 `task.cancel`: `R<Task> -> Result<unit>`, `Task.Cancel`, `0,1,0,1,Q4K,Q4K,1,2,0`
* 420 `task.detach`: `(R<Task>,R<IsolationDomain>,TaskBudget) -> Result<R<DetachedTask>>`, `Task.Detach`, `0,1,0,3,Q16K,Q4K,2,1,0`
* 421 `task.yield`: `() -> Result<unit>`, `Task.Yield`, `0,1,0,1,Q4K,Q4K,1,0,0`
* 422 `process.signal_info`: `u32 -> Result<SignalInfo>`, `Exec.Signal`, `0,4,0,0,Q4K,Q4K,1,0,0`

### `jph.gui@1.0` (IDs 500–523)

* 500 `gui.displays`: `() -> Result<List<DisplayInfo>>`, `GUI.Read`, `0,2,0,0,Q4K,Q16M,1,0,0`
* 501 `gui.window_create`: `WindowSpec -> Result<R<Window>>`, `GUI.Window`, `0,2,0,3,Q16K,Q4K,2,1,0`
* 502 `gui.window_present`: `(R<Window>,Frame) -> Result<unit>`, `GUI.Present`, `0,3,0,3,Q16M,Q4K,2,4,0`
* 503 `gui.window_close`: `R<Window> -> Result<unit>`, `GUI.Window`, `0,2,0,1,Q4K,Q4K,1,4,0`
* 504 `gui.events`: `(R<Window>,GuiEventMask) -> Result<R<EventStream>>`, `GUI.Input`, `0,2,0,3,Q4K,Q4K,1,2,0`
* 505 `gui.events_next`: `(R<EventStream>,u32,u64) -> Result<EventStreamEvent>`, `GUI.Input`, `1,2,0,1,Q4K,Q16M,2,2,1`
* 506 `input.devices`: `() -> Result<List<ResourceRef<InputDeviceCandidate>>>`, `Input.Read`, `0,1,0,0,Q4K,Q16M,1,0,0`
* 507 `input.device_info`: `R<InputDeviceCandidate> -> Result<InputDeviceCandidateInfo>`, `Input.Read`, `0,1,0,0,Q4K,Q16K,1,0,0`
* 508 `input.subscribe`: `(R<InputDeviceCandidate>,InputMask) -> Result<R<InputStream>>`, `Input.Read`, `0,1,0,3,Q4K,Q4K,2,2,0`
* 509 `input.next`: `(R<InputStream>,u64) -> Result<InputStreamEvent>`, `Input.Read`, `1,1,0,1,Q4K,Q16M,2,2,1`
* 510 `text.fonts`: `() -> Result<List<FontInfo>>`, `Text.Read`, `0,1,0,0,Q4K,Q16M,1,0,0`
* 511 `text.shape`: `(Text,Text,ShapeOptions) -> Result<GlyphRun>`, `Text.Shape`, `0,1,0,3,Q16K,Q16M,2,4,0`
* 512 `clipboard.get`: `u32 -> Result<Text>`, `Clipboard.Read`, `0,2,0,1,Q4K,Q16M,1,0,0`
* 513 `clipboard.set`: `(u32,Text) -> Result<unit>`, `Clipboard.Write`, `0,2,0,3,Q16M,Q4K,1,4,0`
* 514 `accessibility.tree`: `(R<Window>,u32) -> Result<AxTree>`, `Accessibility.Read`, `0,2,0,1,Q4K,Q16M,2,0,0`
* 515 `accessibility.action`: `(R<Window>,AxAction) -> Result<unit>`, `Accessibility.Act`, `0,2,0,3,Q16K,Q4K,1,4,0`

### `jph.audio@1.0` (IDs 600–608)

* 600 `audio.devices`: `() -> Result<List<ResourceRef<AudioDeviceCandidate>>>`, `Audio.Enumerate`, `0,4,0,0,Q4K,Q16M,1,0,0`
* 601 `audio.device_info`: `R<AudioDeviceCandidate> -> Result<AudioDeviceCandidateInfo>`, `Audio.Enumerate`, `0,4,0,0,Q4K,Q16K,1,0,0`
* 602 `audio.open`: `AudioSpec -> Result<R<AudioStream>>`, `Audio.Open`, `0,5,0,3,Q16K,Q4K,2,1,0`
* 603 `audio.start`: `(R<AudioStream>,CallbackDescriptor) -> Result<AudioLease>`, `Audio.Play`, `0,3,0,3,Q16K,Q16K,3,4,0`
* 604 `audio.render_ack`: `(R<AudioStream>,u32) -> Result<unit>`, `Audio.Record`, `0,3,0,3,Q16K,Q4K,2,3,0`
* 605 `audio.stop`: `R<AudioStream> -> Result<unit>`, `Audio.Play`, `0,3,0,1,Q4K,Q4K,1,4,0`
* 606 `audio.info`: `R<AudioStream> -> Result<AudioState>`, `Audio.Read`, `0,3,0,0,Q4K,Q16K,1,0,0`
* 607 `audio.renew`: `(R<AudioStream>,AudioLeaseExtension) -> Result<AudioLease>`, `Audio.Queue`, `0,3,0,3,Q4K,Q16K,2,2,0`

### `jph.gpu@1.0` and `jph.ml@1.0`
`jph.gpu@1.0` owns IDs700–706,708–712 and719–729; `jph.ml@1.0` owns IDs713–720. Function IDs are local to the exact interface digest, so ML719/720 and GPU719/720 do not collide. They are separate interface descriptors and nominal-resource tables.

* 700 `gpu.adapters`: `() -> Result<List<AdapterInfo>>`, `GPU.Discover`, `0,4,0,1,Q4K,Q16M,1,0,0`
* 701 `gpu.open`: `(R<AdapterCandidate>,DeviceRequest) -> Result<R<GpuDevice>>`, `GPU.Open`, `0,5,0,3,Q16K,Q4K,2,1,0`
* 702 `gpu.buffer_create`: `(R<GpuDevice>,GpuBufferDesc) -> Result<R<GpuBuffer>>`, `GPU.Memory`, `0,5,0,3,Q16K,Q4K,2,1,0`
* 703 `gpu.buffer_write`: `(R<GpuBuffer>,u64,Bytes) -> Result<unit>`, `GPU.Transfer`, `1,5,0,3,Q16M,Q4K,2,3,1`
* 704 `gpu.buffer_read`: `(R<GpuBuffer>,u64,u32) -> Result<Bytes>`, `GPU.Readback`, `1,5,0,1,Q4K,Q16M,2,2,1`
* 705 `gpu.shader_create`: `(R<GpuDevice>,OpaqueShader) -> Result<R<GpuShader>>`, `GPU.Shader`, `0,5,0,3,Q4M,Q4K,3,5,0`
* 706 `gpu.pipeline_create`: `(R<GpuDevice>,PipelineSpec) -> Result<R<GpuPipeline>>`, `GPU.Render`, `0,5,0,3,Q16K,Q4K,3,5,0`
* 708 `gpu.render_pass_begin`: `(R<GpuDevice>,RenderPassSpec) -> Result<R<RenderPass>>`, `GPU.Render`, `0,5,0,3,Q16K,Q4K,2,2,0`
* 709 `gpu.render_pass_end`: `R<RenderPass> -> Result<unit>`, `GPU.Render`, `0,5,0,1,Q4K,Q4K,1,4,0`
* 710 `gpu.submit`: `(R<GpuDevice>,R<CommandList>) -> Result<R<GpuFence>>`, `GPU.Submit`, `1,5,0,2,Q16M,Q4K,3,5,1`
* 711 `gpu.fence_wait`: `(R<GpuFence>,u64) -> Result<unit>`, `GPU.Wait`, `1,5,0,1,Q4K,Q4K,2,2,1`
* 712 `gpu.fence_status`: `R<GpuFence> -> Result<GpuFenceStatus>`, `GPU.Wait`, `0,5,0,0,Q4K,Q16K,1,0,0`
* 713 `tensor.create`: `(ComputeTarget,TensorSpec) -> Result<R<Tensor>>`, `ML.Tensor`, `0,5,0,3,Q16K,Q4K,2,1,0`
* 714 `tensor.copy`: `(R<Tensor>,R<Tensor>) -> Result<unit>`, `ML.Transfer`, `1,5,0,2,Q4K,Q4K,2,5,1`
* 715 `model.open`: `(ComputeTarget,ModelSpec) -> Result<R<Model>>`, `ML.Execute`, `0,5,0,3,Q16K,Q4K,3,5,0`
* 716 `model.info`: `R<Model> -> Result<ModelInfo>`, `ML.Execute`, `0,5,0,0,Q4K,Q16M,1,0,0`
* 717 `tensor.dispatch`: `(R<Model>,List<R<Tensor>>) -> Result<List<R<Tensor>>>`, `ML.Execute`, `1,5,0,2,Q16K,Q16M,3,5,1`
* 718 `tensor.read`: `(R<Tensor>,u32) -> Result<Bytes>`, `ML.Readback`, `1,5,0,1,Q4K,Q16M,2,2,1`

### `jph.device@1.0` (IDs 800–812)

* 800 `device.enumerate`: `Text -> Result<List<ResourceRef<DeviceCandidate>>>`, `Device.Discover`, `0,4,0,0,Q4K,Q16M,1,0,0`
* 801 `device.candidate_info`: `R<DeviceCandidate> -> Result<DeviceCandidateInfo>`, `Device.Discover`, `0,4,0,0,Q4K,Q16K,1,0,0`
* 802 `device.claim`: `DeviceSelector -> Result<R<Device>>`, `Device.Claim`, `0,5,0,3,Q16K,Q4K,2,1,0`
* 803 `device.info`: `R<Device> -> Result<DeviceInfo>`, `Device.Status`, `0,5,0,0,Q4K,Q16M,1,0,0`
* 804 `device.control`: `(R<Device>,DeviceControlRequest) -> Result<OpaqueDevicePayload>`, `Device.Control`, `0,5,0,2,Q16K,Q16M,3,5,0`
* 805 `device.mmio_read`: `(R<Device>,u64,u32) -> Result<u64>`, `Device.MMIO.Read`, `0,5,0,3,Q4K,Q4K,1,5,0`
* 806 `device.mmio_write`: `(R<Device>,u64,u32,u64) -> Result<unit>`, `Device.MMIO.Write`, `0,5,0,3,Q4K,Q4K,1,5,0`
* 807 `device.dma`: `(R<Device>,DmaDescriptor) -> Result<R<DmaFence>>`, `Device.DMA`, `1,5,0,2,Q16K,Q4K,3,5,1`
* 808 `device.dma_status`: `R<DmaFence> -> Result<DmaFenceStatus>`, `Device.Status`, `0,5,0,0,Q4K,Q16K,1,0,0`
* 809 `device.dma_wait`: `(R<DmaFence>,u64) -> Result<DmaFenceStatus>`, `Device.Status`, `1,5,0,1,Q4K,Q16K,2,2,1`
* 810 `embedded.watchdog_feed`: `R<Device> -> Result<unit>`, `Embedded.Watchdog`, `0,5,0,3,Q4K,Q4K,1,3,0`
* 811 `embedded.sleep`: `u64 -> Result<unit>`, `Embedded.Power`, `1,5,0,1,Q4K,Q4K,1,0,1`

### `jph.ffi@1.0` (IDs 900–907)

* 900 `ffi.trusted_open`: `TrustedNativeDescriptor -> Result<R<NativeModule>>`, `FFI.Trusted`, `0,4,0,3,Q16K,Q4K,3,1,0`
* 901 `ffi.helper_open`: `IsolatedNativeHelperDescriptor -> Result<R<NativeHelper>>`, `FFI.Helper`, `0,4,0,3,Q16K,Q4K,3,1,0`
* 902 `ffi.function_register`: `(R<NativeHelper>,NativeFunction) -> Result<unit>`, `FFI.Helper`, `0,4,0,3,Q16K,Q4K,2,1,0`
* 903 `ffi.call`: `(R<NativeHelper>,u32,Args) -> Result<ResultValue>`, `FFI.Call`, `1,1,0,2,Q16M,Q16M,3,5,1`
* 904 `ffi.cancel`: `R<NativeHelper> -> Result<unit>`, `FFI.Cancel`, `0,4,0,2,Q4K,Q4K,1,2,0`
* 905 `ffi.status`: `R<NativeHelper> -> Result<NativeCallStatus>`, `FFI.Read`, `0,4,0,0,Q4K,Q16K,1,0,0`
* 906 `ffi.close`: `R<NativeHelper> -> Result<unit>`, `FFI.Close`, `0,4,0,1,Q4K,Q4K,1,4,0`

For all asynchronous rows, adapter-internal `AcceptedReply` reserves and returns the channel call ID; it is not a guest-visible Result or SSA value. The terminal completion is correlated to the suspended continuation and carries exactly the row's Result. For all callback rows, registration creates `R<CallbackRegistration>`, callback event/terminal mapping is H-ABI-004C, bounded reentry/queue refusal is H-ABI-004B, and queue/lease/effect closure is checked before dispatch. A row whose host cannot meet its max, affinity, cancellation, accounting, or fence contract is refused before activation; no row silently downgrades to a different mode.

### H-SVC-003 — Constructors, consumers, and lifecycle completeness

The following reachability rules supplement the operation table:

* Every `ResourceRef<T>` in a result has a constructor above or is a bootstrap-supplied typed resource. `VolumeCandidate`, `InputDeviceCandidate`, `AudioDeviceCandidate`, `AdapterCandidate`, `DeviceCandidate`, `ExecutableCandidate`, and `ModelArtifact` are all broker-issued discovery/artifact resources with generation/content identity; value snapshots are obtained through their `*_info` rows.
* `RemoveCursor` is consumed only by `fs.remove_continue`; a report with no cursor has `continuation=none` and no operation may invent one.
* `Watch`, `EventStream`, and `InputStream` are consumed by `watch_next`, `events_next`, and `input.next`; each has sequence/generation/queue state and closes on explicit close, revocation, or terminal source state.
* `Tls` is consumed by `tls.handshake`, `tls.send`, `tls.recv`, and generic lifecycle; `tls.client` creates it from a live `Tcp`. No TLS resource is dead-ended at construction.
* `GpuShader` is consumed by `gpu.pipeline_create`; Texture and TextureView are the sole render-attachment storage mechanism; no parallel target resource exists. `GpuPipeline`, `RenderPass` and `ComputePass` use the complete encoder/pass/submit lifecycle. `CommandEncoder` records commands then finishes into `CommandList`; `GpuFence` supports wait/status and generic lifecycle. `DmaFence` supports status/wait and generic lifecycle.
* `ModelArtifact` is created by an explicit typed artifact import/open path or profile bootstrap; `model.open` validates the pinned format/schema/opset/source identity before creating `Model`. `Model` is consumed by `model.info` and `tensor.dispatch`.
* `Secret` is a nominal resource constructed by `secret.open`; only `secret.read` consumes its bytes. Secret identity and rotation remain the operations-owned `SecretBinding`; no secret bytes enter typed values/receipts.
* Every resource's generic close/query/await IDs are concrete from the resource ordinal table. Ordinary alias/table removal is never broker close. A resource await returns the operations-owned **scoped** QuiescenceProof only when its resource/work scope predicates are proven.

### H-SVC-004 — Proxy, body, and redirect closure

`proxy:Opt<ResourceRef<Proxy>>` is the sole proxy selector in `HttpPolicy`. `Proxy` is generation-bound to one `ResolvedEndpoint`, authority digest, proxy policy digest, optional Secret binding, lease, and owner channel. `proxy.open` checks endpoint scope, proxy mode, credentials, and authority before creation. Each request/redirect carries the exact proxy generation and selected endpoint in its effect receipt. Address classification uses the fixed special-use set in H-SVC-001; unknown ranges reject. DNS rebinding and redirects never switch to a newly authorized private endpoint.

Request body input is a typed `OpaqueBody` resource with content type/digest/length, sealed/exclusive/copy mode, and snapshot charge. The broker reserves input bytes and response/header limits before `Send`; it computes `effective_response_limit=min(request.response_limit,policy.max_body_bytes)` with both fields present and explicit. A body cannot be replayed unless `HttpPolicy.replay` and a rewindable sealed body mode permit it; otherwise redirect requiring replay returns `conflict` before the next send. Response body bytes remain charged and valid only under the `HttpClient`/`HttpResponse` generation. Credentials are stripped on origin change unless exact policy allows; terminal effect axes distinguish accepted request from later body limit/timeout/unknown.

### H-SVC-005 — Scoped quiescence contract

`resource.await_quiescence<T>` requests the operations-owned P-LIFE-001A proof with `scope_kind=1`, exactly the named binding generation. A separate helper/work-unit completion uses scope_kind2 and cannot substitute for binding-wide proof. Neither requires unrelated channel closure or zero unrelated calls. Host outcomes/effects/charges/fence receipts reference their canonical operations bodies. A binding-scoped quiescent_closed result does not prove a physical object reclaimable while other bindings still use it; channel/instance closure requires its own scope0 proof.

### H-SVC-006 — Completion of newly introduced named rows

The following names are used by the registry rows above and are defined here rather than left to host-language inference:

```
CallerContextInfo {
  context_id:Digest256
  origin_principal_id:Digest256
  origin_channel_id:[u8;16]
  origin_instance_generation:Digest256
  deputy_rule_id:u32
  revocation_epoch:u64
  expires_monotonic_ns:u64
  state:u8                 # 00 live, 01 expired, 02 revoked, 03 closed
}
AudioLeaseExtension {
  additional_frames:u64
  additional_duration_ns:u64
  expected_renewal_epoch:u64
}
SignalInfo {
  signal:u32
  portable_kind:u8         # 00 terminate, 01 interrupt, 02 hangup, 03 user, 04 kill, 05 stop
  effect:u8                # 00 advisory, 01 terminating
}
DeviceCandidateInfo {
  device_id:Digest256
  generation:u64
  class:Text<=128
  name:Text<=256
  capabilities:u64
  identity_digest:Digest256
}
NativeCallStatus {
  call_id:u64
  state:u8                 # 00 prepared,01 running,02 completed,03 failed,04 cancelled,05 orphaned
  result_type_id:TypeId
  retained_charge:Opt<Digest256>
}
```

`SignalInfo.portable_kind` and `NativeCallStatus.state` reject unknown values. `ModelArtifactSpec.content_digest` is checked against the sealed buffer length, exact `format_digest`, pinned ONNX schema/opset, external-data policy, and profile maximum before validation. `CallerContextInfo` is returned only for a broker-issued context attachment; a guest cannot manufacture a `CallerContext` by calling this operation.

The `jph.ml@1.0` registry has these typed rows:

* 719 `model.artifact_open`: `(ModelArtifactSpec) -> Result<R<ModelArtifact>>`, `ML.Execute`, `0,5,0,3,Q16K,Q4K,2,1,0`.
* 720 `tensor.write`: `(R<Tensor>,u64 offset,Bytes) -> Result<unit>`, `ML.Transfer`, `1,5,0,3,Q16M,Q4K,2,3,1`.

`model.artifact_open` is the typed constructor for `ModelArtifact`; it requires a sealed buffer, exact content digest, pinned ONNX schema/format/opset identities, sealed-relative external-data ownership, and finite profile bounds. No operation accepts a vague model blob.
 
### H-SVC-007 — Enum and constructor closure for remaining resources

The common/service type tables use these finite enums and masks:

```
Signal: 00 terminate, 01 interrupt, 02 hangup, 03 user, 04 kill, 05 stop
GuiEventMask bits: 0 pointer, 1 key, 2 resize, 3 close, 4 focus, 5 drop
InputMask bits: 0 key, 1 pointer, 2 touch, 3 wheel, 4 gamepad
AudioFormatMask bits: 0 f32, 1 i16, 2 f32_planar
```

Unknown bits reject. `process.signal` takes `Signal`, not an unconstrained u32. `gui.events` takes `GuiEventMask`; `input.subscribe` takes `InputMask`; `AudioDeviceCandidateInfo.formats` and `AudioSpec.format` use the finite audio format values. `AudioSpec.direction` remains 00 output, 01 input, 02 duplex. `DeviceRequest.required_features`, `shader_formats`, and `tensor_formats` are descriptor-bound masks: bits not present in the opened candidate's advertised feature/format masks reject, and a profile must publish the mask meaning in its `device_profile_digest`.

`DomainSpec { parent:Opt<ResourceRef<IsolationDomain>>, authority:List<Right><=256>, budget:BudgetVector, max_tasks:u32, max_handles:u32 }` is the complete input to a broker-created destination domain. `SealedBufferSpec { source:MemorySlice, content_digest:Digest256, immutable:Bool }` and `ExclusiveBufferSpec { source:MemorySlice, lease_duration_ns:u64 }` are the complete inputs to typed buffer constructors; `immutable` must be true for `buffer.seal`, and lease duration must be positive for `buffer.exclusive`.

### H-SVC-008 — Explicit constructors for previously dead-ended resources

The `jph.core@1.0` registry rows include the following typed constructors (all use the registry descriptor tuple grammar):

* 16 `buffer.seal`: `SealedBufferSpec -> Result<R<SealedBuffer>>`, `IO.Read`, `0,1,0,3,Q16K,Q4K,2,1,0`.
* 17 `buffer.exclusive`: `ExclusiveBufferSpec -> Result<R<ExclusiveBuffer>>`, `IO.Read`, `0,1,0,3,Q16K,Q4K,2,1,0`.
* 18 `domain.open`: `DomainSpec -> Result<R<IsolationDomain>>`, `Task.Detach`, `0,4,0,3,Q16K,Q4K,3,1,0`.

The `jph.fs@1.0` registry rows include:

* 121 `fs.open_directory`: `(R<Volume>,RelativePath,ResolveFlags) -> Result<R<Directory>>`, `FS.Open`, `0,4,0,3,Q16K,Q4K,2,1,0`.

The `jph.net@1.0` registry rows include:

* 221 `http.body_stream`: `R<HttpResponse> -> Result<R<BodyStream>>`, `Net.HTTP`, `0,1,0,1,Q4K,Q4K,1,1,0`.

The response resource remains the owner of metadata and creates exactly one body-stream binding at ID 221. `BodyStream` is therefore constructible and consumed, not a dead nominal row. `http.body_read` rejects a response/body generation mismatch or a second body-stream binding as `stale`/`conflict`; EOF closes the body stream only after the final bytes are delivered.
The `jph.gpu@1.0` registry rows include these typed constructors/consumers with the exact registry tuple:
* 719 `gpu.texture_create`: `(R<GpuDevice>,TextureDesc) -> Result<R<Texture>>`, `GPU.Memory`, `0,5,0,3,Q16K,Q4K,2,1,0`.
* 720 `gpu.texture_view`: `(R<Texture>,TextureViewDesc) -> Result<R<TextureView>>`, `GPU.Memory`, `0,5,0,3,Q16K,Q4K,2,1,0`.
* 721 `gpu.sampler_create`: `(R<GpuDevice>,SamplerDesc) -> Result<R<Sampler>>`, `GPU.Render`, `0,5,0,3,Q16K,Q4K,1,1,0`.
* 722 `gpu.bind_group_layout_create`: `(R<GpuDevice>,BindGroupLayoutSpec) -> Result<R<BindGroupLayout>>`, `GPU.Render`, `0,5,0,3,Q16K,Q4K,2,1,0`.
* 723 `gpu.bind_group_create`: `(R<GpuDevice>,BindGroupSpec) -> Result<R<BindGroup>>`, `GPU.Render`, `0,5,0,3,Q16K,Q4K,2,1,0`.
* 724 `gpu.pipeline_layout_create`: `(R<GpuDevice>,PipelineLayoutSpec) -> Result<R<PipelineLayout>>`, `GPU.Render`, `0,5,0,3,Q16K,Q4K,2,1,0`.
* 725 `gpu.encoder_begin`: `R<GpuDevice> -> Result<R<CommandEncoder>>`, `GPU.Render`, `0,5,0,3,Q4K,Q4K,1,1,0`.
* 726 `gpu.encoder_finish`: `R<CommandEncoder> -> Result<R<CommandList>>`, `GPU.Submit`, `0,5,0,1,Q4K,Q4K,1,4,0`
* 727 `gpu.compute_pass_begin`: `(R<GpuDevice>,ComputePassSpec) -> Result<R<ComputePass>>`, `GPU.Render`, `0,5,0,3,Q16K,Q4K,2,2,0`
* 728 `gpu.compute_pass_end`: `R<ComputePass> -> Result<unit>`, `GPU.Render`, `0,5,0,1,Q4K,Q4K,1,4,0`
* 729 `gpu.encoder_record`: `(R<CommandEncoder>,List<Command><=65536) -> Result<unit>`, `GPU.Render`, `0,5,0,3,Q16M,Q4K,2,4,0`

`CallerContext` is a broker-created resource at mode-2 downstream admission or callback/helper propagation; no guest operation constructs it from fields. `AuthorityGrant` is likewise broker-issued only from trusted owner/bootstrap grant admission; no guest operation constructs one from digest fields. `VolumeRoot` and destination `IsolationDomain` may be bootstrap-supplied or created by `domain.open`; `SealedBuffer` and `ExclusiveBuffer` are created by the typed buffer rows. These are explicit construction paths, not implicit defaults.

### H-SVC-009 — Lifecycle and effect constraints for service rows

All operation rows use the following exact preflight order unless their row names a stricter one: authenticated session/channel/interface/function equality; resource TypeId/generation/authority/CallerContext; canonical field/enum/range/relationship checks; snapshot/lease and every ancestor reservation; affinity/reentrancy/cancellation check; effect linearization; terminal Result/effect receipt. No host operation may allocate, read mutable guest bytes, or begin an external effect before the corresponding reservation and ownership check.

Async rows retain their admitted work/attachments through completion, cancellation, close, revocation, or fence according to H-RES-004. Callback rows retain queue/lease/registration until terminal callback state. Device/GPU/audio rows retain buffers/fences/leases through observed completion or an acknowledged reset/fence plus the operations-owned scoped proof. Generic resource close does not cancel unrelated admitted work unless `CloseMode=cancel_then_close` explicitly requests it; a stop request is not proof.
 
### H-SVC-010 — Bootstrap proof and identity closure

The bootstrap uses this bounded tagged `AuthProof/1` record. It is broker-only and never guest authority:

```
AuthProof/1 {
  format:u8 = 01
  proof_kind:u8                 # 00 linux_local_ipc,01 macos_local_ipc,02 windows_local_ipc,
                                # 03 browser_origin_worker,04 embedded_image,05 external_attestor
  algorithm:u8                  # 00 os_peer_credential,01 browser_channel_token,
                                # 02 signed_image,03 attestation_signature
  identity_digest:Digest256
  transport_binding_digest:Digest256
  challenge_digest:Digest256
  issued_monotonic_ns:u64
  expires_monotonic_ns:u64
  subject_digest:Digest256
  proof_material:ProofMaterial
  reserved:u16le = 0
}
ProofMaterial = variant {
  00 LocalPeer { peer_credential_digest:Digest256, endpoint_handle_digest:Digest256 }
  01 Browser { origin_digest:Digest256, worker_digest:Digest256, channel_token_digest:Digest256 }
  02 Embedded { image_digest:Digest256, device_identity_digest:Digest256, provision_digest:Digest256 }
  03 Attestor { attestor_key_digest:Digest256, statement_digest:Digest256, signature:Bytes<=512 }
}
```

The proof kind and material tag must agree; `issued < expires`; proof material is bounded and canonical; a same-user name, URL/PID text, guest digest, or unverified raw handle is not proof. A broker challenge binds exact `transport_binding_digest`, `challenge_digest`, and both nonces. A missing algorithm, expired proof, mismatched identity/transport, unknown variant, or unavailable verifier is activation refusal. `AsciiAName` is an alias for `HostName`'s lower-case RFC 5890 A-label grammar; it is not a second string parser.
<a id="resource-control"></a>
## Resource control and hard termination

### H-RES-001 — Hierarchical reservation and snapshot order

The single ledger hierarchy is `platform -> principal -> isolation-domain -> instance -> channel -> call/task -> host operation/helper/device work`. It accounts guest memory/allocation, snapshot/staging, code/table/cache, stacks, handles, callbacks, queues, CPU, fuel, wall, input/output, descendants, logs/debug snapshots, device memory/queue work, audio stream leases, and retained orphan/quiescence charges. A class that can make a mandatory operation fail is advertised before activation.

Admission order is mandatory: (1) check frame/header length and profile maxima without reading guest bytes; (2) compute checked snapshot length; (3) atomically reserve snapshot bytes at every ancestor; (4) take owned snapshot or verify an already-held sealed/exclusive lease; (5) validate semantics; (6) convert reservation to input charge or release on rejection; (7) reserve operation/helper/device work; (8) execute. No malformed or mutable input can consume unreserved staging. All `used+requested<=limit`, count*stride, and offset+length arithmetic is checked and atomic. Release is idempotent and generation-bound.

Mode-1 output follows the same ledger: reserve staged/output bytes before validation, hold an exclusive output lease through complete commit, or atomically commit broker-owned staging under memory generation and ownership. Partial output is `conflict/interrupted` with effect flags, never successful output. All staged bytes remain charged until commit, close, or failed lease release.

### H-RES-002 — Proposed limits and profile refusal

The proposed beginner defaults remain: guest memory 64 MiB, guest stack 8 MiB, guest/host staging 16 MiB, code/table 32 MiB, one call payload 16 MiB, 128 open bindings, 256 tasks, 256 callback/async entries, 50,000,000 JPM fuel per call, 1 s CPU, 2 s wall per call, 30 s instance wall, 16 MiB input/output per call, zero child descendants, 1 MiB logs, and 4 MiB debug snapshot. JPH/1 hard frame ceiling is 64 MiB; profile admission may be smaller. These are policy numbers, not machine semantics, and do not claim that an embedded host materializes 64 MiB.

A profile may lower a value or request a bounded higher value only when it can measure and enforce it. Fuel bounds deterministic JPM work. CPU is measured scheduler/OS work. Wall is a deadline. Neither interrupts a stuck native call or device kernel. Browser/no-MMU profiles refuse mandatory CPU/kill/device bounds they cannot measure or enforce. Historical plugin defaults remain current plugin law until amendment and are not silently replaced by JPH defaults.

### H-RES-003 — Complete budget vector and attenuation

The reservation vector is:

```
BudgetVector {
  fuel:u64, guest_cpu_ns:u64, host_cpu_ns:u64, wall_ns:u64,
  memory_bytes:u64, stack_bytes:u64, snapshot_bytes:u64, input_bytes:u64,
  output_bytes:u64, queue_entries:u32, handle_count:u32, task_count:u32,
  descendant_count:u32, device_bytes:u64, device_work:u64, log_bytes:u64,
  callback_bytes:u64
}
```

JPM instruction and bounded bulk work consume `fuel`. Imported calls consume fixed admission fuel plus measured host CPU/wall and copied input/output. Callback delivery consumes callback bytes/CPU. Native process/task/device work consumes its own measured/accounted dimensions; it is not falsely represented as deterministic fuel. Every child task/process/device submission satisfies `child_vector <= remaining(parent, domain, profile)` in every dimension before enqueue. A detached destination receives an independently reserved attenuated vector. If a profile cannot measure/terminate a dimension, the operation/profile is refused rather than promising a bound.

The scheduler charges guest execution, imported calls, callbacks, queues, helpers, and descendants under their owner domain. A bulk operation declares maximum non-preemptible quantum. Callback queues are reserved before enqueue. Local fairness weights do not bypass rights or ancestor limits. Deterministic replay requires recorded schedule and host-input trace; no deterministic I/O/clock/random/device guarantee is implied.

### H-RES-004 — Async work, cancellation, and hard termination

Every asynchronous submission creates one admitted work unit with finite bytes, work, deadline, owner generation, and cancellation boundary. The unit retains queue/device/resource reservations until observed `completed`/exit or an acknowledged reset/fence with the operations-owned P-LIFE-001A `QuiescenceProof`. A device-loss notification and generation invalidation only reject future admissions; they do not release charges, storage, device ranges, or permit generation reuse. `fence_wait` timeout cancels only the wait. Closing a buffer/device/task/audio stream does not free storage or permit generation reuse while work may still read/write it.

Cancellation records caller-response, guest-stop, and helper-reclaim deadlines separately. It drains queued unstarted work, returns typed errors, and invokes service hooks. At the grace deadline, a process/job/cgroup/queue reset is used only when the profile declared it killable/fenceable. A stop request is not proof. Until exit or fence, the helper/work remains charged, quarantined, and retains device ranges, writer locks, callbacks, secrets, and generations. If no backend can establish quiescence, the profile must have refused the operation before submission.

Hard traps terminate the authority-isolated instance and guest tasks; ordinary denied/not-found/exhausted-before-effect/busy/cancelled results leave a sound instance runnable. Cancellation never retracts sent packets, committed writes, delivered signals, disclosed secret bytes, or submitted device commands. The error flags/receipt state exactly expose `not_started`, `accepted`, `completed`, or `unknown`, plus not-retractable, partial, retry, and awaiting-quiescence.

### H-RES-005 — Cleanup, orphan accounting, secrets, and close invariants

Channel close closes its queue, callbacks, aliases, helper descendants, staging leases, and debugger views, but underlying state remains charged through quiescence. A completion after close is not delivered to the guest; it remains in the canonical receipt with call ID, binding generation, effect axes, and residual work. The broker never reuses a channel/object generation while an old helper could complete.

Secret reads use `SecretBinding` and a one-way disclosure transition. Revocation blocks future reads; it cannot erase guest-observed bytes. Secret values are redacted from errors, logs, callbacks, receipts, crash dumps, and debugger snapshots unless a separately granted debug-secret right exists. A disclosed secret's effect view is `completed + not_retractable`; query returns metadata only. Backup/restore and migration code must use the operations-owned BackupRecord/fence contract and cannot copy a live secret or restore a resource generation without a fresh authority/bootstrap decision. Host service receipts identify pending quiescence rather than claiming close/destruction.

<a id="isolation"></a>
## Isolation, trust, and residual risk

### H-ISP-001 — Trust classes, TCB, and domains

For a hostile portable instance, the TCB is the bounded JPM parser/verifier, reference/equivalence-checked engine, JPH decoder and resource table, Authority evaluator, selected OS/VM/process boundary, and any driver/validator/device TCB named by the profile. Package signatures, compiler provenance, caches, and provider metadata are policy evidence, not replacements for these components. Each mutually distrustful principal receives a separate authority/isolation domain; same-domain sharing is allowed only when principal, generation, and sharing grant match. A broker denial does not prove kernel containment.

### H-ISP-002 — Immutable/sealed and exclusive zero-copy

Zero-copy is valid only for bytes sealed immutable for the entire validation/use/callback/completion lifetime or for an exclusive transferable lease preventing guest mutation, relocation, deallocation, and re-entry. Otherwise the broker snapshots and charges bytes. No borrowed guest address survives suspension/reentry. Mutable lease cancellation/trap may leave partial receiver mutation and is not transaction rollback. Output mode 1 specifically requires an exclusive output lease or atomic broker-staged commit; revalidation alone is not safe.

### H-ISP-003 — Threat and residual boundaries

The proposal addresses malicious portable bytes, stale/forged bindings, confused deputies, dependency/provider authority, denial of service, callback reentrancy, browser origin confusion, device command validation, and secret/receipt disclosure with the parser, typed broker, session equality, ledger, queue caps, profile refusal, and redaction rules above. It does not claim resistance to a compromised kernel, administrator, hypervisor, CPU, DMA device, browser implementation, driver, or hardware. Cache/timing/power/network/device side channels remain residual. Safe Jet does not claim general deadlock freedom or retroactive secret revocation.

### H-ISP-004 — Callable FFI boundary

The FFI descriptors and operations above are the only callable native transition. Trusted in-process descriptors are owner-gated and unavailable to hostile profiles. Isolated helper calls accept only registered typed FunctionRef/Args/ResultValue, have explicit kill/accounting backend enums, callback queue ownership, finite stack/call/budget fields, and retain charges until exit/fence. Raw pointers, native addresses, arbitrary unwind objects, managed identities, JNI/NIF pointers, and V8 external pointers are forbidden. A missing enum/schema/owner/revocation/quiescence path is a pre-activation refusal.

### H-ISP-005 — Bounds, executable permissions, and speculation

JPM checks bounds/control-flow independent of guard pages or hardware faults. Generated code uses W^X/equivalent policy; no self-modifying guest code, writable executable page, computed native jump, or pointer fabrication exists. A host may select branch/cache mitigations but must record the selected mitigation profile. No categorical side-channel elimination is claimed.

### H-ISP-006 — Traps and semantic ownership

Hard containment traps terminate the affected isolated instance; they do not turn partial transfer into a valid result or resume an interrupted shared heap. Ordinary operation errors are JPH typed results. Source/import meaning remains sema-owned; JPH/JPM validation checks portable bytes and host safety but does not invent source diagnostics. No-MMU verified software containment has an explicit narrower TCB and is not a pass for hostile native/process isolation.

<a id="package-graph"></a>
## Package graph

### P-BASE-001 — One graph and one order

The existing package/lock/Hangar/receipt graph is the sole authority for source, artifacts, selections, actions, trust, authority decisions, generations, volumes, secrets, deployment, and receipts. A foreign lock, OCI tag, registry index, process PID, path, cache filename, or host status string is a projection or input fact, never a second authority.

Identity construction has this acyclic order:

1. Immutable source-tree, artifact-byte, manifest, interface, target-profile, trust-subject, and selection facts are captured first.
2. Action request bodies are constructed from those facts and exact policy/authority inputs. Action IDs are computed without action outputs or receipt references.
3. Action result receipts are constructed from the action ID and observed outputs/effects. Receipt IDs omit their own ID and publication-back references.
4. A lock body is constructed from node, selection, artifact, action-result, source, policy, and writer facts. A lock digest is computed without the graph digest or lock publication receipt.
5. A graph body is constructed from the lock digest, graph nodes/edges/actions/artifacts/authority/profile facts, and optional predecessor graph. A graph digest is computed without the graph ID or publication receipt.
6. A publication receipt is constructed after lock and graph digests exist. It may name the old/new pointers and journal proof but is not a preimage of either target.
7. A generation activation receipt is constructed only after the generation, readiness, route, and host-fence facts exist.

A reverse reference is a graph view. It does not become a hash preimage merely because a record exposes it. `graph_digest`, `lock_digest`, `receipt`, `publication_receipt`, `active_pointer`, and reverse edge indexes are derived links when the body projection below says so.
`P-BASE-001` also owns the canonical `OperationOutcomeRef` carried by graph receipts. Its single complete body (`EffectView`, `RetainedCharge`, `effect_receipt`, and host flag encoding) is defined once in the repaired logical schema below under `P-GRAPH-001`; host rows reference this type by name and digest and do not duplicate it.

### P-BASE-002 — JPK-C14N/1 wire primitive

All proposed graph IDs use `JPK-C14N/1`; no implementation may substitute a generic serializer, map order, language debug formatter, or unspecified JSON canonicalization.

The preimage byte grammar is:

```text
u8       = one octet
u16le    = unsigned 16-bit integer, little-endian
u32le    = unsigned 32-bit integer, little-endian
u64le    = unsigned 64-bit integer, little-endian
i64le    = signed two's-complement 64-bit integer, little-endian
Bytes(x)  = u32le(byte_length(x)) || x
Text(s)   = Bytes(UTF8(s)); UTF-8 is required and no Unicode normalization is applied
Bool(b)   = 0x00 for false, 0x01 for true
Null      = 0x00
Some(x)   = 0x01 || x
Digest(d) = 0x01 || 32 raw SHA-256 bytes for sha256:d; no textual prefix is used in a preimage
Time(t)   = i64le(unix_nanoseconds_UTC(t))
Duration  = u64le(nanoseconds)
Count(n)  = u32le(n)
```

`Record(tag, version, fields)` is `Bytes(ASCII(tag)) || u16le(version) || u16le(field_count) || field_1 || ... || field_n`. Every field is emitted in the fixed order in its record definition and is encoded with its declared primitive or nested record type. A set-sorted array is `Count(n)` followed by items sorted by their complete encoded item bytes; an ordered array preserves the declared order. A map is never encoded as a map: its schema supplies a fixed key/value record and an explicit sort key. Values outside the schema, duplicate keys, non-canonical booleans, invalid UTF-8, integer overflow, or trailing bytes are rejected.

For every identity, `H(tag, body) = SHA256(Bytes(ASCII(tag)) || 0x00 || body)`. The public `Digest` projection is `sha256:` followed by 64 lowercase hexadecimal characters. Textual `node:`, `action:`, `receipt:`, `generation:`, and `backup:` IDs are labels over a digest; labels are not hashed as alternate identity bytes.

A one-field mutation changes the identity whose projection includes that field. A field explicitly listed as excluded changes the result receipt or publication receipt instead. Implementations MUST expose the projection name and field coverage in `See`; they MUST NOT infer coverage from a serializer.

### P-ID-001 — Graph and lock body projections

The exact body projections are:

```text
NodeBody/1 fields, in order:
  name, version, reference.raw, reference.source_alias, reference.channel,
  manifest_digest, source, interface, semantic_bundle_digest, requested_effects(sorted),
  requested_authority(sorted), outputs(sorted), platform_constraints(sorted),
  license_refs(sorted), provenance_refs(sorted)
Excluded: node_id, reverse edges, graph/lock IDs, publication receipts.

ActionBody/1 fields, in order:
  kind, input_digests(ordered), authority_digest, toolchain_digest,
  target_digest, policy_digest, idempotency_tuple_digest, override_digest
Excluded: action_id, output_digests, result receipt, journal timestamps.

ReceiptBody/1 fields, in order:
  action, parent_receipts(sorted), exact_inputs(ordered), semantic_bundle_digest,
  planned_authority, effective_authority, planned_outputs(ordered), observed_outputs(ordered),
  operation_outcome, activation, rollback, journal(ordered), status, failure,
  signer_refs(sorted), provenance_refs(sorted), cache_identity_digests(sorted)
Excluded: receipt_id, publication targets, active pointers, and reverse indexes.

LockBody/1 fields, in order:
  format, nodes(ordered), platforms(ordered), trust_policy_digest,
  authority_policy_digest, trust_snapshot_digest, toolchains(ordered), semantic_bundle_digests(sorted),
  owned_source_states(ordered), generated_at, writer
Excluded: graph_digest, lock_digest, publication receipt, active-generation pointer.

GraphBody/1 fields, in order:
  format, root, manifest_digest, nodes(ordered), edges(ordered), actions(ordered),
  artifacts(ordered), authorities(ordered), target_profiles(ordered), lock_digest,
  parent_graph, created_by, selection_overrides(ordered)
Excluded: graph_id, publication receipt, active-generation pointer.

PublicationBody/1 fields, in order:
  transaction_id, expected_lock_digest, expected_graph_digest,
  expected_generation_pointer, expected_source_root_digest,
  expected_source_root_identity, expected_source_root_epoch, old_lock_digest,
  new_lock_digest, old_graph_digest, new_graph_digest, old_generation_pointer,
  new_generation_pointer, old_source_root_digest, new_source_root_digest,
  old_source_root_identity, new_source_root_identity, old_source_root_epoch,
  new_source_root_epoch, journal_proof_digest, outcome
This is computed after its targets exist and is never inserted into their bodies.
```

`NodeId = node:H(NodeBody/1)`, `ActionId = action:H(ActionBody/1)`, `ReceiptId = receipt:H(ReceiptBody/1)`, `LockDigest = H(LockBody/1)`, `GraphDigest = H(GraphBody/1)`, and `PublicationReceiptId = receipt:H(PublicationBody/1)`. `GenerationId` and `BackupId` use the same rules in their lifecycle definitions. The order is a contract, not an implementation suggestion.

Identity dependency is typed and acyclic. Primitive/source digests may feed `NodeBody`, `ActionBody`, `AuthorityBody/1`, `TargetProfileBody/1`, and `TrustSnapshotBody/1`; those bodies may feed artifacts, locks, graphs, and generations; receipts and publication receipts may name earlier identities but never occur in the preimage of an identity they can themselves reference. Every nested array in an identity body carries the child's typed digest or a named immutable body projection, never a full lifecycle record with its derived ID, mutable state, journal, or reverse receipt. A verifier rejects a digest whose namespace is not permitted by this order and rejects self, later-phase, or reverse references.

```text
AuthorityBody/1 {
  parent_digest: Digest | null,
  requested: [AuthorityRequest] sorted,
  effective: [AuthorityGrantBody/1] sorted,
  application_authority_digest: Digest,
  host_import_decision_digest: Digest,
  revocation_epoch: u64
}
AuthorityGrantBody/1 { right:String, scope:String|null, source:"host-policy" | "interactive-policy" | "accepted-delegation" }
TargetProfileBody/1 {
  os:String, architecture:String, abi:String,
  engine_tiers:["reference" | "jit" | "aot"] sorted,
  jph_interface_digests:[Digest] sorted,
  feature_digest:Digest, numeric_semantics_digest:Digest,
  resource_limits_digest:Digest, containment_digest:Digest,
  native_execution:"denied" | "trusted-explicit" | "allowed",
  device_profile_digest:Digest|null
}
GenerationBody/1 {
  graph_digest:Digest, lock_digest:Digest, artifacts:[Digest] sorted,
  services:[String] sorted, volume_binding_digests:[Digest] sorted,
  resource_attachment_digests:[Digest] sorted,
  secret_binding_digests:[Digest] sorted, parent:GenerationId|null,
  authority_snapshot_digest:Digest, candidate_pointer_digest:Digest|null
}
TrustSnapshotBody/1 {
  root_digest:Digest, root_version:u64, metadata_set_digest:Digest,
  coverage_manifest_digest:Digest,
  revocation_epoch:u64, revocation_completeness:"complete" | "partial" | "unknown",
  known_empty_revocations:Boolean, subject_digests:[Digest] sorted,
  issued_at:Timestamp, expires_at:Timestamp,
  freshness_policy_digest:Digest, signed_statement:Digest
}
```
`TrustSnapshot.snapshot_id = H(TrustSnapshotBody/1)`; its `lock_digest` and `graph_digest` fields are admission-view bindings, excluded from that signed body so the snapshot does not depend on a lock that references the snapshot. The lock/graph admission record compares those view bindings against the required subject set and `coverage_manifest_digest`.

`ReceiptBody` nested `operation_outcome` names an earlier host effect receipt body (never the containing `receipt_id`), while activation/rollback/journal facts carry only their declared immutable pointer/body digests and earlier receipt IDs. `failure.outcome`, journal input digests, signer/provenance refs, and cache identities may reference only already sealed inputs; a receipt cannot be in its own `exact_inputs`, operation outcome, or reverse publication preimage. The graph/lock body arrays use named immutable projections: `LockBody.nodes` encodes `LockNodeBody/1` (node ID, name, manifest/source/selection/semantic-bundle/output digests) and excludes action/verification receipt fields; `LockBody.platforms` encodes `TargetProfileBody/1`; toolchain rows encode `{name,version,digest}`. `GraphBody.actions` encodes `ActionBody/1`, authorities encode `AuthorityBody/1`, target profiles encode `TargetProfileBody/1`, artifacts encode their immutable artifact bodies, and selection overrides encode `SelectionOverrideBody/1`. The corresponding `LockRecord`/`GraphRecord` views may join receipt/status/pointer fields after the body digest is sealed, but those views are never substituted into the body preimage.

```text
LockNodeBody/1 {
  node_id: NodeId, name: String, manifest_digest: Digest,
  source: SourceRef, semantic_bundle_digest: Digest | null,
  selected: Selection, outputs: [Digest] sorted
}
SelectionOverrideBody/1 {
  source: "cli" | "project-config" | "lock" | "host-policy" | "profile-default",
  key: String, requested_value: String,
  expected_parent_digest: Digest, precedence_rank: u8
}
OverrideDigest = H("SelectionOverrideBody/1", SelectionOverrideBody/1)
```

`AuthorityRecord.host_import_decision_receipt` is an external decision fact and is not in `AuthorityBody/1`; its immutable `host_import_decision_digest` is. This prevents the authority/receipt cycle while requiring the receipt to state and verify the same decision digest. `GenerationId`, `TargetProfile`/profile digest, and `TrustSnapshot.snapshot_id` are hashes of their corresponding body projections; mutable state and receipt pointers remain lifecycle facts.

### P-GRAPH-001 — Complete repaired logical schemas

The following are complete proposed logical records. Required fields are always present. Optional values use their declared `null`/tagged value; absence and empty are not interchangeable.

```text
Digest       = "sha256:" + 64 lowercase hexadecimal characters
NodeId       = "node:" + Digest
ActionId     = "action:" + Digest
ReceiptId    = "receipt:" + Digest
GenerationId = "generation:" + Digest
BackupId     = "backup:" + Digest

GraphRecord {
  format: "JPKG/1",
  graph_id: Digest,
  root: NodeId,
  manifest_digest: Digest,
  nodes: [NodeRecord],
  edges: [EdgeRecord],
  actions: [ActionRecord],
  artifacts: [ArtifactRecord],
  authorities: [AuthorityRecord],
  target_profiles: [TargetProfileRecord],
  lock_digest: Digest,
  parent_graph: Digest | null,
  created_by: { tool_digest: Digest, invocation_digest: Digest },
  selection_overrides: [SelectionOverrideRecord],
  publication_receipt: ReceiptId | null
}

SemanticBundle {
  semantic_bundle_format: u8 = 01,
  prelude_corelib_digest: Digest,
  numeric_semantics_digest: Digest,
  mir_schema: u32 = 3,
  required_module_digests: [Digest] sorted by encoded digest bytes,
  required_interface_digests: [Digest] sorted by encoded digest bytes
}

SemanticBundleDigest = H("SemanticBundleBody/1", SemanticBundleBody/1)

SemanticBundleBody/1 fields, in order:
  semantic_bundle_format:u8,
  prelude_corelib_digest:Digest,
  numeric_semantics_digest:Digest,
  mir_schema:u32le,
  required_module_digests(sorted),
  required_interface_digests(sorted)

The digest preimage uses the fixed JPK-C14N/1 `Record`/`Count`/`Digest` encodings above. Counts are emitted even for an empty list; duplicate digests, a format other than `01`, a MIR schema other than `3`, or unsorted arrays reject. `semantic_bundle_digest` is derived from the body and is not included in its own preimage. A `SemanticBundle` object is stored once in the graph/Hangar object store; source-bound records reference its digest and validate the complete object before admission.

NodeRecord {
  node_id: NodeId,
  name: String,
  version: String,
  reference: { raw: String, source_alias: String | null, channel: String | null },
  manifest_digest: Digest,
  source: SourceRef,
  interface: { imports: [InterfaceRef], exports: [InterfaceRef], world_digest: Digest },
  semantic_bundle_digest: Digest | null,
  requested_effects: [String],
  requested_authority: [AuthorityRequest],
  outputs: [Digest],
  platform_constraints: [Constraint],
  license_refs: [Digest],
  provenance_refs: [Digest]
}

EdgeRecord {
  from: NodeId,
  to: NodeId,
  kind: "runtime" | "build" | "dev" | "optional" | "transitive-source" | "toolchain",
  requested_range: String,
  selected_version: String,
  interface_digest: Digest,
  condition: Condition | null
}

InterfaceRef {
  interface_digest: Digest,
  signature_digest: Digest,
  direction: "import" | "export",
  version: String,
  required: Boolean
}

Constraint { key: String, operator: "equals" | "at-least" | "includes" | "excludes", value: String }
Condition { kind: "target" | "feature" | "profile" | "optional", expression: String, result: Boolean }

SourceRef {
  kind: "workspace" | "owned" | "path" | "git" | "registry" | "foreign" | "binary",
  locator: String,
  tree_digest: Digest,
  manifest_digest: Digest,
  upstream: UpstreamRef | null,
  license_digest: Digest,
  dirty: Boolean
}
UpstreamRef {
  locator: String,
  revision: String,
  tree_digest: Digest,
  manifest_digest: Digest,
  license_digest: Digest
}
OwnedSourceRef {
  node_id: NodeId,
  source_record_digest: Digest,
  root_path: String,
  root_identity: Digest,
  root_epoch: u64,
  base_tree_digest: Digest,
  local_tree_digest: Digest,
  upstream_tree_digest: Digest | null,
  ownership: "project" | "workspace" | "transitive-owned",
  dirty: Boolean,
  last_merge_receipt: ReceiptId | null
}

JournalFact {
  transaction_id: String,
  point: String,
  time: Timestamp,
  input_digests: [Digest],
  state: String,
  detail_digest: Digest | null
}

FailureFact {
  category: String,
  stable_detail: String,
  retry: "never" | "same-key" | "after-refresh" | "after-reconcile" | "user-choice",
  residual_effects: [String],
  outcome: OperationOutcomeRef | null
}

AuthoritySnapshot/1 {
  application_authority_digest: Digest,
  authority_digest: Digest,
  revocation_epoch: u64,
  host_import_decision_receipt: ReceiptId
}
ActivationPrepareBody/1 {
  transaction_id: String,
  generation: GenerationId,
  authority_snapshot_digest: Digest,
  target_profile_digest: Digest,
  expected_old_pointer: Digest | null,
  expected_old_route: Digest | null
}
ActivationPrepare/1 {
  prepare_digest: Digest,
  body: ActivationPrepareBody/1,
  receipt: ReceiptId
}
`prepare_digest = H("ActivationPrepareBody/1", body)`; `receipt` is a lifecycle view and is excluded from the body preimage. The prepare receipt MUST bind the same `authority_snapshot_digest` and expected-old values.

ActivationFact {
  generation: GenerationId,
  pointer_digest: Digest,
  route_digest: Digest,
  host_id: String,
  authority_snapshot_digest: Digest,
  expected_old_pointer: Digest | null,
  linearized: Boolean,
  readiness_digest: Digest | null
}

RollbackFact {
  candidate: GenerationId,
  data_compatibility: String,
  external_effects_undone: Boolean,
  reason_digest: Digest,
  backup_id: BackupId | null
}

TargetProfileRecord {
  profile_digest: Digest,
  os: String,
  architecture: String,
  abi: String,
  engine_tiers: ["reference" | "jit" | "aot"],
  jph_interface_digests: [Digest],
  feature_digest: Digest,
  numeric_semantics_digest: Digest,
  resource_limits_digest: Digest,
  containment_digest: Digest,
  native_execution: "denied" | "trusted-explicit" | "allowed",
  device_profile_digest: Digest | null
}

AuthorityRecord {
  authority_digest: Digest,
  parent_digest: Digest | null,
  requested: [AuthorityRequest],
  effective: [AuthorityGrant],
  application_authority_digest: Digest,
  host_import_decision_digest: Digest,
  host_import_decision_receipt: ReceiptId,
  revocation_epoch: u64
}
AuthorityRequest { right: String, scope: String | null, reason: String, source: "manifest" | "workspace" | "semantic" }
AuthorityGrant {
  right: String,
  scope: String | null,
  source: "host-policy" | "interactive-policy" | "accepted-delegation",
  decision_receipt: ReceiptId
}

Selection {
  raw_reference: String,
  selected_version: String,
  channel_snapshot_digest: Digest | null,
  source_digest: Digest,
  artifact_digest: Digest | null
}
SelectionOverrideRecord {
  source: "cli" | "project-config" | "lock" | "host-policy" | "profile-default",
  key: String,
  requested_value: String,
  expected_parent_digest: Digest,
  precedence_rank: u8,          # derived closed rank: host=0, lock=1, user=2, project=3, profile=4
  override_digest: Digest,
  result: "selected" | "rejected" | "conflict",
  action: ActionId,
  receipt: ReceiptId
}

ArtifactRecord {
  artifact_digest: Digest,
  kind: "jpm" | "native" | "toolchain" | "device" | "foreign-runtime" | "oci-projection",
  producer_action: ActionId,
  target: TargetProfileRecord | null,
  jpm_digest: Digest | null,
  jph_digest: Digest | null,
  jph_interface_digests: [Digest],
  semantic_bundle_digest: Digest | null,
  feature_profile_digest: Digest | null,
  native_cache: Digest | null,
  trust_state: "local" | "verified" | "quarantined" | "revoked" | "unknown",
  cache_state: "absent" | "present" | "corrupt" | "stale",
  provenance_refs: [Digest]
}

ActionBody {
  kind: "resolve" | "fetch" | "merge" | "compile" | "link" | "admit" |
        "activate" | "migrate" | "rollout" | "backup" | "quarantine" | "reconcile",
  input_digests: [Digest],
  authority_digest: Digest,
  toolchain_digest: Digest | null,
  target_digest: Digest | null,
  policy_digest: Digest,
  idempotency_tuple_digest: Digest,
  override_digest: Digest | null
}
ActionRecord { action_id: ActionId, body: ActionBody, result_receipt: ReceiptId | null }

LockNode {
  node_id: NodeId,
  name: String,
  manifest_digest: Digest,
  source: SourceRef,
  semantic_bundle_digest: Digest | null,
  selected: Selection,
  outputs: [Digest],
  action_receipts: [ReceiptId],
  receipt: ReceiptId
}
LockRecord {
  format: "JPK-LOCK/1",
  graph_digest: Digest,
  nodes: [LockNode],
  platforms: [TargetProfileRecord],
  trust_policy_digest: Digest,
  authority_policy_digest: Digest,
  trust_snapshot_digest: Digest,
  toolchains: [{ name: String, version: String, digest: Digest }],
  semantic_bundle_digests: [Digest] sorted and duplicate-free,
  owned_source_states: [OwnedSourceRef],
  generated_at: Timestamp,
  writer: { tool_digest: Digest, invocation_digest: Digest },
  publication_receipt: ReceiptId | null
}

EffectView {
  phase: "not_started" | "accepted" | "completed" | "unknown",
  retractability: "retractable" | "not_retractable",
  quiescence: "quiescent" | "awaiting_quiescence",
  partial: "none" | "partial",
  retry: "safe" | "unsafe"
}
RetainedCharge {
  retained_bytes: u64,
  retained_work: u64,
  residual_duration_ns: u64,
  owner_generation: u64,
  fence_required: Boolean
}
OperationOutcomeRef {
  host_contract: "HOST-LP-019",
  effect_receipt: ReceiptId,
  effect_view: EffectView,
  retained_charge: RetainedCharge | null,
  caller_context_digest: Digest | null,
  error_flags: u32
}
ReceiptRecord {
  format: "JPK-RECEIPT/1",
  receipt_id: ReceiptId,
  action: ActionId,
  parent_receipts: [ReceiptId],
  exact_inputs: [Digest],
  semantic_bundle_digest: Digest | null,
  planned_authority: Digest,
  effective_authority: Digest,
  planned_outputs: [Digest],
  observed_outputs: [Digest],
  operation_outcome: OperationOutcomeRef | null,
  activation: ActivationFact | null,
  rollback: RollbackFact | null,
  journal: [JournalFact],
  status: "prepared" | "committed" | "failed" | "recovered" | "quarantined" | "conflict" | "unknown",
  failure: FailureFact | null,
  signer_refs: [Digest],
  provenance_refs: [Digest],
  cache_identity_digests: [Digest],
  caller_context_digest: Digest | null
}
```

The host owns the meaning, flag encoding, and per-operation admissibility of `EffectView` and `RetainedCharge`. `error_flags` uses the host mapping: bits 0..1 encode phase, bit 2 `not_retractable`, bit 3 `awaiting_quiescence`, bit 4 `partial`, bit 5 `retry_safe`, bit 6 receipt present; bits 7..31 are zero and rejected otherwise. The operations receipt carries these values without deriving an alternate status from human detail text.
**M-EFFECT-SCHEMA-002.** `P-GRAPH-001` remains the only logical `EffectView`/`RetainedCharge` body. The machine wire projection is:

```text
EffectViewWire/1 {
  phase:u8             # 00 not_started, 01 accepted, 02 completed, 03 unknown
  retractability:u8    # 00 retractable, 01 not_retractable
  quiescence:u8        # 00 quiescent, 01 awaiting_quiescence
  partial:u8           # 00 none, 01 partial
  retry:u8             # 00 safe, 01 unsafe
  reserved:[u8;3] = 00
}
```

The conversion from logical string enums to these bytes is total; unknown logical values or nonzero reserved bytes reject. `EffectViewWire/1` is not hashed as a second body. `OperationOutcomeRef.effect_view` hashes/serializes the logical owner body, while an H-ABI Error carries only the wire flags. `RetainedCharge` remains the exact P-BASE-001 projection with fields `retained_bytes:u64, retained_work:u64, residual_duration_ns:u64, owner_generation:u64, fence_required:Bool`; no host or machine chapter redeclares it.

### P-GRAPH-002 — Graph invariants

`P-GRAPH-001` is repaired as follows: a lock is reconstructible from immutable digests, exact selections, target records, authority-policy identity, toolchain records, owned-source records, and retained receipts. Moving names/channels/paths are selection inputs only. A missing required field, unbound digest, or mutable path produces an incomplete-lock refusal.

`P-GRAPH-002` is repaired: every executable or host-affecting output has an `ArtifactRecord`, producer action, target/trust state, and result receipt. Native, AOT-cache, device, foreign-runtime, OCI, and JPM outputs cannot share an identity merely because their package name matches. For every source-bound node, lock, artifact, verification/admission receipt, and source-producing action, `semantic_bundle_digest` (or the lock's sorted `semantic_bundle_digests`) MUST resolve to one complete `SemanticBundle` object above; the verifier recomputes its `SemanticBundleDigest` and rejects a missing, partial, mismatched, duplicated, or stale object before cache, link, or activation admission. No record may repeat only Prelude/CoreLib, MIR, numeric, module, or interface fields as a substitute for that reference.

`P-GRAPH-004` is repaired: requested authority is copied from the graph to the host decision request. Only the current `ApplicationAuthority` decision, exact `application_authority_digest`, exact `host_import_decision_receipt`, and `revocation_epoch` populate `effective`. The graph never treats a manifest, signature, source ownership, or `AuthorityRequest` as a grant.

### P-CAS-001 — Expected-old publication

Every lock, graph, source-root, and generation publication transaction carries:

```text
PublicationExpectation {
  expected_lock_digest: Digest | null,
  expected_graph_digest: Digest | null,
  expected_generation_pointer: Digest | null,
  expected_source_root_digest: Digest | null,
  expected_source_root_identity: Digest | null,
  expected_source_root_epoch: u64 | null,
  transaction_id: String
}
```

At the local graph journal linearization point, the writer obtains the existing graph/lock ownership lease, re-reads all expected pointers through no-follow handles, and compares exact digests and transaction parent. A mismatch in any expected digest, root identity, non-wrapping root epoch, or transaction parent returns `conflict` with the staged lock/graph/source/generation objects and a receipt; it is never a successful last-writer-wins update. The lease is local serialization, not a distributed exactly-once claim. The source write/modify lease MUST exclude writes through the committed root identity; an advisory lock or an already-open editor descriptor is not exclusion.

Atomic file replacement remains useful to prevent partial bytes, but it is not CAS. Existing lock/Hangar/storage ownership is retained. The proposed CAS is a new transaction predicate around that owner, not a second lock file or storage authority.

### P-CAS-002 — Journal proof and recovery

`SourcePointerTransitionBody/1` is the immutable pointer evidence used by the publication receipt:

```text
SourcePointerTransitionBody/1 {
  transaction_id: String,
  old_root_identity: Digest | null,
  old_root_epoch: u64 | null,
  new_root_identity: Digest | null,
  new_root_epoch: u64 | null,
  expected_old_root_identity: Digest | null,
  expected_old_root_epoch: u64 | null
}
SourcePointerTransition/1 {
  transition_digest: Digest,
  body: SourcePointerTransitionBody/1,
  pointer_cas: "not_attempted" | "won" | "lost" | "unknown",
  recovery: "none" | "restored_old" | "published_new" | "quarantined",
  backend_receipt: ReceiptId | null
}
```

`transition_digest = H("SourcePointerTransitionBody/1", body)`; CAS/recovery state and receipts are views, not body inputs.

`PublicationBody.outcome` is one of `"committed" | "conflict" | "unknown" | "recovered_old" | "recovered_new" | "quarantined"`; an unrecognized outcome is malformed. A publication writes, in order, `tx.begin`, `inputs.bound`, `objects.sealed`, `lock.staged`, `graph.staged`, `source.staged` where applicable, `cas.checked`, `publish.prepared`, `lock.published`, `graph.published`, `source.published`, `generation.published`, `publish.committed`, and a publication receipt. The journal record contains expected old digests, new digests, transaction ID, and a digest over the complete ordered journal.

Recovery accepts a new state only when every required new object, pointer, and journal marker has durable matching digests for the same transaction. If a marker is missing, a pointer has the wrong old/new digest, or a current source edit does not match the recorded root, recovery retains the old visible lock/root/generation, quarantines the staged candidate, and records `conflict` or `unknown`. It never pairs an arbitrary directory with a lock.

### P-CAS-003 — Resolution and offline behavior

Resolver steps are: parse current `package.jet` and lock; capture source/artifact/selection facts; apply fixed override precedence; verify trust and profile; build graph; create actions; obtain action result receipts; compute lock; CAS-publish lock/graph. A failure before CAS leaves active roots unchanged.

Offline resolution may use only project source, owned source, Hangar objects, sealed caches, lock records, and signed trust snapshots that satisfy the trust rules below. Missing bytes, missing exact toolchain, incomplete revocation evidence, or an expired snapshot under the selected mode refuses before mutation. Network order and filesystem order never break a tie.

### Native cache identity and admission

### P-CACHE-001 — One canonical NativeCacheKey

The same complete `NativeCacheKey/2` is used by execution-tier admission, the package graph, and the machine-tier cache verifier. It is one fixed tuple, not a catch-all digest and not a second machine-local schema:

```text
CacheFuelIdentity {
  fuel_schedule_version: u32,
  fuel_schedule_digest: Digest,
  imported_call_charge_digest: Digest,
  input_codec_charge_digest: Digest,
  output_codec_charge_digest: Digest,
  host_effect_charge_policy_digest: Digest
}

NativeCacheKey/2 {
  jpm_digest: Digest,
  jpm_format_version: (u8,u8),
  jph_abi_version: (u8,u8),
  jph_interface_digests: [Digest] sorted by encoded digest bytes,
  semantic_bundle_digest: Digest | null,
  fuel: CacheFuelIdentity,
  provider_implementation_digests: [Digest] sorted by encoded digest bytes,
  shim_implementation_digests: [Digest] sorted by encoded digest bytes,
  foreign_runtime: {
    family: "none" | "jvm" | "clr" | "v8" | "beam" | "other",
    runtime_version: String | null,
    runtime_build_digest: Digest | null,
    adapter_digest: Digest | null
  },
  import_export_digest: Digest,
  engine_build_digest: Digest,
  compiler_build_digest: Digest,
  target_os: String,
  target_architecture: String,
  target_abi: String,
  target_feature_digest: Digest,
  optimizer_flags_digest: Digest,
  mitigation_flags_digest: Digest,
  debug_deopt_schema_digest: Digest,
  cache_format_digest: Digest,
  specialized_policy_digest: Digest
}
```

The fixed field order is exactly the displayed order and is serialized with the JPK-C14N/1 record/array rules. `semantic_bundle_digest` is required for source-bound artifacts and MUST resolve to the complete graph-owned `SemanticBundle`; it is `null` only for explicitly machine-only artifacts. The fuel record names every deterministic and boundary charge: `fuel_schedule_digest` covers the complete versioned schedule, opcode/immediate/reference rows, bulk and straight-line quanta, flags, formulas, and safepoint placement; imported calls use `imported_call_charge_digest`, input and output codecs use their separate digests, and broker/native/device host effects use `host_effect_charge_policy_digest`. There is no cost-table-only alias. `specialized_policy_digest` identifies the complete cache-admission policy, including profile, dynamic-code, authority/refusal, debug, and mitigation decisions; it is not an undefined catch-all.

`foreign_runtime.family="none"` requires all other foreign-runtime fields to be null. A JVM/CLR/V8/BEAM/other adapter records its exact runtime version, build digest, and adapter identity. Provider and shim arrays are sorted and duplicate-free. Any one-field change changes `NativeCacheKey/2` and its digest `H(NativeCacheKey/2)`. `abi_digest` may be a display projection only; it never replaces this tuple. A missing, partial, stale, corrupt, revoked, or key-mismatched identity is an integrity failure, not an ordinary cache miss.

### P-CACHE-002 — Sealed-object admission

A cache record is:

```text
NativeCacheRecord {
  key: NativeCacheKey,
  key_digest: Digest,
  cache_digest: Digest,
  complete_marker_digest: Digest,
  producer_action: ActionId,
  package_generation: GenerationId,
  target_profile_digest: Digest,
  bytes_object: Digest,
  trust_state: "local" | "trusted-builder" | "revoked" | "quarantined",
  state: "staged" | "sealed" | "admitted" | "bad_cache" | "quarantined",
  invalidated_by: [Digest]
}

CacheCompleteMarker/1 {
  key_digest: Digest,
  cache_digest: Digest,
  package_generation: GenerationId,
  target_profile_digest: Digest,
  producer_action: ActionId,
  sealed_bytes_object: Digest
}
NativeCacheBody/1 {
  key_digest: Digest,
  cache_digest: Digest,
  complete_marker_digest: Digest,
  producer_action: ActionId,
  package_generation: GenerationId,
  target_profile_digest: Digest,
  bytes_object: Digest
}
NativeCacheBodyDigest = H("NativeCacheBody/1", canonical(NativeCacheBody/1))

CacheReaderLease {
  key_digest: Digest,
  cache_digest: Digest,
  map_generation: u64,
  admitted_at: Timestamp,
  charged_until: "unmapped" | "quarantined" | "released"
}
```

Admission acquires an immutable Hangar object or a sealed file descriptor whose complete marker and byte digest are checked on the same object. `key_digest = H(NativeCacheKey/2)`, `cache_digest = H(sealed_bytes)`, `complete_marker_digest = H(CacheCompleteMarker/1)`, and `NativeCacheBodyDigest = H(NativeCacheBody/1)` over the displayed fields; the marker's `sealed_bytes_object` MUST equal `bytes_object`, and the record's immutable body digest MUST match the sealed object. It compares the full key, producer action, package generation, target profile, trust, and digests before mapping. A missing object is a cache miss only when a fresh action can be formed from trusted inputs.

### P-CACHE-003 — Cache quarantine linearization

Publication is a per-key expected-old CAS: a staged record may become `sealed` only when the current `(key_digest, package_generation, target_profile_digest)` has no competing admitted record or the expected-old record is named and quarantined in the same journal transaction. Quarantine CAS compares `(key_digest, cache_digest, state=sealed/admitted)` under that journal. A partial object, bad complete marker, key mismatch, revoked producer, or digest mismatch becomes `bad_cache`, is retained as evidence, and receives a new rebuild action. A second builder cannot publish a partial or competing object as present.

Mapping an immutable object is the admission linearization point. The reader obtains a `CacheReaderLease` before mapping; quarantine cannot invalidate a lease that is already mapped and charged, while a reader that has not acquired the lease is refused after the quarantine fence. Charges and map leases remain governed by the host retained-charge/effect contract until unmapped/quiescent. Rebuild uses a new action and new object identity. `P-SUPPLY-005` and `P-GRAPH-003` are therefore integrity failures, not ordinary misses.

<a id="owned-source"></a>
## Owned source

### P-SOURCE-001 — Ownership and non-destructive default

A newly acquired source dependency defaults to a project-owned editable tree under `.jet/deps/<package-key>/`, unless it is already project/workspace-owned or explicitly selected as a binary/toolchain input. `.jet/lock` contains the canonical relationship. Hangar, cache cleanup, generation deletion, and unrequested install/update operations never overwrite or delete an owned tree. Explicit removal requires a receipt naming the exact tree and an archive/export/backup disposition.

An explicit conflict-free update or path-level user resolution may replace selected bytes, but only after a complete three-way merge, sealed-tree verification, expected-local-digest CAS, and a receipt naming the selected side. This is not a silent mutation. `P-SOURCE-001` therefore means “no silent or unrequested mutation/deletion,” not “an explicit resolved update can never publish new bytes.”

### P-SOURCE-002 — Complete source relationship schema

```text
OwnedSourceRecord {
  format: "JPK-SOURCE/1",
  source_record_digest: Digest,
  node_id: NodeId,
  name: String,
  relative_path: String,
  base: {
    tree_digest: Digest,
    manifest_digest: Digest,
    upstream_revision: String,
    imported_at: Timestamp
  },
  local: {
    root_identity: Digest,
    tree_digest: Digest,
    manifest_digest: Digest,
    last_recorded_tree: Digest,
    source_root_epoch: u64
  },
  upstream: {
    locator: String,
    revision: String,
    tree_digest: Digest,
    manifest_digest: Digest,
    license_digest: Digest
  } | null,
  ownership: "project" | "workspace" | "transitive-owned",
  dirty: Boolean,
  license_state: "present" | "changed" | "missing" | "policy-rejected",
  binary_paths: [String],
  transitive: [{ node_id: NodeId, ownership: String, pinned_tree: Digest }],
  ignore_rules_digest: Digest,
  last_merge_receipt: ReceiptId | null
}

SourceSnapshot {
  snapshot_id: Digest,
  root_identity: Digest,
  source_root_epoch: u64,
  tree_digest: Digest,
  entries: [{ relative_path: String, kind: "file" | "directory" | "symlink", mode: Integer,
              bytes_digest: Digest | null, link_target: String | null }],
  captured_at: Timestamp,
  acquisition: "filesystem-snapshot" | "exclusive-source-lease"
}
```

`source_record_digest = H(OwnedSourceBody/1)` where `OwnedSourceBody/1` contains the complete `OwnedSourceRecord` fields except `source_record_digest`, mutable journal state, and receipts. `OwnedSourceRef` is only a named lock projection of that one body: its node, root identity/epoch, base/local/upstream digests, ownership, dirty state, and merge receipt MUST equal the referenced source record. A lock/source journal disagreement is `conflict`, never two valid authorities.

SourceConflict {
  path: String,
  kind: "add/add" | "modify/modify" | "delete/modify" | "rename/rename" |
        "rename/edit" | "type" | "license" | "binary" | "dirty" | "transitive",
  base_digest: Digest | null,
  local_digest: Digest | null,
  upstream_digest: Digest | null,
  base_path: String | null,
  local_path: String | null,
  upstream_path: String | null,
  resolution: "unresolved" | "keep-local" | "take-upstream" | "manual",
  license_effect: "none" | "recheck" | "blocked",
  bytes_refs: [Digest]
}

The tree digest covers canonical no-follow relative paths, object kind, mode bits that affect builds, file bytes, and symlink target text. Entries are sorted by encoded relative path. Traversal outside the root, a changed type, or an unrepresentable special object is refusal, not a guessed merge.

### P-SOURCE-003 — Scan coherence and no speculative VFS

An update scans a complete `SourceSnapshot` of local `L`, exact immutable base `B`, and staged upstream `U`. A real filesystem snapshot primitive or an enforced project-source read/modify lease is required. An advisory lock alone is not proof that an arbitrary editor cannot write. If neither snapshot nor write-exclusion primitive is available, automatic update returns `unavailable` and keeps the staged candidate; it does not build a speculative virtual filesystem or discard user bytes.

The snapshot must include file/type/mode/symlink facts before merge. A second scan is required after acquiring the commit lease. If the current root digest or source-root epoch differs from the snapshot expectation, the transaction returns `source_concurrent_edit`, keeps the old tree and lock visible, and leaves the candidate user-owned for rebase or explicit replacement.

### P-SOURCE-004 — Three-way update

For each canonical path in `B`, `L`, and `U`: unchanged/added/deleted/modified/type-changed/binary classification is deterministic; unique equal-content delete/add is the only automatic rename. `L=B` selects `U`, `U=B` selects `L`, and `L=U` selects that content. All other combinations create a conflict. Binary, invalid UTF-8, executable, symlink, generated-but-user-edited, license, and transitive-source conflicts carry complete digest references and never receive fabricated text markers.

License paths and transitive source selection are protected policy inputs. Source, manifest, license, package-graph, and binary/toolchain checks run on the staged result. Any unresolved conflict prevents action receipt status `committed`.

### P-SOURCE-005 — Staged candidate and pair publication

The live owned directory remains untouched while `U`, merge output, and the sealed candidate are staged. A successful result writes a new `local.tree_digest`, `base.tree_digest=U`, upstream metadata, and merge receipt only in staged graph objects. Commit requires `expected_source_root_digest`, `expected_source_root_identity`, `expected_source_root_epoch`, `expected_lock_digest`, and `expected_graph_digest` to match under the source write-exclusion lease. A second scan compares all three source values after acquiring that lease; equal bytes in a distinct directory or a changed-and-restored root still conflict.

If the host provides a real atomic directory/root-pointer swap, the transaction durably records old/new root identities and epochs, then publishes the sealed root identity and lock under the same transaction journal and CAS. Recovery after any partial order either CAS-restores the old visible lock/root pair or quarantines the candidate; it never calls the pair committed from one pointer alone. If the host cannot provide that pointer operation and write exclusion, the updater refuses automatic replacement; it does not claim that rename plus advisory locking excludes arbitrary edits. A user may inspect/export the candidate and explicitly retry after a supported primitive is available.

### P-SOURCE-006 — Source/lock crash recovery

`source.begin`, `snapshot.captured`, `upstream.staged`, `merge.staged`, `conflicts.recorded`, `tree.sealed`, `lock.staged`, `cas.checked`, `lock.published`, `source.published`, and `publish.committed` are durable journal points. Recovery completes only an all-durable matching lock/source pair for one transaction ID and expected old digests. Otherwise it retains the old visible pair and quarantines the candidate. A new lock is never paired with an arbitrary current directory, and a new directory is never treated as committed merely because it exists.

Owned trees, staged candidates, backups, and recovery candidates remain user-owned. GC can remove only explicitly disposable staging names after lease/quarantine retention; it never removes an owned root, conflict bytes, license evidence, or a candidate referenced by a live receipt.

### P-SOURCE-007 — Resolution and removal

“Take upstream” is a user-selected receipt-bearing replacement of one complete path, not a silent overwrite. A manual file supplied under the owned root is copied into a staged candidate, hashed, and checked against the expected current root before commit. Removal checks active generations, reverse graph edges, retained receipts, and replacement generation; “not used by the current build” is not permission to delete.

<a id="supply-chain"></a>
## Supply chain

### P-TRUST-001 — Acyclic signed statements

The previous self-referential `SignedStatement` is replaced by these records:

```text
StatementIdentity/1 {
  signed_bytes_digest: Digest,
  signature_set_digest: Digest,
  predecessor_digest: Digest | null
}
RootBody/1 {
  role: String,
  version: u64,
  predecessor_digest: Digest | null,
  keys: [KeyRef] sorted,
  threshold: u32,
  expires: Timestamp,
  subject_scope: String
}

StatementBody {
  subject_digests: [Digest],
  predicate_type: String,
  predicate_bytes_digest: Digest,
  issued_at: Timestamp,
  expires_at: Timestamp | null,
  metadata_set_digest: Digest | null,
  predecessor_digest: Digest | null
}

SignedStatement {
  body: StatementBody,
  signed_bytes_digest: Digest,
  statement_digest: Digest,
  signatures: [{ key_id: String, signature: Bytes, signed_at: Timestamp }]
}
```

`StatementBody` excludes all derived digest fields and signatures. `signed_bytes_digest = H(StatementBody/1)` and each signature covers exactly the `Record("JPK-STATEMENT-BODY",1,fields)` bytes. Signature entries are sorted by `(key_id, signed_at, signature_bytes)` for identity; duplicate key IDs do not increase a threshold. `signature_set_digest = H(SignatureSet/1)` over that sorted set, and `statement_digest = H(StatementIdentity/1)` over `signed_bytes_digest`, `signature_set_digest`, and `predecessor_digest`; neither digest is included in its own preimage. `RootBody/1` is the exact signed root/rotation body, including role, predecessor, keys, threshold, version, expiry, and subject scope. Root, rotation, revocation, metadata, and builder statements use these fixed body rules.

A one-field mutation of a body changes `signed_bytes_digest`; a signature-set mutation changes `statement_digest`. A verifier rejects a digest field found inside the signed body, missing body fields, duplicate signatures, non-canonical bytes, or an expired signature.

### P-TRUST-002 — Trust records and separate decisions

```text
TrustPolicy {
  format: "JPK-TRUST/1",
  policy_digest: Digest,
  roots: [RootRole],
  allowed_algorithms: [String],
  threshold_rules: [ThresholdRule],
  max_metadata_age_seconds: Integer,
  offline_mode: "deny-stale" | "use-known" | "pinned-only",
  known_revocations: [RevocationRef],
  allowed_builders: [BuilderRef],
  allowed_licenses: [LicenseRule],
  native_policy: "explicit-local" | "explicit-attested" | "deny-remote",
  owner_software: "local-allowed" | "local-confirmed-policy"
}

RootRole {
  name: "root" | "targets" | "snapshot" | "timestamp" | "source" | "builder" | "release",
  root_digest: Digest,
  previous_root_digest: Digest | null,
  keys: [KeyRef],
  threshold: Integer,
  version: Integer,
  expires: Timestamp,
  rotation_statement: Digest | null
}
KeyRef {
  key_id: String,
  scheme: String,
  public_key_digest: Digest,
  role: String,
  not_before: Timestamp,
  not_after: Timestamp,
  status: "active" | "retiring" | "revoked" | "expired"
}

ThresholdRule {
  role: String,
  threshold: Integer,
  subject_scope: String,
  statement_predicate: String
}

RevocationRef {
  record_digest: Digest,
  subject_kind: "key" | "artifact" | "builder" | "toolchain" | "metadata",
  subject_digest: Digest,
  effective_at: Timestamp
}

BuilderRef {
  builder_id: String,
  statement_digest: Digest,
  builder_digest: Digest
}

LicenseRule {
  expression: String,
  disposition: "allow" | "warn" | "reject",
  evidence_digest: Digest | null
}

TrustSnapshot {
  snapshot_id: Digest,
  root_digest: Digest,
  root_version: Integer,
  metadata_set_digest: Digest,
  coverage_manifest_digest: Digest,
  lock_digest: Digest,
  graph_digest: Digest,
  revocation_epoch: u64,
  revocation_completeness: "complete" | "partial" | "unknown",
  known_empty_revocations: Boolean,
  subject_digests: [Digest] sorted and duplicate-free,
  issued_at: Timestamp,
  expires_at: Timestamp,
  freshness_policy_digest: Digest,
  signed_statement: Digest
}

AdmissionRecord {
  subject: Digest,
  identity: "source" | "jpm" | "native" | "toolchain" | "device" | "projection",
  signature_state: "unsigned-local" | "threshold-valid" | "invalid" | "expired" | "revoked" | "unknown",
  provenance_state: "absent" | "present" | "policy-accepted" | "policy-rejected",
  license_state: "accepted" | "warning" | "rejected",
  freshness: "fresh" | "stale-approved" | "unproven" | "missing",
  authority_state: "not-granted" | "requested" | "granted-by-host",
  decision: "admit" | "quarantine" | "reject",
  policy_digest: Digest,
  trust_snapshot: Digest | null,
  caller_context_digest: Digest | null,
  receipt: ReceiptId
}
```

Signature, provenance, license, freshness, revocation, verifier, and host-authority facts remain separate. A valid signature never supplies a JPH grant. A local owner policy may admit local source without registry notarization, but that remains a policy decision and not a safety or authority claim.

### P-TRUST-003 — Root predecessor CAS

`ThresholdEvidence/1 {
  subject_root_digest: Digest,
  role: String,
  threshold: u32,
  signer_key_ids: [Digest] sorted and duplicate-free,
  signature_digests: [Digest] sorted and duplicate-free,
  signed_statement_digest: Digest
}`
The evidence is valid only when `len(signer_key_ids) == len(signature_digests)`, each signature verifies the exact `signed_statement_digest`/`subject_root_digest` body under its key, and the distinct valid signer count is at least `threshold`; unknown keys, duplicate signers, threshold zero, and mismatched list order reject.

The accepted root identity is `H(RootBody/1)` where `RootBody` excludes `root_digest` and `rotation_statement`; those are derived links. A root rotation is a signed statement containing role, exact `previous_root_digest`, old root version, new root version, `old_threshold_evidence:ThresholdEvidence/1`, `new_threshold_evidence:ThresholdEvidence/1`, new root digest, and expiry; the evidence is bound by immutable digest/body, not an untyped list. The accepted-root store applies `expected_current_root_digest` and `expected_current_version` CAS. The only ordinary transition is `new_version = old_version + 1`; a skipped version requires an explicitly signed recovery statement naming every skipped predecessor and is not inferred from a larger integer.

A same-version different body, predecessor mismatch, rollback, invalid old/new threshold, expired key, or missing root signature is rejected. A successful rotation records the new accepted root and rotation receipt together. Recovery never chooses between two same-version roots by mirror order.

### P-TRUST-004 — Freshness, completeness, and offline defaults

`FreshnessPolicy/1 {
  clock_kind: "signed_wall_utc",
  unit: "nanoseconds",
  max_metadata_age_seconds: u64,
  max_clock_skew_seconds: u64
}`
`freshness_policy_digest = H("FreshnessPolicy/1", canonical(FreshnessPolicy/1))`; timestamps are compared only under this declared clock and checked for overflow.

The proposed defaults are `offline_mode="deny-stale"` and `max_metadata_age_seconds=86400`; both are explicit policy fields and are not current runtime defaults. A new activation or generation admission needs a `TrustSnapshot` whose root, metadata set, signed `coverage_manifest_digest`, revocation epoch/completeness, issue/expiry window, and sorted subject digests cover exactly the source/artifact/toolchain/interface subjects required by the selected lock/graph. The lock body carries `trust_snapshot_digest` and therefore references this snapshot; the admission view also compares the snapshot's expected lock/graph bindings and required subject set. The signed snapshot body does not point back to the lock/graph, so no snapshot->lock->snapshot identity cycle exists. `complete` is valid only when the signed manifest enumerates every required subject and its revocation set (including signed known-empty evidence when empty); an enum without that witness is `unknown` and refuses new admission under `deny-stale`/`pinned-only`. Freshness checks `issued_at <= now + max_clock_skew < expires_at` and `now - issued_at <= max_metadata_age_seconds` under the declared `FreshnessPolicy/1`; negative/overflowing deltas, unknown clock identity, or an expired bound refuse admission.

* `deny-stale` refuses a new admission when the snapshot is expired or revocation completeness is not `complete`.
* `pinned-only` admits a previously locked subject only with exact bytes and a fresh, signed, complete snapshot; it refuses moving references and refuses when the witness is absent or incomplete.
* `use-known` may admit an exact locked subject with an expired witness only when policy explicitly permits it, records `freshness="unproven"`, and never claims current registry state.

Absence of a revocation record is not a signed known-empty set. A disconnected already-admitted instance may continue under local lifecycle policy, but it cannot fetch a new secret/key or claim current revocation state. Reconnection compares trust epochs and fences assignments whose policy no longer admits them.

### P-TRUST-005 — Cache and trust interaction

A stale or incomplete trust witness never turns a corrupt cache into a miss. Cache admission first verifies the sealed object and complete `NativeCacheKey`, then checks the trust snapshot and builder policy. A revoked builder, artifact, toolchain, or cache is quarantined and receives a new action. The receipt identifies whether an existing instance continued under local policy or a new admission was refused.

<a id="lifecycle"></a>
## Lifecycle

### P-LIFE-001 — Complete lifecycle records

```text
InstallTransaction {
  transaction_id: String,
  graph_digest: Digest,
  lock_digest: Digest,
  expected_active_generation_pointer: Digest | null,
  requested_generation: GenerationId,
  stages: ["resolve", "fetch", "verify", "materialize", "build", "admit", "prepare"],
  idempotency_key: Digest,
  idempotency_label: String | null,
  journal: [JournalFact],
  status: "open" | "committed" | "failed" | "recovered" | "quarantined" | "conflict",
  receipt: ReceiptId
}

GenerationRecord {
  generation_id: GenerationId,
  graph_digest: Digest,
  lock_digest: Digest,
  artifacts: [Digest],
  services: [String],
  volume_bindings: [VolumeBinding],
  resource_attachment_digests: [Digest],
  secret_bindings: [SecretBinding],
  parent: GenerationId | null,
  authority_snapshot_digest: Digest,
  candidate_pointer_digest: Digest | null,
  active_pointer_digest: Digest | null,
  state: "staged" | "prepared" | "starting" | "ready" | "active" | "draining" | "retained" | "failed" | "rolled-back" | "fenced",
  activation_epoch: Integer,
  readiness_digest: Digest | null,
  route_receipt: ReceiptId | null,
  proof_digest: Digest,
  receipt: ReceiptId
}

InstanceRecord {
  instance_id: String,
  generation_id: GenerationId,
  node_id: NodeId,
  host_id: String,
  resource_attachment_digests: [Digest],
  state: "declared" | "prepared" | "starting" | "running" | "ready" | "draining" | "stopped" | "failed" | "killed" | "unknown",
  process_identity: String | null,
  application_authority_digest: Digest,
  authority_digest: Digest,
  host_import_decision_receipt: ReceiptId,
  authority_revocation_epoch: u64,
  authority_snapshot_digest: Digest,
  resource_digest: Digest,
  restart_count: Integer,
  health: HealthFact,
  stop: {
    caller_deadline: Duration | null,
    guest_stop: "not-requested" | "requested" | "observed" | "failed",
    helper_state: "none" | "running" | "orphaned" | "exited",
    resource_release: "held" | "quarantined" | "released",
    quiescence: "unknown" | "observed" | "fenced",
    quiescence_proof: QuiescenceProofRef | null
  },
  receipt: ReceiptId
}

ServiceRecord {
  service_id: String,
  generation_id: GenerationId,
  dependencies: [{ service_id: String, condition: "prepared" | "running" | "ready" | "completed" }],
  endpoint_identity: String,
  volume_bindings: [VolumeBinding],
  resource_attachment_digests: [Digest],
  secret_bindings: [SecretBinding],
  readiness: ReadinessSpec,
  restart: RestartPolicy,
  state: "declared" | "prepared" | "starting" | "running" | "ready" | "draining" | "stopped" | "failed" | "quarantined"
}

ReadinessSpec {
  predicate_digest: Digest,
  timeout: Duration,
  probe_authority_digest: Digest,
  retry_policy_digest: Digest,
  failure: "pause" | "fence" | "fail-generation"
}
VolumeBinding {
  volume_id: String,
  expected_epoch: Integer,
  schema_version: String,
  mount_mode: "read-only" | "read-write" | "exclusive",
  fence_token_digest: Digest,
  attachment_digest: Digest
}

HealthFact {
  health_epoch: Integer,
  predicate_digest: Digest,
  result: "healthy" | "unhealthy" | "not-ready" | "denied" | "unsupported" | "timeout" | "crashed",
  observed_at: Timestamp,
  detail_digest: Digest | null
}

RestartPolicy {
  mode: "never" | "on-failure" | "always",
  max_attempts: Integer,
  initial_delay: Duration,
  max_delay: Duration,
  circuit_breaker_digest: Digest
}

VolumeRecord {
  volume_id: String,
  content_identity: Digest | null,
  mutable_epoch: Integer,
  owner_service: String,
  schema_version: String,
  backup_refs: [BackupId],
  resource_attachment_digest: Digest | null,
  fence_epoch: Integer,
  mount_mode: "read-only" | "read-write" | "exclusive",
  state: "available" | "attached" | "migrating" | "detached" | "quarantined" | "destroyed"
}

BackupBody/1 {
  volume_id: String,
  source_epoch: u64,
  schema_version: String,
  snapshot_digest: Digest,
  content_digest: Digest,
  complete_marker_digest: Digest,
  encryption_key_ref: String,
  key_version: String,
  consistency: "application-consistent" | "crash-consistent" | "unknown",
  created_at: Timestamp
}
BackupRecord {
  backup_id: BackupId,
  body: BackupBody/1,
  state: "staged" | "sealed" | "complete" | "incomplete" | "quarantined" | "restored",
  restore_receipt: ReceiptId | null
}

MigrationRecord {
  migration_id: String,
  volume_id: String,
  expected_source_epoch: Integer,
  from_schema: String,
  to_schema: String,
  preflight_digest: Digest,
  backup: { kind: "none" } | { kind: "present", backup_id: BackupId },
  idempotency_tuple_digest: Digest,
  idempotency_key: Digest,
  idempotency_label: String | null,
  compatibility: "old-readable" | "new-readable" | "dual" | "unknown",
  state: "planned" | "backed-up" | "running" | "committed" | "failed" | "unknown" | "restored" | "irreversible",
  external_effect: "none" | "attempted" | "committed" | "unknown",
  reconciliation_receipt: ReceiptId | null,
  resulting_epoch: Integer | null
}

SecretPolicyBody/1 {
  accepted_versions: [Digest] sorted,
  drain_start: Timestamp | null,
  drain_until: Timestamp | null,
  revocation_mode: "future_fetches" | "all_handles_at_deadline",
  fence_epoch: u64
}
SecretPolicy/1 {
  policy_digest: Digest,
  body: SecretPolicyBody/1
}
SecretBindingBody/1 {
  secret_id: String,
  version_digest: Digest,
  predecessor_binding_digest: Digest | null,
  recipient_service: String,
  authority_digest: Digest,
  fence_epoch: u64,
  not_before: Timestamp,
  expires_at: Timestamp | null,
  rotation_policy_digest: Digest
}
SecretBinding {
  binding_id: Digest,
  body: SecretBindingBody/1,
  host_binding_receipt: ReceiptId,
  state: "prepared" | "accepted" | "active" | "retiring" | "revoked",
  state_epoch: u64,
  expected_old_state: "prepared" | "accepted" | "active" | "retiring" | "revoked" | null,
  transition_receipt: ReceiptId | null
}
```

`BackupId = H(BackupBody/1 body)`. Mutable `state` and `restore_receipt` are lifecycle facts and never change the immutable backup identity. A backup identity therefore binds volume, source epoch, schema, complete marker, consistency, encryption-key reference, and bytes. Restore additionally requires a storage fence or operations-owned scope-appropriate `QuiescenceProof` plus expected current volume/fence CAS; a digest alone is never a restore authorization.

### P-LIFE-001A — Complete QuiescenceProof owned by operations

Operations owns the graph-level `QuiescenceProof/1` record. Its effect fields are references to the exact host `HOST-LP-019` receipts; it does not redefine host effect-point, cancellation, close, delegation, or resource-wire types.

```text
QuiescenceProof {
  format: "JPK-QUIESCENCE/1",
  proof_digest: Digest,
  instance_id: String,
  generation_id: GenerationId,
  host_id: String,
  scope_kind: 0 | 1 | 2,       # 0 instance/channel, 1 resource_binding, 2 work_unit
  scope_id: Digest,
  scope_generation: u64,
  scope_owner_channel: Digest,
  scope_state: "closed" | "quiescent" | "fenced",
  admission_epoch: u64,
  channel_id: String | null,
  channel_generation: Integer | null,
  stop_receipt: ReceiptId | null,
  close_and_drain_receipt: ReceiptId,
  inventory_digest: Digest,
  operation_outcome: OperationOutcomeRef | null,
  observed_at: Timestamp,
  channel: {
    state: "closed" | "unchanged",
    close_receipt: ReceiptId | null
  },
  admitted_work: {
    nonterminal_calls: Integer,
    call_ids: [String],
    callbacks: Integer,
    callback_ids: [String],
    leases: Integer,
    binding_ids: [String],
    effect_receipts: [ReceiptId]
  },
  queues: {
    queue_ids: [String],
    drained: Boolean,
    drain_receipts: [ReceiptId]
  },
  helpers: [{
    helper_id: String,
    owner_generation: GenerationId,
    assignment_digest: Digest,
    target_identity: Digest,
    terminal_state: "exited" | "fenced",
    fence_epoch: Integer | null,
    fence_token_digest: Digest | null,
    host_receipt: ReceiptId
  }],
  writers: [{
    resource_id: String,
    resource_kind: "volume" | "endpoint" | "secret" | "device" | "other",
    owner_generation: GenerationId,
    assignment_digest: Digest,
    target_identity: Digest,
    fence_epoch: Integer | null,
    fence_token_digest: Digest | null,
    backend_receipt: ReceiptId
  }],
  effect_summary: {
    host_outcome_receipts: [ReceiptId],
    residual_effects_digest: Digest,
    retained_charges_digest: Digest | null
  },
  result: "observed" | "fenced"
}

QuiescenceProofRef {
  proof_digest: Digest,
  scope_kind: 0 | 1 | 2,
  scope_id: Digest,
  scope_generation: u64,
  scope_owner_channel: Digest,
  scope_state: "closed" | "quiescent" | "fenced",
  result: "observed" | "fenced",
  host_outcome_receipts: [ReceiptId],
  residual_effects_digest: Digest,
  retained_charges_digest: Digest | null
}
```

`proof_digest = H(QuiescenceProof/1 body)` with `proof_digest` excluded. The body uses the fixed field order above; all IDs, effect receipts, helper rows, and writer rows are sorted by encoded identity. The broker freezes admissions at one scope-specific close/epoch linearization, computes `inventory_digest` over the complete closed-world set of calls, callbacks, leases, bindings, helpers, queues, and writers in that scope, and rejects omitted rows. For `scope_kind=0`, `scope_state="closed"`, `stop_receipt` and `close_and_drain_receipt` are present, channel identity is present, all admitted-work counters are zero, queues are drained, and every helper/writer is exited or exactly fenced. For `scope_kind=1`, `scope_state="quiescent"|"fenced"`, `scope_id` names exactly one resource binding generation and `scope_owner_channel` identifies its owner; this proof is not an interchangeable object-wide or any-binding proof. Only work/leases/writers that can use that exact binding are required to be terminal or fenced; `channel.state` MUST be `"unchanged"` and unrelated channel work is not a predicate. Physical object reclaim additionally requires every other binding and use of that object to be released or separately proven. For `scope_kind=2`, the named call/helper/lease is terminal or fenced and its dependent writer rows are covered; this named-work proof does not free or authorize reuse of unrelated work, bindings, or object state. Every writer acknowledgement binds target, assignment, owner generation, and exact fence token. `result="observed"` requires exit/completion and no outstanding writer for the scope; `result="fenced"` requires a non-null fence token/epoch and acknowledgement for every member that did not exit. `retained_charges_digest` is mandatory whenever any referenced outcome has `fence_required=true` or `awaiting_quiescence`; it is mandatory as an explicit empty-set digest otherwise. A proof never infers quiescence from counters read before the scope freeze or from a device-loss notification.

The host contract supplies the meaning of each referenced effect receipt, retained charge, close, and fence acknowledgement. Operations MUST preserve those values and MUST NOT infer quiescence from `guest_stop`, a caller timeout, an advisory lock, or a process PID.

### P-LIFE-002 — Candidate-before-active activation

`activation.prepare` records the expected active generation pointer, exact authority snapshot `(application_authority_digest, authority_digest, revocation_epoch, host_import_decision_receipt)`, target/profile, volume/secret bindings, route policy, and fence requirements. It publishes only a non-authoritative `candidate_pointer`.

The host then spawns the candidate, records process identity, checks the exact authority digest and revocation epoch atomically at channel creation, and waits for every declared readiness predicate. Before any external route, volume, secret, or device cutover/fence, a failed readiness or authority check leaves A and those external targets unchanged. Only after readiness, required volume/secret/endpoint fences, and route admission succeed may activation attempt the expected-old local state transition. A backend that truly owns a shared transactional local pointer/route pair may atomically CAS that pair; otherwise the pointer and route are separate linearization points, each with its own expected-current compare and receipt. No local operation makes a remote cutover atomic or promises that A remains routable/writable after an external effect.

If the local backend cannot prove that shared pair transaction, the operation records separate local pointer and route receipts and does not mark the generation active until both match the same transaction. If that proof cannot be made before external cutover, it refuses or leaves the candidate staged. Once any external route/fence/commit may have occurred, recovery records the actual target as `unknown` or `fenced` rather than assuming the old route or pointer.


### P-LIFE-003 — Activation recovery
Before any external cutover, a crash before local commit leaves A and all external targets unchanged and fences/stops unready B. A crash-before-local-commit alone does not prove that no remote effect occurred once an external route, fence, or commit may have happened. After any such effect, recovery retains A's code, fences or quarantines B, records the actual target `unknown`/`fenced`, and makes no claim that A remains routable or writable. It reconciles the actual target, then uses an expected-current CAS to complete B or restore A only when data, schema, and authority safety checks pass. A process found by identity without a complete instance record is stopped or quarantined, never silently adopted. A committed generation remains retained even if later service health fails.

`ExternalCutoverBody/1 {
  target_kind: "route" | "volume" | "secret" | "device",
  target_identity: Digest,
  old_binding_digest: Digest | null,
  new_binding_digest: Digest,
  expected_old_backend_digest: Digest | null,
  expected_old_local_pointer: Digest | null,
  fence_token_digest: Digest,
  operation_key: Digest,
  authority_snapshot_digest: Digest
}`
`ExternalCutoverRecord` adds `state: "prepared" | "committed" | "unknown" | "fenced"`, `prepare_receipt`, `barrier_receipt`, and `commit_receipt`; its identity is `H("ExternalCutoverBody/1", body)` and lifecycle fields are excluded.

P-LIFE-003 recovery treats the local active pointer and each external backend as separate linearization points. A truly shared transactional local backend may atomically CAS its own pointer/route pair; external route, volume, secret, and device backends remain separate. Backend prepare/commit MUST compare the expected-old backend digest and exact fence token, and commit MUST return a receipt naming the new digest and `ExternalFenceBarrier/1`. Active publication requires durable receipts for the same `operation_key`, generation, target, and authority snapshot; the receipts do not claim one global atomic operation. If any side succeeds while another reply is lost or its CAS fails, the controller preserves A's code, fences or quarantines B, records the actual target `unknown`/`fenced`, and does not assert that A remains routable or writable. It reconciles the actual target, then uses expected-current CAS to complete B or restore A only when data, schema, and authority safety checks pass. It never marks B active from a local pointer alone and never overwrites an unknown backend route without a receipt-bound compare.

### P-LIFE-004 — Authority snapshot at prepare/spawn

The activation transaction stores one content-identified `AuthoritySnapshot/1` and binds its digest into `ActivationPrepare/1`, `GenerationRecord`, `InstanceRecord`, `ActivationFact`, and the prepare receipt. Channel creation and first effect admission compare all four members (`application_authority_digest`, `authority_digest`, `revocation_epoch`, `host_import_decision_receipt`) against current host authority atomically. A mismatch returns `authority_changed_before_spawn`, aborts/replans preparation, and never widens to live policy. A post-activation revocation denies future admissions under the host rule; already admitted effects keep the exact host-owned `EffectView` and `RetainedCharge` classification in their receipts. Any worked-example shorthand such as `(A2,E8)` is explicitly symbolic for the complete four-member snapshot, never a two-field substitute.

### P-LIFE-005 — Stop and quiescence consumption

A caller deadline, guest stop observation, helper state, resource release, effect completion, and quiescence are separate facts. `quiescence="observed"` is legal only with a matching `QuiescenceProofRef` whose `result="observed"`; `quiescence="fenced"` is legal only with a proof whose `result="fenced"` and backend acknowledgements cover every writer/volume/endpoint/secret path. A stop request alone never releases a resource or permits a conflicting writer.
A hard-kill request is not proof of immediate process, kernel, driver, or device exit. A general-purpose host cannot promise an absolute hard-stop bound; it retains charges and bindings until the host proof of exit/fence. Local activation is the only atomic pointer scope; fleet activation and external effects remain non-global and non-rollbackable. Route, volume, secret, and device activation therefore require the `ExternalCutoverRecord`/barrier receipts above; a two-phase journal is coordination evidence, not a claim that remote and local storage commit atomically.

Every lifecycle operation embeds `OperationOutcomeRef` from the exact host operation receipt. Cancellation before the host effect point is represented by `effect_view.phase=not_started`; admission at or after that point is represented as `accepted`, `completed`, or `unknown`, with exact `retractability`, `partial`, `retry`, and `quiescence` axes. It is never represented as a false no-effect cancellation. Pending helper/driver/storage work retains the exact `RetainedCharge` until the host proves exit/fence under `HOST-LP-019`. Operations does not create a second effect model.

### P-LIFE-006 — State machine and journal order

Install: `open -> resolving -> fetched -> verified -> materialized -> built -> admitted -> prepared -> committed`; any stage may become `failed`, `conflict`, or `quarantined`.

Generation: `staged -> prepared -> starting -> ready -> active -> draining -> retained`; `staged/prepared/starting/ready -> failed|fenced` is allowed. `active` requires readiness and route CAS.

Instance: `declared -> prepared -> starting -> running -> ready`; `running -> draining -> stopped` or `running -> killed|unknown`, with reclaim only after host helper exit/fence proof.

Migration: `planned -> backed-up -> running -> committed`, or `planned -> failed`, `running -> unknown`, `unknown -> committed|failed|restored` only after reconciliation. `unknown` is not an alias for `failed` and never authorizes automatic rollback.

Journal semantic points are `tx.begin`, `inputs.bound`, `objects.sealed`, `generation.staged`, `activation.prepare`, `candidate.published`, `instance.spawned`, `service.ready`, `route.prepare`, `activation.commit`, `drain.begin`, `instance.stopped`, `helper.exit|fence.established`, `resource.release`, `migration.effect`, `migration.reconciled`, and `generation.retained`. Each point carries the exact parent pointer, action/receipt IDs, and effect/quiescence refs required by the host contract.

### P-LIFE-007 — Backup and restore CAS

A restore selects a complete, readable, policy-approved `BackupRecord` whose immutable `BackupBody/1`, `volume_id`, source epoch, schema compatibility, key version, and completeness match the migration expectation. Before replacing the current volume it requires a storage backend barrier or operations-owned instance/resource/work-unit `QuiescenceProof`, then CAS-compares current volume ID/epoch/schema/fence. On success it creates a new volume epoch and binds the new fence to the restore transaction; it never mutates the old backup. A mismatch, incomplete marker, unavailable key, unreadable bytes, missing writer fence, or wrong schema returns refusal and leaves the current volume unchanged.

### P-LIFE-008 — Migration unknown and external effects

An external migration action keeps one idempotency tuple across retries. If a reply is lost after an external effect, the durable state is `unknown` with `external_effect=unknown`; the service must reconcile its durable state before `committed`, `restored`, or rollback. Automatic rollback is forbidden while unknown. A committed external effect may be irreversible; rollback receipts set `external_effects_undone=false` when applicable.

### P-LIFE-009 — Volume ownership

Exclusive writers require the current volume fence epoch and a valid host fence. Read-only snapshots may coexist only under the storage service's consistency proof. A hard-kill request, stale generation pointer, or expired lease never releases an exclusive volume. Charges and aliases remain retained according to the host `QuiescenceProof`/effect contract.

### P-LIFE-010 — Secret rotation

Secret plaintext never enters a manifest, artifact, lock, receipt, or debugger record. Human version labels in service diagnostics are display-only; fetch/attach resolves the broker-issued binding and `version_digest`, never a raw version string. `binding_id = H("SecretBindingBody/1", body)`; `SecretPolicy/1` is immutable and its `policy_digest` MUST equal `H("SecretPolicyBody/1", body)`. Rotation is a durable expected-old CAS `prepared(new) -> accepted(new) -> active(new) -> retiring(old) -> revoked(old)` with `state_epoch` incremented once and `transition_receipt` naming the old/new states. `predecessor_binding_digest`, `fence_epoch`, authority digest, and host binding receipt are required at every transition; two bindings cannot both be `active` for one service/fence epoch. New fetches compare the current secret fence epoch, exact policy digest, and authority digest and carry the current external cutover fence token. Existing handles follow the host contract: they may continue only between `drain_start` and `drain_until` recorded by `SecretPolicy/1` and are refused after retirement/revocation; a record cannot infer continued use from a logical secret ID.

Endpoint routing, volume attach, and secret activation use an ordered cutover: establish external fence/epoch, admit the new binding, prove candidate readiness, route new traffic, drain old generation, then retire/revoke old secret. A partitioned host cannot fetch a new version or report global rotation complete. A mixed-version route is allowed only when both versions' compatibility and fence records are explicit.

### P-LIFE-011 — Idempotent operations

The host/service graph persists this durable record at the operation linearization point:

```text
OperationTuple/1 {
  operation_domain: String,
  operation_kind: String,
  service_or_backend: Digest,
  target_identity: Digest,
  expected_parent_or_epoch: Digest,
  input_digest: Digest,
  output_schema_digest: Digest,
  deployment_or_fence_epoch: u64,
  policy_digest: Digest,
  graph_digest: Digest,
  lock_digest: Digest,
  authority_digest: Digest,
  caller_context_digest: Digest | null,
  parent_operation_digest: Digest | null
}
OperationDedupRecord {
  key: Digest,
  tuple: OperationTuple/1,
  tuple_digest: Digest,
  state: "attempted" | "committed" | "unknown",
  effect_receipt: ReceiptId | null,
  terminal_receipt: ReceiptId | null,
  residual_effects_digest: Digest | null
}
```

`tuple_digest = H(OperationTuple/1 body)` with no derived fields. `key = H(OperationDedupKey/1)` over `operation_domain`, `operation_kind`, `service_or_backend`, `target_identity`, `expected_parent_or_epoch`, `input_digest`, `output_schema_digest`, `deployment_or_fence_epoch`, `policy_digest`, `graph_digest`, `lock_digest`, `authority_digest`, `caller_context_digest`, and `parent_operation_digest`; `key`, `tuple_digest`, `state`, receipt/result fields, and residual effects are excluded. The store performs a durable unique-key insert/CAS at the external effect linearization point and retains the tuple for the declared retention window; key reuse after expiry is refused unless a new epoch/namespace is present. An exact retry returns the same record; a reused key with a different tuple returns `conflict`; concurrent inserts cannot both admit the effect. `attempted` or `unknown` requires reconciliation or an explicit abort receipt and never silently repeats an external effect.

### P-LIFE-012 — No rollback fiction

Code rollback creates a new activation transaction with the failed generation as parent. It validates current host profile, authority, interface, secret versions, volume schema/epoch, backup facts, readiness, and fence state. It does not edit history, undo packets/secrets/database effects, or activate a half-prepared tree.

<a id="distributed-operation"></a>
## Distributed operation

### P-DIST-001 — Fleet records and local authority

```text
FleetRecord {
  fleet_id: String,
  graph_digest: Digest,
  policy_digest: Digest,
  hosts: [HostRecord],
  rollout: RolloutPolicy,
  endpoint_policy_digest: Digest,
  secret_policy_digest: Digest,
  volume_policy_digest: Digest
}
HostRecord {
  host_id: String,
  profile_digest: Digest,
  authority_domain: Digest,
  lease: { owner: String, epoch: Integer, expires: Timestamp },
  last_seen_generation: GenerationId | null,
  capability_digest: Digest,
  external_fence_epoch: Integer,
  state: "eligible" | "staging" | "ready" | "active" | "partitioned" | "fenced" | "draining" | "unavailable"
}
DeploymentRecord {
  deployment_id: String,
  fleet_id: String,
  target_generation: GenerationId,
  parent_generation: GenerationId,
  assignments: [AssignmentRecord],
  phase: "planned" | "staging" | "canary" | "rolling" | "paused" | "completed" | "degraded" | "rollback-requested" | "rolling-back" | "abandoned",
  idempotency_key: Digest,
  control_receipt: ReceiptId
}
AssignmentRecord {
  host_id: String,
  generation: GenerationId,
  expected_parent_generation: GenerationId,
  fence_epoch: Integer,
  state: "assigned" | "staged" | "ready" | "activated" | "draining-old" | "confirmed" | "failed" | "fenced" | "unknown",
  health_digest: Digest | null,
  host_receipt: ReceiptId | null
}
RolloutPolicy {
  batch_size: Integer,
  canary_count: Integer,
  min_ready_fraction: Decimal,
  monitor_seconds: Integer,
  on_failure: "pause" | "rollback-code" | "continue-degraded",
  max_unavailable: Integer,
  drain_seconds: Integer,
  require_external_fence: Boolean
}
```

A host assignment is valid only for deployment ID, target generation, expected parent, host ID, and current fence epoch. The controller stores assignment receipts and never derives state from a host status string.

### P-DIST-002 — External fence enforcement

Operations records the shared fence token and backend acknowledgement; a host-local lease is not an external fence:

```text
ExternalFenceBody/1 {
  target_kind: "volume" | "endpoint" | "secret" | "device" | "host",
  target_id: String,
  previous_epoch: u64,
  new_epoch: u64,
  assignment_digest: Digest,
  backend_identity_digest: Digest
}
ExternalFenceRecord {
  fence_id: Digest,
  body: ExternalFenceBody/1,
  state: "requested" | "established" | "lost" | "unknown",
  requested_receipt: ReceiptId,
  acknowledgement_receipt: ReceiptId | null
}
`fence_id = H("ExternalFenceBody/1", body)`; state and receipts are lifecycle views.

FenceToken {
  target_kind: "volume" | "endpoint" | "secret" | "device" | "host",
  target_id: String,
  epoch: u64,
  assignment_digest: Digest,
  backend_identity_digest: Digest,
  acknowledgement_receipt: ReceiptId
}
```

`ExternalFenceBarrierBody/1 {
  target_kind: "volume" | "endpoint" | "secret" | "device" | "host",
  target_id: String,
  previous_epoch: u64,
  new_epoch: u64,
  assignment_digest: Digest,
  backend_identity_digest: Digest,
  prior_writer_set_digest: Digest,
  prior_writer_count: u64
}
`ExternalFenceBarrier/1 { barrier_digest: Digest, body: ExternalFenceBarrierBody/1, barrier_receipt: ReceiptId }` is the backend's IO barrier view, with `barrier_digest = H("ExternalFenceBarrierBody/1", body)`; receipt/lifecycle fields are excluded from the body. `prior_writer_set_digest` covers every earlier-epoch writer/submission that was completed or rejected; an omitted/unknown set cannot establish the barrier. The shared volume writer/attach, endpoint route update, secret fetch/activation, and device/audio/GPU/DMA submission backend MUST compare the presented `FenceToken` to the current `(target_kind,target_id,epoch,assignment_digest)` and accept only exact equality. An `established` acknowledgement is valid only after every writer/submission from `previous_epoch` is complete or rejected and the barrier receipt names that set; generation invalidation/device loss alone is not a barrier. An older epoch is rejected; a future epoch is rejected until the backend has established it. A host-local rejection of a future control message is not a fence. A new host B may become ready/active for a conflicting writer only after old host A's epoch is externally fenced, or when the new operation is read-only/isolated under a recorded proof.

If a volume, endpoint, secret, or device backend cannot compare the current fence token and return the barrier receipt, the rollout pauses/degrades and retains the old assignment; it never marks B globally safe. `state="established"` requires the backend acknowledgement plus `ExternalFenceBarrier/1` covering all prior-epoch writers; `lost` or `unknown` prevents new writes, route changes, secret fetches, and device submissions until reconciliation. Fence evidence is a receipt-bound effect, not a controller assertion.

### P-DIST-003 — Rollout order and partitions

1. Validate graph, generation, authority snapshot, trust witness, profile, migration/backup, secret, volume, endpoint, and rollout policy.
2. Acquire control-plane lease and monotonically assign deployment/fence epochs. Lease reachability is not host readiness.
3. Stage and verify each host. Publish only candidate pointers; local activation follows P-LIFE-002.
4. Establish external fences where required, spawn, prove readiness, and CAS local active/route pointers.
5. Wait the declared monitor interval and health receipt before the next batch.
6. Drain old generations only after route and fence receipts. A failure pauses, rolls back code only when data compatibility permits, or continues degraded only when the minimum-ready rule still holds.
7. Complete only when every required assignment is confirmed or the declared degraded policy admits the missing hosts. The final receipt lists partitions, unknown effects, fenced hosts, mixed versions, and residual actions.

During partition, a host may continue an already admitted generation under its last valid lease and local authority policy, but it cannot fetch a new secret/key, accept a new control mutation, or claim fresh revocation/rollout state. On reconnection, the controller reconciles durable operation records, compares epochs, externally fences stale assignments, and classifies lost replies before retry.

### P-DIST-004 — Durable distributed deduplication

Fleet actions use `OperationDedupRecord` in the existing host/service graph. The deployment tuple includes deployment ID, host ID, generation, expected parent, fence epoch, graph/lock digest, input/output policy digests, and action kind. Exact retries return the same receipt; changed tuples conflict. A lost reply is `unknown` until the host/service ledger reconciles. There is no global exactly-once or global rollback promise.

### P-DIST-005 — Mixed versions and routing

Protocols and data migrations declare compatibility for old/new generations. Endpoint routing, secret versions, and volume epochs have independent records and must not be inferred from code generation. A service is traffic-ready only after its route, volume, secret, and health facts satisfy its readiness predicate. An endpoint tag or Kubernetes object is a projection and cannot override the Jet graph.

### P-DIST-006 — Secret and authority cutover

The fleet secret policy names accepted versions, predecessor, fence epoch, route policy, drain window, and revocation behavior. A host whose authority or trust snapshot changed between prepare and spawn refuses activation rather than widening rights. A partitioned host remains on its recorded version until drain/reconnect policy decides; it cannot claim that a controller-side rotation reached it.

<a id="compatibility"></a>
## Compatibility and host-platform profiles

### H-COMP-001 — Meaning versus availability

Profiles may differ in capabilities, limits, scheduling, executable-memory policy, device presence, and enforcement backend, but never in scalar/type/trap/error/ownership/cancellation meaning. A mandatory missing operation, limit, affinity, resource, kill/fence backend, or isolation property causes pre-activation refusal. Optional absence returns `unsupported` or `unavailable`; `denied` means authority/user policy refused. Renaming a losing workload as a profile is not a pass.

### H-COMP-002 — Standards mapping

Pinned adapters retain the existing proposal's RFC 3629 UTF-8, RFC 3986 URI, RFC 9110/9112 HTTP, RFC 8446 TLS, WebAssembly 3.0, WASI 0.2/3, OCI 1.1.1/1.2, WebGPU 18 September 2026, SPIR-V 1.6 Revision 8, Vulkan 1.4 environment, and PTX ISA 9.4 boundaries. They map values/resources through JPH and do not replace JPH identity, typed ownership, redirects, authority, or receipts. Component Model binary maturity and adapter proof remain explicit gates.

### H-COMP-003 — Linux hostile baseline

A hostile Linux profile requires a named backend with all of: dedicated worker identity; no-new-privileges; mandatory seccomp syscall policy; declared descriptor-inheritance policy; mount/filesystem namespace or equivalent path boundary; declared network namespace/egress policy; cgroup or equivalent CPU/memory/accounting backend when those limits are mandatory; and a kill/reclaim path whose quiescence behavior is recorded. Descriptor-relative no-follow opens use `openat2(2)` or a named equivalent with the selected resolve flags. A same-user worker process without these mechanisms is trusted/weak only and cannot claim hostile isolation. Missing any mandatory mechanism refuses activation.

### H-COMP-004 — Hostile macOS minimum

A macOS profile may claim `hostile` only with `MacHostileBackend/1`:

```
MacHostileBackend {
  backend:u8                 # 00 AppSandbox+Seatbelt, 01 signed VM/container
  code_signing:u8            # 00 hardened runtime mandatory, 01 unavailable
  descriptor_policy:u8        # 00 allowlisted inheritance only, 01 unavailable
  filesystem_policy:u8       # 00 security-scoped bookmark/no-follow roots, 01 unavailable
  network_policy:u8           # 00 explicit entitlement/egress filter, 01 unavailable
  device_policy:u8            # 00 broker-only typed devices, 01 unavailable
  executable_policy:u8       # 00 signed AOT/interpreter, 01 signed JIT entitlement
  helper_kill_backend:u8     # NativeKill enum, not unavailable
  accounting_backend:u8      # NativeAccounting enum, not unavailable
  fence_backend_digest:Digest256
  policy_digest:Digest256
}
```

`backend=00` requires an App Sandbox/Seatbelt profile, hardened code-signing/runtime, explicit descriptor inheritance allowlist, security-scoped bookmark/root generation checks, explicit network/device entitlements, broker-only handles, and a declared helper kill/accounting/fence path. `backend=01` requires a signed VM/container with equivalent facts. Any `unavailable` field, missing code-sign/JIT policy, same-user worker without the named boundary, or unenforceable mandatory limit classifies the profile as trusted/weak and non-hostile or refuses activation. XPC/launchd transport alone is not containment. macOS path/bookmark staleness is `stale`; a path string never grants.

### H-COMP-005 — Windows

The broker owns `HANDLE`s with desired access/sharing/reparse policy; handle identity, not `GetFinalPathNameByHandle`, is authority. Hostile portable instances require AppContainer or equivalent plus Job Object or declared kill/accounting backend. Message queue/window-thread affinity is explicit; synchronous `SendMessage` is unavailable under `no_reentry`. Winsock temporary/permanent errors preserve JPH categories. Missing descendant or output enforcement refuses spawn.

### H-COMP-006 — Browser

The principal is the browser tuple origin `(scheme,host,port)` plus browser-issued worker/channel identity. Opaque origins are not serialized as filesystem/network identities. OS paths, shell, process, raw sockets, and arbitrary secrets are unavailable. OPFS and picker handles are typed resources with quota/permission expiry. Fetch/CORS is an HTTP backend restriction, not a grant; JS/DOM objects and pointers never become resource tokens. Mandatory native-only operations refuse before activation.

### H-COMP-007 — Mobile

iOS requires app-container, signing, declared entitlements, and interpreter/signed AOT unless a separately signed entitled JIT profile proves executable-memory policy. Android uses app sandbox, scoped storage, declared permissions, and checked shipped/interpreter/AOT code; remotely loaded executable behavior is not a silent fallback. App lifecycle interruption returns `interrupted` and does not claim effect rollback. Missing background, JIT, dynamic-code, or persistence guarantees remain unavailable/refusal.

### H-COMP-008 — MMU, MPU, and no-MMU embedded

MMU profiles may use process/page isolation. MPU profiles advertise finite aligned regions, count, and granularity. No-MMU profiles may provide checked software bounds/static pools and trusted portable execution, but cannot claim hostile-process isolation, arbitrary hard termination, temporal safety against a native helper, or hard native-call/device bounds without an external process, hardware partition, co-processor, or equivalent. Raw MMIO/DMA requires the typed device ranges and exclusive leases above. Mandatory stronger properties refuse activation.

### H-COMP-009 — Device/audio profile

GPU/audio/device profiles publish candidate and generation, vendor/driver/validator/source digests, exact formats/features, resource limits, queues, width/alignment/order, finite work/deadline/lease, reset/loss state, and fence backend. A software fallback is a distinct candidate and receipt. Device loss blocks future admissions and retains work/charges until completion/fence/proof. Audio must use generation-bound candidate resources, authenticated CallbackQueue, finite AudioLease, and `audio.renew`; raw device digest or indefinitely renewable queue is nonconforming.

### H-COMP-010 — Foreign adapters

JVM/CLR/V8/BEAM/Wasm adapters use the exact JPH registry and AdapterContract/1. Managed heap/class-loader/isolate identity is not authority. FFI uses H-ISP-004 typed classes; dynamic reflection or unregistered native calls are build/activation refusal. A pinned GPU/model standard adapter must publish `format_digest`, source/schema digests, complete TypeId projection, descriptor rows, and source/version identity; a name such as “WebGPU” or “ONNX” without these identities is not a valid adapter.

### H-COMP-011 — OCI/process adapters

OCI images/indexes remain distribution projections. A container, rootless namespace, gVisor, Kata, or Firecracker backend must publish principal/channel authority above mutable image state, filesystem/volume identity, rootless/cgroup/network/device filtering, VMM/helper lifecycle, and kill/fence proof. Container namespace and digest do not themselves grant authority or prove security. Missing egress, descendant, or accounting enforcement refuses the required profile.

### H-COMP-012 — Compatibility gates and evidence limits

Every required workload declares exact service rows, limits, authority, callback/affinity, device and isolation requirements. The selected profile either enforces them or refuses before activation. Browser process/raw-socket, mobile JIT, no-MMU hostile isolation, absent GPU/audio deadline, missing Linux no-follow/seccomp/cgroup, and unavailable Windows/macOS limit are visible losing/uncovered cells. These clauses are proposed/documentary; no runtime, performance, escape, or security result is claimed. Host facts remain tied to the cited Linux man-pages, Microsoft, Apple, WHATWG, Web Audio/WebGPU, Zephyr/WAMR, JVM/.NET/V8/BEAM, OCI, and pinned RFC sources already listed by the canonical proposal.


<a id="developer-experience"></a>
## Developer experience

### P-DX-001 — Current commands versus proposed extensions

Current source-inspected ownership remains:

```text
jet run <file.jet> [-- <args>]
jet build <file.jet|dir>
jet build --verify <receipt-id>
jet inspect <existing-plane>
jetpack add|remove|update|lock|outdated|info|explain|why|logs|trust|hangar|service-probe|<provider operation>
```

The following are **PROPOSED EXTENSIONS**, not current commands: `jetpack deps status`, `jetpack deps update`, `jetpack receipt show --json`, `jet inspect --jpm`, `jet inspect --machine`, and `jet debug attach`. They must reuse current command ownership and report unsupported until implemented. `jetpack run`, `jetpack build`, `jetpack test`, and `jetpack fmt` remain retired and are not resurrected.

### P-DX-002 — Authority wording and beginner profile

Current law is described as “manifest-less applications use the compatibility set `IO`, `Mem.Alloc`, and `Exec` through `ApplicationAuthority::ambient_basics`.” It is not described as unqualified deny-by-default. A proposed adoption profile may project only named `IO.Stdio`/argv facts plus explicitly selected `Mem.Alloc`/`Exec`, deny FS/Net/Secret/Device by default, and record the host effective-authority digest. A package manifest supplies requests and policy inputs; it does not create a host grant. The summary therefore says “no ambient host capabilities beyond the compatibility set” until the adoption amendment is ratified.

### P-DX-003 — See/Replace/Refuse precedence

All selection, tier, source-resolution, migration, cache, trust, and rollout overrides use this fixed precedence:

1. Host/verifier safety and current enforcement facts.
2. Immutable locked graph invariants and expected parent/epoch/CAS facts.
3. Explicit user choice (`cli` or an interactive policy decision).
4. Project configuration.
5. Profile/default policy.

The source-to-rank function is closed and derived: `host-policy=0`, `lock=1`, `cli=2`, `project-config=3`, `profile-default=4`; any other source/rank pair is `protocol_error`/`conflict`. Authority effective values are not selectable by a manifest or CLI; the current host `ApplicationAuthority` decision remains authoritative. A requested value conflicting with a higher rank returns `Refuse`/`conflict` and never silently falls back. The rejected override is still recorded as a `SelectionOverrideRecord` and receipt, and its canonical body/digest is included in `ActionBody.override_digest`.

`See` shows selected value, rejected alternatives, exact source, precedence rank, parent digest, authority/trust/route facts, and receipt. `Replace` names a complete alternative and expected parent, then performs the same CAS and verification. `Refuse` leaves all active roots, owned source, and active generation unchanged.

### P-DX-004 — Inspection, receipts, and replay

`jet inspect` keeps source/AST provenance, TIR facts, MIR schema 3, JPM bytes, JPH interfaces, target native output, authority, graph, and receipts as distinct identities. A source-map value can be `available`, `optimized_out`, `not_materialized`, `redacted`, or `unavailable_on_tier`; a debugger never invents a value or treats a host pointer as a machine value. Attach requires debug authority bound to instance/generation/fields and revalidates across generation changes.

A replay artifact binds JPM, graph/lock, tier, schedule decisions, host input bytes, clock/random substitutions, and external-effect records. It replays pure computation and recorded host responses only. An unrecorded network, secret, native-device, or external-database effect returns refusal rather than contacting the live host.

### P-DX-005 — Structured result

```text
OperationResult {
  format: "JPK-RESULT/1",
  ok: Boolean,
  category: "parse" | "resolve" | "integrity" | "trust" | "authority" | "conflict" | "reconcile" | "lifecycle" | "unsupported" |
  stable_detail: String,
  action: ActionId | null,
  receipt: ReceiptId | null,
  input_digests: [Digest],
  required: [String],
  observed: [String],
  retry: "never" | "same-key" | "after-refresh" | "after-reconcile" | "user-choice",
  redactions: [String]
}
```

`stable_detail` is a proposed machine-readable token, not a registered E-code. Host errno, HTTP status, and provider text are nested detail. Effect phase, retractability, partial effect, retry safety, retained charge, quiescence, and residual effects come from the host operation receipt, not from `stable_detail`.

<a id="worked-examples"></a>
## Worked examples

All examples in this section are **PROPOSED EXAMPLES** unless explicitly marked current source-inspected. No command output, digest, readiness result, or runtime behavior is claimed as observed. A JSON object labeled `excerpt` is intentionally abbreviated and is not a complete schema record; a block labeled `complete` includes every required field. Labels such as `Digest@A2`, `Receipt@R1`, `Generation@G1`, and `AuthoritySnapshot@S1` are symbolic typed placeholders, never computed digests or observed runtime values.

### P-EX-001 — Current-syntax beginner package and logical lock

The current source-inspected first program remains:

```jet
// Current source-inspected example; stdout is not claimed here.
print("hello, world")
```

The current command is:

```text
jet run Examples/features/basics/hello.jet
```

A complete proposed `package.jet` uses current Jet syntax and parser-owned fields, not JSON:

```jet
name: "hello-tool"
version: "0.1.0"
edition: "2026"
packages: {
    hello_tool: executable,
}
deps: {
    greeter: "1.4.0",
}
targets: { machine: { profile: "hosted" } }
outputs: {
    run: .Executable{ entry: run },
}
authority: {
    holds: {
        allow: [IO, Mem.Alloc, Exec],
    },
}
```

A logical lock projection is separate from `package.jet`; it is not presented as current parser input:

```json
{
  "format": "JPK-LOCK/1",
  "graph_digest": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
  "nodes": [
    {
      "node_id": "node:sha256:2222222222222222222222222222222222222222222222222222222222222222",
      "name": "greeter",
      "manifest_digest": "sha256:4444444444444444444444444444444444444444444444444444444444444444",
      "source": {
        "kind": "registry",
        "locator": "registry:main/greeter/1.4.0",
        "tree_digest": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
        "manifest_digest": "sha256:4444444444444444444444444444444444444444444444444444444444444444",
        "upstream": {
          "locator": "registry:main/greeter",
          "revision": "1.4.0",
          "tree_digest": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
          "manifest_digest": "sha256:4444444444444444444444444444444444444444444444444444444444444444",
          "license_digest": "sha256:5555555555555555555555555555555555555555555555555555555555555555"
        },
        "license_digest": "sha256:5555555555555555555555555555555555555555555555555555555555555555",
        "dirty": false
      },
      "semantic_bundle_digest": "sha256:1818181818181818181818181818181818181818181818181818181818181818",
      "selected": {
        "raw_reference": "greeter:1.4.0",
        "selected_version": "1.4.0",
        "channel_snapshot_digest": "sha256:9999999999999999999999999999999999999999999999999999999999999999",
        "source_digest": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
        "artifact_digest": "sha256:6666666666666666666666666666666666666666666666666666666666666666"
      },
      "outputs": ["sha256:6666666666666666666666666666666666666666666666666666666666666666"],
      "action_receipts": ["receipt:sha256:7777777777777777777777777777777777777777777777777777777777777777"],
      "receipt": "receipt:sha256:7777777777777777777777777777777777777777777777777777777777777777"
    }
  ],
  "platforms": [
    {
      "profile_digest": "sha256:8888888888888888888888888888888888888888888888888888888888888888",
      "os": "linux",
      "architecture": "x86_64",
      "abi": "jph1",
      "engine_tiers": ["reference", "jit", "aot"],
      "jph_interface_digests": ["sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
      "feature_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      "numeric_semantics_digest": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
      "resource_limits_digest": "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
      "containment_digest": "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
      "native_execution": "denied",
      "device_profile_digest": null
    }
  ],
  "trust_policy_digest": "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  "authority_policy_digest": "sha256:1212121212121212121212121212121212121212121212121212121212121212",
  "trust_snapshot_digest": "sha256:1313131313131313131313131313131313131313131313131313131313131313",
  "toolchains": [],
  "semantic_bundle_digests": ["sha256:1818181818181818181818181818181818181818181818181818181818181818"],
  "owned_source_states": [],
  "generated_at": "2026-09-19T00:00:00Z",
  "writer": {
    "tool_digest": "sha256:1414141414141414141414141414141414141414141414141414141414141414",
    "invocation_digest": "sha256:1515151515151515151515151515151515151515151515151515151515151515"
  },
  "publication_receipt": null
}
```

The sample digest strings are illustrative. A real implementation computes them from the fixed preimages above and must not infer missing fields. The `semantic_bundle_digest` points to the complete graph/Hangar `SemanticBundle` object; the node, lock, and any admission receipt all validate that same object rather than copying a partial Prelude/CoreLib/MIR/numeric subset. `platforms` contains full `TargetProfileRecord` values, and `selected` contains every required selection field.

### P-EX-002 — Owned-source update with concurrent edit

The owned root is `.jet/deps/greeter/`. The updater captures `SourceSnapshot S1` with complete path/type/mode/symlink facts and `local.tree_digest=L1`. It stages upstream `U1`, merges against base `B1`, and finds a `modify/modify` conflict in `greet.jet` plus a protected license conflict. It writes `SourceConflict` records and leaves the live tree and lock unchanged.

Before the user resolves the conflict, an editor changes `README.jet`; a second scan under the commit lease computes `L2 != L1`. The updater returns `conflict/source_concurrent_edit`, keeps the sealed candidate and all bytes user-owned, and does not overwrite `README.jet`. Only after the user chooses a complete side/manual file and a subsequent snapshot matches the expected root may the transaction publish the new source root and lock pair. If no supported source snapshot/write-exclusion primitive exists, the updater returns `unsupported/source_snapshot_unavailable` rather than claiming advisory lock safety.

### P-EX-003 — Migration, backup, and unknown result

Initial state (excerpt):

```json
{
  "volume_id": "vol-orders",
  "mutable_epoch": 12,
  "schema_version": "orders.v1",
  "backup_refs": [],
  "state": "attached"
}
```

A planned irreversible migration is complete only with a tagged backup field; the following is a planned-record excerpt:

```json
{
  "migration_id": "migration:orders-v1-v2-0001",
  "volume_id": "vol-orders",
  "expected_source_epoch": 12,
  "from_schema": "orders.v1",
  "to_schema": "orders.v2",
  "preflight_digest": "sha256:1616161616161616161616161616161616161616161616161616161616161616",
  "backup": { "kind": "none" },
  "idempotency_tuple_digest": "sha256:1717171717171717171717171717171717171717171717171717171717171717",
  "idempotency_key": "sha256:2727272727272727272727272727272727272727272727272727272727272727",
  "idempotency_label": "orders:migrate:12:orders.v1:orders.v2",
  "compatibility": "unknown",
  "state": "planned",
  "external_effect": "none",
  "reconciliation_receipt": null,
  "resulting_epoch": null
}
```

The planner refuses before stopping the old service because the policy requires a backup. The operator creates this complete record:

```json
{
  "backup_id": "backup:sha256:1818181818181818181818181818181818181818181818181818181818181818",
  "body": {
    "volume_id": "vol-orders",
    "source_epoch": 12,
    "schema_version": "orders.v1",
    "snapshot_digest": "sha256:1919191919191919191919191919191919191919191919191919191919191919",
    "content_digest": "sha256:2020202020202020202020202020202020202020202020202020202020202020",
    "complete_marker_digest": "sha256:2121212121212121212121212121212121212121212121212121212121212121",
    "encryption_key_ref": "secret:orders-backup",
    "key_version": "4",
    "consistency": "application-consistent",
    "created_at": "2026-09-19T00:00:00Z"
  },
  "state": "complete",
  "restore_receipt": null
}
```

The migration changes `backup` to `{ "kind":"present", "backup_id":"backup:..." }`; the immutable backup identity is part of `OperationTuple/1.input_digest` and `expected_parent_or_epoch`, so the tuple/key is recomputed from that exact body rather than conditionally. It obtains a storage `ExternalFenceBarrier/1` or operations-owned `QuiescenceProof`, CAS-compares volume ID/epoch/fence (12), creates epoch 13, and records `committed`. A reply lost after an external database commit produces (excerpt):

```json
{
  "state": "unknown",
  "external_effect": "unknown",
  "reconciliation_receipt": null,
  "resulting_epoch": null
}
```

No retry runs the migration or restores the backup until reconciliation supplies the durable external state. A code rollback may be selected only after compatibility or verified restore; it records `external_effects_undone=false` when the database effect remains.

### P-EX-004 — Candidate/readiness/routing activation

Generation A is recorded with endpoint `svc:orders`, volume epoch 12, and secret version 4. Generation B is staged with graph/lock digests, `AuthoritySnapshot@S1 = {application_authority_digest: Digest@A2, authority_digest: Digest@E8, revocation_epoch: 9, host_import_decision_receipt: Receipt@R-auth}`, secret version 5, and a candidate pointer. The host prepares and spawns B, compares all four snapshot members at channel creation, proves the declared health/readiness predicate, establishes volume/endpoint/secret fence epoch 9 and an `ExternalFenceBarrier/1`, and obtains a route-preparation receipt. It then attempts the expected-old local pointer CAS and backend route commit described by one `ExternalCutoverBody/1`; if no external cutover has occurred, A and the targets remain unchanged, while any later route/fence/commit state is determined from its receipts rather than assumed atomic.

If B crashes before readiness and no external cutover/fence has occurred, recovery fences/stops B and retains the unchanged A state. If the local pointer/route transaction cannot be proved or the current pointer is not A, recovery records `conflict` without claiming that A is active; if a remote route/volume/secret commit reply is lost, B is fenced/quarantined and the cutover is `unknown` until the actual backend state is reconciled. Recovery then uses an expected-current CAS to complete B or restore A only when data, schema, and authority safety checks pass. It does not expose B merely because `candidate_pointer=B` exists.

### P-EX-005 — Native/GPU logical projection, not current package.jet

The following is a **logical proposed graph projection**, not a runnable `package.jet` file. New fields are intentionally not inserted into current Jet syntax. Its schema is `ManifestProjection/1`: `{format, source_manifest_digest, portable_entry, native_libraries[{name,artifact,os,architecture,abi,trust_transition}], host_services[{interface,required_features,fallback}], authority_requests, ingestion_action, ingestion_receipt}`. `IngestionAction` hashes the projection plus source/portable/native/host-service digests and outputs the graph/artifact/adapter records; `ingestion_receipt` records trust transition and refusal facts. It is a graph ingestion record, not a parser-owned manifest.

```json
{
  "format": "JPKG-MANIFEST-PROJECTION/1",
  "source_manifest_digest": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
  "portable_entry": "sha256:2323232323232323232323232323232323232323232323232323232323232323",
  "native_libraries": [
    {
      "name": "filter-c",
      "artifact": "sha256:2424242424242424242424242424242424242424242424242424242424242424",
      "os": "linux",
      "architecture": "x86_64",
      "abi": "c-safe-scalar-text-v1",
      "trust_transition": "explicit-native"
    }
  ],
  "host_services": [
    {
      "interface": "jet:gpu/compute@1",
      "required_features": ["f32", "storage-buffer"],
      "fallback": "portable-cpu"
    }
  ],
  "authority_requests": ["GPU.Discover", "GPU.Submit", "FS.Read", "FS.Write"],
  "ingestion_action": "action:sha256:2525252525252525252525252525252525252525252525252525252525252525",
  "ingestion_receipt": "receipt:sha256:2626262626262626262626262626262626262626262626262626262626262626"
}
```

The corresponding current-syntax source manifest would contain only parser-owned fields, for example:

```jet
name: "image-filter"
version: "2.0.0"
edition: "2026"
outputs: {
    run: .Executable{ entry: run },
}
targets: { machine: { profile: "hosted" } }
authority: {
    holds: {
        allow: [FS, GPU, IO, Mem.Alloc],
    },
}
```

The projection ingestion action records the native/GPU artifact and adapter trust transition. A signature does not grant GPU or filesystem authority. Missing adapter, feature, executable-memory policy, entitlement, or native trust transition returns `unsupported`, `unavailable`, `denied`, or `conflict` according to the exact fact; it never silently substitutes an untrusted native path.

<a id="machine-example"></a>
### JPM/1 complete small artifact example
This hypothetical artifact is 40 bytes. It defines `run:()->i32`, computes 42, and exports it. The example is a wire walkthrough, not implementation evidence.

Assembly:

```
types:
  sig0 = () -> (i32)
functions:
  f0 : sig0
code f0:
  block0:
    r0 = i32.const 42
    return r0
exports:
  "run" = function 0
```

Complete bytes:

```
4A 50 4D 00 01 00 00 00                         # 0..7 header
01 05 01 00 01 01 00                            # 8..14 types
03 02 01 00                                     # 15..18 functions
09 0A 01 01 00 02 09 00 2A 00 05 02             # 19..30 code
0A 07 01 03 72 75 6E 00 00                      # 31..39 exports
```

The code payload is exactly ten bytes: `01 01 00 02 09 00 2A 00 05 02`. The return reference is one byte `02`: `packed=(index<<2)|kind=(0<<2)|2`, the first flat result slot. Offset accounting is:

| offsets | bytes | meaning |
|---:|---|---|
| 0..7 | 8 | header |
| 8..14 | 7 | type tag/length and payload |
| 15..18 | 4 | function tag/length and payload |
| 19..30 | 12 | code tag/length + 10-byte payload |
| 31..39 | 9 | export tag/length and payload |
| total | **40** | complete artifact |

A return reference `80 80 80 80 08` is not valid in the repaired low-bit format. A reserved kind (`packed & 3 == 3`), a result ref to a future producer, or an index above `0x3FFF_FFFF` is `bad_ssa`.

## Cross-chapter invariants

These invariants bind the machine, host, operations, package, and comparison chapters; they do not create separate authority or lifecycle records.

1. **Host ABI identity and result mapping.** The JPH import identity and adapter-version/mode/contract digest are cross-bound to M-ARCH-017. JPH/1 defines the exact 64-byte call header, scalar/aggregate/Error layouts, 32-byte `MemorySlice` (including its mode names), 68-byte attachments, callback prefix, accepted/completion states, and reentrancy/affinity. Every terminal function reply/completion is the exact declared `Result<T,Error>` variant whose header discriminant/domain/code agree; `AcceptedReply` is control-only and no bare `Error` variant is accepted. Host descriptors expose the parameter/result/MemorySlice/attachment/aggregate/error mapping consumed at link.
2. **Host memory and resource events.** Host snapshot, exclusive lease, staging, atomic commit, retain/delegate/close/revoke, resource identity, and stale/closed typed results follow E.4/E.3's event and identity rules. Mode-1 `exclusive_output` requires the host's enforced lease or broker-owned staging with atomic commit; revalidation alone is insufficient. A device-loss notification alone does not release retained charges/storage, and table/value removal does not close a broker binding. `BrokerToken` is not carried in JPM or guest payloads.
3. **Operations and resource accounting.** The operations/resource-control contract has exactly one `FuelSchedule` and digest; host, broker, native, and device CPU remain separately accounted; helper, output, and codec charges are reserved; host outcomes use `EffectView`/`RetainedCharge`; whole-instance hard trap/deadline/fuel termination and helper cleanup are enforced. Tier transitions are fuel-neutral.
4. **Package graph and semantic identity.** The package graph contains exactly one `SemanticBundle`, its digest, `NativeCacheKey/2`, provider/shim implementation identities, specialized-policy digest, fuel/codec/host-charge digests, JPM/JPH versions, target/features/ABI, and generation. A bare JPM is machine-only; source-bound activation requires the bundle. A stale or corrupt cache is an integrity failure, not a miss.
5. **Plugin and native compatibility.** Existing plugin/native ABI and trust classes remain in force until a separate clean cutover; helper calls are bound to instance generation, and hard termination forbids re-entry or result publication. No shim aliases authority or lifetime.
6. **Comparison and evidence.** Comparison uses the repaired exact machine against standard substrates without excluding unsupported cells. The evidence corpus includes the corrected 40-byte sample, all malformed vectors, FP amendment status, JPH projection, cache/fuel identities, wait replay, and host lease races. Source inspections remain evidence, not runtime proof.

## Machine-specific evidence and status

The machine clauses are proposed protocol text. Source-backed terminology and current-law boundaries come from the captured semantic/Jet contracts; WebAssembly, ISA, managed-runtime, host, deployment, architecture, proof, and failure evidence are inputs for comparison, not proof that JPM has been implemented. In particular, current exact-`Int`/Fraction routes and incomplete scalar-FP law remain adoption gates; this repair chooses the proposed all-tier FP amendment for JPM only and does not silently ratify it as current Jet source law.

<a id="competitors"></a>
## Competitors and incumbent strengths

### Comparison method

The comparison unit is a **complete stack for one job**, not a naked opcode listing. For an HTTP service, the stack includes source/compiler, artifact, runtime, TLS/network boundary, filesystem or database authority, process/container policy, startup path, readiness, and update behavior. For a browser workload, it includes the browser's JS/Wasm/component tier and user-activation/security policy. For an ML workload, it includes model weights, device libraries, transfers, validation, and device-loss behavior. A foreign stack can win a job without being a replacement for JPM/1, and JPM/1 can preserve a foreign stack through a projection without claiming to own its semantics.

### Finite comparison corpus

The canonical comparison universe is a Cartesian product of **46 candidate/configuration rows**, **14 fixed workloads** (`W01_NUM` through `W14_EMBEDDED`), and **6 layers** (`L1_REPRESENTATION`, `L2_LANGUAGE_RUNTIME`, `L3_HOST_ABI`, `L4_ISOLATION`, `L5_PACKAGE_DISTRIBUTION`, and `L6_LIFECYCLE_ORCHESTRATION`): **3,864 cells**. The candidate/configuration table and total status function are canonical in [the performance protocol](#performance); this chapter supplies the complete-stack comparison meaning, not a second corpus.

The approved structural snapshot classifies 700 cells as `adapter-required`, 252 as `device-only`, 140 as `representation-only`, 2,412 as `unavailable`, and 360 as `unknown`. It has zero `comparable` cells, zero denominator-eligible cells, and no observed metric. These are declared applicability/readiness dispositions, not benchmark, conformance, security, or deployment results.

### JVM, HotSpot, and Graal Native Image

**EVIDENCE:** JVMS SE 25 defines hardware/OS-independent class files, typed object/reference execution, verification, dynamic linking, and implementation freedom for memory, GC, and optimization. HotSpot SE 25 documents Serial, Parallel, G1, and ZGC trade-offs. JEP 444 virtual threads are M:N user-mode threads for scale and keep existing tooling; JEP 491 (Release 24) removes nearly all `synchronized` monitor pinning, with native/FFM callbacks and class-loading/initialization waits remaining.

JVMTI/JNI are powerful in-process authority crossings. JEP 486 disables the Java Security Manager, so managed type correctness is not hostile-package isolation. Graal Native Image is target-specific closed-world AOT; reflection, JNI, resources, proxies, and initialization need metadata. **Overlap:** managed services, AOT, libraries, debugging, and lifecycle. **Non-equivalence:** adapted Java still carries Java GC/reflection/runtime semantics; preserve JVM execution as an explicit adapter.


### .NET CLR, ECMA-335, and NativeAOT

**EVIDENCE:** ECMA-335 defines metadata, CIL, CTS, and VES; .NET adds managed references, generational GC, exceptions, reflection, diagnostics, and broad unmanaged interop. AssemblyLoadContext provides dependency/type isolation and unloadability but is not binary or hostile-code isolation.

NativeAOT is self-contained target-specific AOT with no runtime JIT, but trimming closes reflection/dynamic-code paths, generic instantiations affect size, and post-publish managed diagnostics are narrower. Single-file native extraction changes startup and permission assumptions. **Overlap:** managed services, AOT, diagnostics, and libraries. **Non-equivalence:** CLR semantics and native calls remain explicit adapters under the Jet authority/lifecycle wrapper.


### WebAssembly core, Components/WIT, WASI, and runtimes

**EVIDENCE:** WebAssembly 3.0 supplies a normative sectioned binary, structural validation, typed control flow, memories/tables, traps, tail calls, exceptions, GC/SIMD/memory64, and implementation limits. Threads remain proposal-level in the captured standards evidence. Components add WIT worlds, typed imports/exports, resources, composition, and Canonical ABI; parts of the binary/concurrency work remain development-stage. WASI P2 is capability-oriented, and current P3 evidence adds native async `future`/`stream` support; Wasmtime 46 is named as first final 0.3 support.

Wasmtime provides core/components, fuel/epochs, AOT, Pulley, and DWARF/native and guest-debug paths with configuration-dependent fidelity. Wasmer documents interpreter/AOT/JIT and explicit grants but not captured P3/component parity; WAMR supports interpreter/AOT/JIT/tier-up and embedded profiles, while AOT debugging is experimental and multi-thread debugging is unsupported in the cited docs. Preserve validated core, components, WASI, browser reach, and debugging, while stating version, host-call, and package/authority/lifecycle costs. wasmCloud demonstrates integrated components, capabilities, OCI transport, readiness, and deployment. The Spin deployment evidence inspected here is historical and must be labeled as such; it still shows an incumbent integration pattern that JPM must not claim as novel.


### OCI, Docker, containerd, rootless, gVisor, Kata, and Firecracker

**EVIDENCE:** OCI Image v1.1.1 defines manifests, indexes, config, and ordered layers; indexes select OS/CPU variants, not a common syscall/CPU ABI. OCI Runtime v1.2 and containerd Runtime v2 define process lifecycle and shim management. Docker rootless uses user namespaces and subordinate UID/GID ranges. The pinned documentation also records host/driver-dependent or unsupported combinations for AppArmor, checkpoint/restore, overlay networking, SCTP, storage drivers, cgroups, and devices; each such feature remains an explicit `unavailable`/`failing` cell rather than a waived capability.

gVisor supplies a userspace Sentry/Gofer boundary with documented syscall gaps; Kata adds a KVM guest kernel; Firecracker adds KVM, seccomp/cgroups/namespaces/jailer, and VirtIO. Firecracker does not itself filter networking: the host must configure the network namespace, egress/ingress policy, and filtering. Firecracker snapshots need trusted files and do not guarantee connection continuity. Preserve mature image/layer/volume/secret/readiness/rollout and rootless strengths. Costs remain host kernel/architecture dependence, daemon/registry trust, gVisor compatibility, VM boot/device overhead, and mutable external state; export OCI rather than pretend JPM replaces it.


### V8 isolates and ECMAScript

**EVIDENCE:** V8 isolates have separate heaps, contexts provide embedder checks, Inspector supplies a debugger protocol, modules are host-resolved, and termination is distinguishable from ordinary exceptions. The captured V8 evidence is moving/current-context guidance rather than an immutable candidate/configuration: it describes an in-process sandbox on supported 64-bit builds and recommends process isolation for hostile code and side channels, but does not establish a frozen package/authority ABI or current strongest baseline. A performance cell is `unknown`/`unavailable` until a commit, build flags, host profile, and feature configuration are pinned.

Snapshots and code caches are version-sensitive/best-effort. JPM should retain V8/browser adapters and expose origin, quota, user activation, API absence, and JIT denial rather than promise native paths in browsers.


### BEAM/OTP

**EVIDENCE:** OTP 29.1 provides lightweight private-heap processes, mailbox copying, supervision, hot code replacement, BeamAsm JIT on x86-64/AArch64, and tracing. Erlang is dynamically typed; typespecs are not a complete bytecode verifier. NIFs are in-process shared libraries and can corrupt or crash the emulator; default distributed cookies/cleartext are not package authority.

Preserve actor fault isolation, supervision, hot replacement, and observability, but keep them distinct from hostile-code isolation. BEAM remains an explicit managed-process/adapter path with its scheduler, GC, NIF, and code-version costs.


### eBPF and Solana SBPF

**EVIDENCE:** Linux eBPF combines fixed instructions with program-type helpers and symbolic pointer/register state, bounds/alignment/initialization/reference tracking, and complexity limits; kernel/program type matter. SBPF is a distinct 64-bit-slot VM with interpreter, verifier, x86-64 JIT, memory regions, syscalls, and compute metering; versions change features and call encoding, and Solana deliberately lacks general OS APIs.

These systems demonstrate region-aware validation and budgets, not a general portable host ABI. JPM should copy explicit import/object/lifetime/meter separation without inheriting kernel/chain version semantics.


### NaCl/PNaCl and SFI

**EVIDENCE:** NaCl enforced reliable disassembly, alignment/layout, masked indirect targets, immutable code, static validation, service-only host access, and an outer process sandbox; hardware exceptions terminated modules and release binaries lacked interactive debugging. Historical paper figures (~30 MB/s validation; ~5% average/~12% worst SPEC2000 overhead) are not JPM measurements. PNaCl distributed a constrained LLVM subset; Chromium later deprecated NaCl/PNaCl for Wasm without a captured single-cause failure.

SFI's strength is a native-code proof boundary. Costs are validator/toolchain/layout coupling, code-size/cache overhead, C/C++ UB and temporal safety, abrupt traps, outer-sandbox dependence, and debug limits. JPM uses closed typed instructions instead.


### Raw/profiled RISC-V

**EVIDENCE:** RISC-V defines an open ISA family and an EEI for memory/I/O, initial state, exceptions, interrupts, and ECALLs; optional extensions and versions fragment targets. Profiles reduce combinations but explicitly do not define boot, ABI, devices, memory map, discovery, or policy. Raw RV ELF therefore has no intrinsic sandbox, host whitelist, or meter.

A controlled RISC-V guest would need frozen extensions, EEI/ECALL virtualization, memory/trap/control-flow rules, metering, and host ABI. That is another machine contract, not safe execution of arbitrary RV binaries.


### LLVM bitcode and MLIR bytecode

**EVIDENCE:** LLVM offers mature typed SSA, optimizers, targets, and debug tooling, but carries UB, `undef`, poison, `freeze`, evolving instructions/types, and droppable metadata; its verifier is not a sandbox/authority policy. Textual IR has no compatibility promise. MLIR bytecode is versioned and upgradeable, but assumes immutable dialects and does not define execution, traps, memory, imports, termination, or isolation.

“Typed SSA” is therefore insufficient: JPM must close opcodes/types/FP/atomics/traps/imports and define bounded validation. LLVM/MLIR remain backend/adaptor ecosystems, not JPM wire law.


### SPIR-V, PTX, and WebGPU

SPIR-V and PTX are device representations. They encode rich shader/kernel or GPU execution semantics, not portable application filesystem, process, network, authority, package, or lifecycle contracts. WebGPU supplies a browser-mediated adapter/device/feature/limit boundary with command and shader validation, zero-initialization and robust behavior requirements, asynchronous errors, and device loss, but an adapter may be absent and performance/device memory remain host-specific. A JPM/1 GPU service can carry validated SPIR-V/PTX or WebGPU operations behind typed imports, with device/driver/validator TCB, staging/copy/synchronization, and device loss visible. It MUST NOT present a GPU ISA as the general sandbox.


### Blow's bounded criticism

The primary Standup #39 recording says a cross-platform machine-like substrate could support real debuggers and analyzability, and describes WebAssembly as an attempted equivalent that “doesn't quite get there” and is not the default. LambdaConf 2024 asks why generalized libraries are not both compiled and run from VM-like bytecode. These are requirements for inspectability, not evidence of JVM failure, universal Wasm failure, or performance. Current JVM JDWP/JPDA and Wasm DWARF/Chrome DevTools show real but different incumbent debugging paths.

<a id="downside-ledger"></a>
## Downside ledger

Each row has one material downside. “Eliminated by contract” means an exact rule makes the stated failure a conformance failure, not that the entire threat disappears. “Mitigated” names the residual price. “Irreducible” means the competing requirements cannot both be satisfied by the selected design. “Empirically unknown” means the mechanism is specified but its practical result is not measured. Every falsifier is an observable test, not a promise.

| ID / family | Evidence-backed strength and material downside | Exact proposed mechanism or requirement | Residual cost | Disposition | Falsifier or required observation |
|---|---|---|---|---|---|
| L-01 / JPM parser | New parser is new attack surface. | Fixed JPM/1 header, ordered sections, canonical unsigned-LEB32, bounded lengths, reject overlong/overflow before allocation. | Independent implementation, fuzzing, review. | Mitigated | Malformed corpus stays bounded and tier-independent. |
| L-02 / JPM bytes | Typed SSA plus metadata may cost bytes. | Measure code/import/debug/authority/package/receipt and full runtime bytes; make no size claim from code alone. | Tiny programs may exceed Wasm/native images. | Empirically unknown | Cold transfer/install bytes per paired fixture. |
| L-03 / JPM validation | Independent validation costs load time and memory. | Validate sections, SSA, types, CFG, imports, references, bounds, limits, and traps before activation; hints never replace checks. | Validation may dominate tiny cold calls. | Empirically unknown | Cold first-result CPU/wall/peak and hostile limits. |
| L-04 / JPM compiler | Public seam requires a new aligned lowering. | Source/AST -> TIR -> canonical MIR (schema 3) remains semantic authority; MIR-to-JPM preserves facts and Prelude/CoreLib meaning. | Compiler work, cache invalidation, drift risk. | Mitigated | Differential tiers; no private IR silently defines law. |
| L-05 / JPM `Int`/GC | Exact `Int` and managed guests still need runtime machinery. | Preserve arbitrary-precision `Int` above JPM `i64`; use specified guest big-number/library, no host pointer identity or compulsory tracing collector. | Big-number, GC, reflection, exception, and library costs remain. | Irreducible | Exact-Int/managed pairs show outputs, peak memory, GC/library cost; narrowing fails. |
| L-06 / JPM tooling | New source/debug tooling may lag incumbents. | Version source/debug identities; report source, portable/native mapping, optimized-away values, and authority/resource cause. | JDWP, DevTools, CLR, and BEAM may be richer. | Empirically unknown | Debug corpus across tiers. |
| L-07 / JPM ecosystem | Jet starts with less ecosystem. | Use named foreign adapters/projections and existing package/lock/Hangar homes; never imply compatibility without an adapter. | Libraries, providers, IDEs, and operator knowledge remain costs. | Irreducible | Missing dependency/provider is uncovered, not passed. |
| L-08 / JPM cache | Native cache helps warm startup but is not semantic proof. | Bind cache to JPM/interface/compiler/target/ABI/mitigation/policy identities; corrupt or untrusted cache is an integrity failure. | Duplication, invalidation, trusted builders, cold compile. | Identity eliminated; cost unknown | Wrong target/build/policy/revocation vectors and cold/warm metrics. |
| L-09 / JPM browser | Browser reach does not grant OS APIs. | Browser backend exposes origin, quota, activation, API absence, and JIT denial; no raw path/process/device authority. | Lowering/debug transforms and capability gaps. | Mitigated | Browser paired cell reports exact denial/unsupported outcomes. |
| L-10 / JPM ABI | Typed ABI still needs every host adapter. | Snapshot host input into owned immutable bytes before semantic validation unless an immutable/exclusive lease covers use; check lengths first; no raw pointer across suspend/reentry. | Copies, serialization, adapter/error mapping; mutable zero-copy may be partial. | Mitigated | Double-fetch, lease, reentry, stale/forged handle tests; copy bytes measured. |
| L-11 / JPM authority | Revocation cannot undo effects. | Default-deny; receiver-scoped delegation; check open/connect/redirect/spawn; revoke at broker admission; generation-close aliases; bound admitted work. | Packets, secrets, writes, commits remain. Ordinary typed operation errors need not kill instance. | Mitigated; past effects irreducible | TOCTOU/admission/revocation/post-commit-cancel tests. |
| L-11a / failure/plugin | Hard traps cannot safely resume arbitrary shared guest state. | Invalid state, exhausted instance fuel/deadline, failed invariant, or unrecoverable engine failure terminates isolated instance/tasks. Independent requests need own state or recovery. Plugin reuse-after-trap requires explicit amendment/cutover. | Restart/recovery; shared callers not magically preserved. | JPM contract; incumbent behavior unknown | Trap/kill/restart and plugin-lifetime vectors. |
| L-12 / JVM | Mature class files, GC, virtual threads, and JDWP retain dynamic/native authority costs. | The explicit managed-runtime adapter owns a process/authority boundary, class-loading/reflection closure policy, JNI/JVMTI rights, cancellation, and failure receipts. | The adapter still retains GC, dynamic loading, native callback, and target-specific AOT costs; JVM may win a named job. | Empirically unknown | Dynamic-load, native-callback, virtual-thread and debug pairs with the adapter policy bound. |
| L-13 / .NET | CLR/GC/NativeAOT strong; ALC is not hostile isolation. | Require CLR process boundary, NativeAOT trim/reflection closure, extraction and diagnostics policy. | Target artifacts and AOT metadata/debug gaps. | Mitigated | Reflection/JNI/resource closure and cold artifact tests. |
| L-14 / Wasm core | Validated core is strong; host/component/version semantics differ. | Pin core, WIT, WASI P2/P3, runtime, profile, and engine; never use “raw Wasm” as comparator. | Canonical ABI/version/async/thread/host-call coupling. | Mitigated | Cross-runtime feature/version vectors. |
| L-15 / Wasmtime/Wasmer/WAMR | Existing interpreter/JIT/AOT/embedded/debug paths are capable but differ. | Treat caches as trusted target artifacts; exchange portable bytes; record exact feature matrix. | JPM may lose mature startup/tooling. | Empirically unknown | Strongest documented configs on same pairs. |
| L-16 / Spin/wasmCloud | Components/capabilities/OCI/readiness/deploy are documented incumbent integration patterns; detailed Spin evidence is historical. | Pin current versions or label the Spin cell unknown; compare whole stack and name only Jet semantic/package/authority additions. | Incumbent cloud/edge workflow may arrive sooner; Spin current quotas/signing/runtime compatibility remain unknown. | Empirically unknown | Equal deploy/readiness/capability/rollback pairs with historical/current provenance. |
| L-17 / OCI/rootless | OCI has mature image/volume/secret/rollout records; rootless retains explicit kernel/storage/cgroup prerequisites and documented unsupported features rather than all network/checkpoint/device capabilities. | Separate content, signer, authority, freshness, platform, volumes, secrets, lifecycle; bind rootless driver/kernel/cgroup/network profile and export OCI. | JPM cannot remove daemon/registry/kernel/device prerequisites or host-level egress filtering; unsupported rootless features remain unavailable/failing. | Mitigated | Rootful/rootless/OCI full-cost cells with exact backend limits. |
| L-18 / gVisor | Userspace kernel reduces host surface but has syscall gaps and cgroup dependence. | Pin its compatibility/security profile as a strongest comparator. | Workloads may be unavailable or slower. | Empirically unknown | Required native/process cells under pinned gVisor. |
| L-19 / Kata/Firecracker | VM boundary adds a separate kernel/TCB and boot/device/snapshot costs; Firecracker itself does not filter traffic. | Measure guest assets, VirtIO, jailer, host-configured filtering, trusted snapshots, and continuity; no warm-vs-cold trick. | Boot/memory/device/network cost, host-filtering responsibility, and no external rollback. | Mitigated | Cold/warm startup, restore, loss, and boundary receipts. |
| L-20 / V8 | Current 64-bit V8 sandbox is maintained but in-process and embedder-configured. | Require process policy for hostile code plus explicit module, isolate, callback, termination, and origin rules. | V8 may win JS/browser compatibility/tooling. | Mitigated | Current build plus process-separated hostile tests. |
| L-21 / BEAM | Actors/supervision/hot code are strong; dynamic typing/NIFs are not hostile isolation. | Keep actor fault isolation separate; declare NIF and version transitions. | BEAM may win actor fault tolerance and operations. | Irreducible | Actor/failure/hot-upgrade/NIF pairs. |
| L-22 / eBPF/SBPF | Verifier state and deterministic meters are strong but helper/kernel/chain coupled. | Freeze JPM imports, object lifetime, CFG/loop bounds, meter, and interpreter/JIT equivalence independently. | JPM may lose compact specialization. | Irreducible | Packet/map/account/helper/version/meter pairs. |
| L-23 / NaCl/PNaCl | SFI proves native code shape but costs layout, traps, debug, and outer sandbox; PNaCl was deprecated. | Use closed JPM opcodes, explicit control flow, immutable code, and containment. | New compiler/runtime replaces direct native code. | Mitigated | Control-flow corpus plus code/validation/startup metrics. |
| L-24 / RISC-V | Open ISA/profiles are useful targets, not sandboxes. | Freeze guest profile and virtualize EEI/ECALL/memory/traps/control flow/meter, or use only as backend. | Guest becomes another JPM-like machine. | Scope eliminated; backend unknown | Raw ELF hostile load fails; controlled backend measured. |
| L-25 / LLVM/MLIR | Mature ecosystems carry UB/poison, evolving semantics, and dialect freedom. | Close JPM opcodes/types/FP/atomics/traps/imports; keep LLVM/MLIR adapters. | Lose generic IR compatibility; add lowering. | JPM semantics eliminated; ecosystem cost irreducible | Poison/undef/metadata/unknown-dialect vectors. |
| L-26 / SPIR-V/PTX/WebGPU | Validated device paths still vary by adapter, limits, loss, and transfer. | Expose device/features/limits/validation/transfers/loss/software fallback as typed services. | No universal GPU, zero-copy, precision, or latency. | Mitigated | Same model/weights/precision/transfer pairs. |
| L-27 / universal profiles | Browser/mobile/no-MMU/GPU authority and executable-memory rules conflict. | Fail closed on missing mandatory service/limit; profiles never change scalar/type meaning. | Some required jobs remain unavailable. | Irreducible | Full OS/browser/mobile/no-MMU/GPU matrix. |
| L-28 / lifecycle | Generations/readiness/rollback cannot make fleets atomic or undo effects. | Use pins, idempotency, fencing, readiness distinction, migrations, and partition states. | Mixed versions, DB migration, and committed effects remain. | Mitigated | Crash/WAL/migration/partition/rollback receipts. |

<a id="conformance"></a>
## Conformance vectors and malicious cases

This section is a **test specification**, not executed evidence. A conforming implementation has one reference semantics and must report exact expected outcomes. A signed artifact or passing source inspection is not a test result.

### Fixture and receipt identity

Each vector fixes source/package-lock and dataset digests, generator/seed, inputs/concurrency, correctness oracle, failure schedule, services, grants/limits, profile/tier, complete toolchain/host/device versions, flags, artifact/interface/debug/cache hashes, attestations, and cache/install state. The receipt binds these, verifier/compiler IDs, timestamps, transitions, outputs/errors, charges, and observed-versus-expected status.

### Fixed workload contracts

The fourteen workload rows below are complete proposed fixture contracts, not executed receipts. Each `fixture` identity names deterministic input, correctness output, and failure/policy schedules. If an eventual paired-corpus manifest lacks the named immutable source, dataset, model, or oracle bytes, the cell remains `uncovered`/`UNMEASURED`; it is never guessed.

| workload | fixture and deterministic input | correctness oracle | fixed failure/policy input |
|---|---|---|---|
| `W01_NUM` | `NUM-V1`; SplitMix64 dyadic integer/FP draws, 64 × 64 matrix, and radix-2 N = 1024 FFT | exact round-to-nearest-even bits; canonical NaN/zero/subnormal; no ULP relaxation | wrap/checked overflow, divide, NaN, signed-zero, and subnormal cases; no host services |
| `W02_TEXT` | `TEXT-V1`; four fixed valid/malformed UTF-8 objects and lower-ASCII-key transform | exact serialized bytes and typed malformed-input error; no partial output | cancellation after parse item 2; no host services |
| `W03_FILES` | `FILES-V2`; 4,096 × 4,096-byte files, 1 GiB stream, 512 KiB manifest, exact names/content | tree, stream, manifest digests and byte count; no root escape/symlink/reparse | no-follow/beneath-root, mode, durability, and mutation before/after admission |
| `W04_CONC` | `CONC-V1`; 64 workers, queue 128, events `E0..E4095`, cancellation at `E2048`, parked callback/helper | deterministic sum/count plus retained-charge/quiescence state | fixed worker/fairness schedule; unsupported async/thread profile is unavailable |
| `W05_NET` | `NET-V2`; `S-TLS-RFC8448-001`, 1,024 logical request IDs, at most 32 in flight, fixed response schedule | status/body/error/redirect/TLS digests and cancellation boundary | no Internet; revoke-before-admission, cancel-after-acceptance, and denied external redirect |
| `W06_BUILD` | `BUILD-SOURCE-V2`; four-layer 64-module semantic source template, fixed edges/bodies/entry, ten edits | diagnostics, semantic output, and emitted-source digest; compiler/runtime acquisition is cold cost | guest cannot arbitrary-spawn; harness compiler process is separately granted |
| `W07_WEB` | `WEB-V2`; 1,024 task submissions with fixed auth/schema/origin/quota outcomes and browser checkpoints | body/status/error/accessibility/origin digests; explicit denial is not success | browser origin/activation/quota/TLS/JIT policy |
| `W08_GAMES` | `GAMES-V2`; `S-GPU-WGSL-001`, headless 1,920 × 1,080, 10,000 fixed-step frames, 1,024 indexed quads, scripted input/WGSL | frame command/simulation digest, 16,666,667 ns frame bound, device-loss state | same GPU/driver/features/precision; no window-manager substitution |
| `W09_CLI` | `CLI-V2`; exact depth-4 names, 8,192 × 512-byte files, normalize transform, arguments/malformed cases | output tree/index/error digest and argument status; malformed args reject before mutation | cancellation at item 4,096 and fixed retry tuple |
| `W10_DATA` | `DATA-V2`; 10,485,760 × 1,024-byte rows, eight exact columns, null/dictionary/malformed schedules | exact rows/schema/hash and 512 MiB resource bound; spill bytes are observed, not prescribed | malformed row schedule; no hidden data reduction |
| `W11_BACKEND` | `BACKEND-V2`; 32 accounts, 1,000 idempotent transactions, restart at request 499, v1→v2 migration | committed-state/receipt lineage and recovery classification; lost reply remains unknown | same database/service/fencing/quiescence policy; no exactly-once claim |
| `W12_AI` | `MNIST-ONNX-V1`; `S-MNIST-MODEL-001`, `S-MNIST-SAMPLE-002`, `S-MNIST-DATA-001`, public `t10k` images/labels in IDX order 0..9999, batch = 1, selected CPU/GPU lane | reference-prediction equality plus numerical oracle; ground-truth accuracy is reported, never assumed | same device/library/precision/transfer; GPU lane cannot fall back to CPU |
| `W13_GUI` | `GUI-V2`; exactly 1,000 key/pointer/text/activate/undo events, ten accessibility/framebuffer checkpoints, 60 Hz bound | document/output/accessibility/frame digests and per-frame samples | same display/input backend; absent permission is unavailable |
| `W14_EMBEDDED` | `EMBEDDED-V2`; 10,000 32-byte CRC-32C sensor frames, 64 KiB working memory, 1 ms period/deadline | control/output trace, sequence/CRC/deadline state; energy only with a physical meter | no-MMU board/profile required; deadline is checked, not instant effect reclamation |

### Fixed workload recipes

These are proposed fixture contracts, not measurements or implementation coverage. The source identities are recorded in the corpus source registry; generated fixtures and physical profiles still require bound receipts before a cell is comparable.

The shared integer generator is `SPLITMIX64-V1`:
`state=(state+0x9E3779B97F4A7C15) mod 2^64`;
`z=((state xor (state>>30))*0xBF58476D1CE4E5B9) mod 2^64`;
`z=((z xor (z>>27))*0x94D049BB133111EB) mod 2^64`;
output=`z xor (z>>31)`, with every shift logical and every multiply reduced
modulo `2^64`. Byte fixtures use little-endian output and record seed, draw
count, generator version and eventual content digest.

All fixtures have a successful full-work run and separate fault runs. Fault injection MUST NOT replace the successful workload in a performance cell. Input generation and oracle generation occur before the measured run; generated input bytes enter the program at runtime and are digest-bound. Generator/fixture versions and every host/library/device/profile choice enter the receipt. Physical bindings and generated digests are absent here, not implied defaults.

`W01_NUM` (`NUM-V1`) resets SplitMix64 to `0x4A45542D4E554D31` once. Consume draws in this exact array order: `I32A,I32B,I64A,I64B,F32A,F32B,F64A,F64B,M32A,M32B,M64A,M64B,X32,X64`. Each I/F/M array has4096 elements. Each X array has1024 `(real,imag)` pairs, consuming real then imaginary draws. I32 uses low32 bits and I64 all64; signed interpretation is two's complement. F32/M32/X32 use exact `((u>>40)-2^23)*2^-20`; F64/M64/X64 use `((u>>11)-2^52)*2^-49`. Values are exact dyadics in `[-8,8)`, rounded once to the named IEEE format.

For each integer width emit, in increasing index order, wrapping add, wrapping multiply, signed checked add and signed truncating divide of A/B. Checked overflow emits `integer_overflow`; division by zero emits `integer_divide_by_zero`; minimum/-1 emits overflow. For each F width emit A+B, A*B, A/B and a left-to-right dot product initialized to +0. Matrix results are row-major `C[i,j]`, with `i,j,k=0..63` loops and `acc=round(acc+round(MA[i,k]*MB[k,j]))` initialized to +0. No FMA or reassociation.

FFT is unnormalized radix-2 decimation-in-time: bit-reverse ten-bit indexes, then loops `m=2,4,...,1024`, `base=0,m,...,1024-m`, `j=0..m/2-1`. Twiddles are correctly rounded exact real `cos(-2*pi*j/m)` and `sin(-2*pi*j/m)` in the target format; an arbitrary-precision interval oracle refines until one nearest-even value is determined, with exact0/+1/-1 handled symbolically. Do not inherit ambient libm rounding. For `a=X[base+j]`, `b=X[base+j+m/2]`, compute `tr=round(round(wr*br)-round(wi*bi))`, `ti=round(round(wr*bi)+round(wi*br))`, then store componentwise `round(a+t)` and `round(a-t)` using the old a/b. Every primitive rounds nearest-even, gradual underflow is retained, invalid arithmetic yields positive canonical quiet NaN (`0x7fc00000`/`0x7ff8000000000000`), signed-zero follows IEEE arithmetic. A separate edge vector evaluates add/multiply/divide on the ordered Cartesian pairs of `[+0,-0,+min-subnormal,-min-subnormal,+1,-1,+inf,-inf,+qNaN(payload1),-qNaN(payload1)]`; those values do not poison the whole matrix/FFT fixture. Outputs are operation-order little-endian bits or the named error. Native-library tolerance experiments are separate cells; they do not relax this semantic oracle or I9.

`W02_TEXT` (`TEXT-V1`) processes four independent objects in order. Valid input bytes are hex `7b224b6579223a224a657420cea95c6e227d0a` (a JSON object with key `Key`, string value ending in decoded LF, followed by document LF). The three invalid inputs replace `ce a9` with respectively `c3 28`, `e2 28 a1`, `f0 28 8c bc`. Parse one JSON object of unique ASCII keys/string values; reject malformed UTF-8 before parsing, duplicate keys, nonstrings and ASCII-case-fold collisions. Lowercase ASCII keys, preserve decoded Unicode values, sort keys by UTF-8 bytes, serialize compact JSON with only required quote/backslash/control escapes and a final LF. Successful output is hex `7b226b6579223a224a657420cea95c6e227d0a`; malformed cases return `invalid_utf8` without output for that object. Cancellation is a separate run between objects1 and2 (zero-based); completed output0 remains, failed object1 has no output, objects2/3 are not admitted.

`W03_FILES` (`FILES-V2`) creates exactly4096 files. File i has path `d%02u/f%04u.bin` using directory `i//64` and name i, for i=0..4095; thus64 files per directory, not4096 per directory. Each file has4096 bytes. One continuous little-endian SplitMix64 byte stream seeded `0x4A45542D46494C31` supplies file contents in increasing i, then1024 one-MiB chunks of `stream.bin`. The manifest contains4096 records in i order, each128 bytes: NUL-padded UTF-8 relative path[80], size:u64le=4096, mode:u32le=0644, raw SHA256[32], reserved[4]=0. The job streams copies to output and independently hashes every copied file/stream. It emits a matching manifest and a32-byte stream hash. Reads+writes total `2*(16MiB+1GiB+512KiB)+32`, below3GiB. No-follow/beneath-root, mode and journaled durability are required. Separate negative cases replace the selected root before admission, replace it after an immutable snapshot, add a symlink escape, and request `../outside`; expected refusal/no unauthorized write follows the host snapshot/generation contract.

`W04_CONC` (`CONC-V1`) receives jobs0..4095, submits to64 workers through capacity128, computes `u64(job)*job`, joins all and emits results sorted by job ID plus their exact u64 sum. A replay-only failure schedule completes jobs0..2047 before cancellation of further admission. A callback and helper are admitted after job2047 and parked behind explicit release barriers; they remain charged when cancellation returns. Query must report those two outstanding admissions, not quiescence. Release callback completion first, helper fence second; only then may their scoped proof become quiescent. No jobs>=2048 commit a result in this schedule. Performance runs use real concurrency rather than timing this serialized fault schedule; output order remains deterministic.

`W05_NET` (`NET-V2`) uses1024 logical request IDs with at most32 in flight; redirects add actual requests and their cost. The public test RSA key is RFC8448 section2 and the certificate DER is the server Certificate entry in section3. TLS is1.3, AES_128_GCM_SHA256, X25519, ALPN `http/1.1`; no resumption/0RTT. This isolated fixture pins that exact certificate as its test identity and SNI `server`, not production Web-PKI trust/clock/name validation. The publicly disclosed test key must never be used outside the fixture. Fresh handshake randomness is real and not part of the output oracle; handshake bytes are not claimed deterministic.

Requests are GET `/v1/item/<decimal-id>` over a fixture-assigned loopback endpoint. IDs0..255 return200 with65536 bytes `byte[k]=(id+k)&255`. IDs256..511 return302 with empty body and relative Location `/v2/item/<id>`, then the same200 body. IDs512..767 carry `Authorization: Bearer invalid` and return401 with UTF-8 `unauthorized\n`; IDs768..1023 return503 with `unavailable\n`. Other IDs carry `Bearer fixture`. Response headers are Content-Length and, for redirects, Location; no Date or random header enters the semantic oracle. Record `(id,redirect_count,status,SHA256(body))` in ID order. Separate cases revoke before socket admission, cancel after response acceptance, and attempt an external redirect; the last is denied. The actual TLS library/version and socket endpoint are bound host facts, not missing fixture semantics.

`W06_BUILD` (`BUILD-SOURCE-V2`) has64 modules `m<layer>_<index>` for layer0..3,index0..15, each exporting `f_l_j(x:u64)->u64`. Reset SplitMix64 to `0x4A45542D42554931`; in layer/index order draw K then C; `r=(layer+index+1) mod64` uses no draw. All additions wrap64. Layers0..2 compute `rotl64(f_(l+1)_j(x)+f_(l+1)_((j+1)%16)(x xor K_lj),r_lj) xor C_lj`; layer3 computes `rotl64(x+K_3j,r_3j) xor C_3j`. Those are the complete dependency edges. The entry reads runtime integers0..4095; for each x it calls all16 f_0_j in increasing j and XORs their results from initial0, then emits decimal-index, colon, sixteen lowercase result hex digits and LF. Each peer emits its language's direct named-function/module spelling, no precomputed outputs; UTF-8 LF, sorted import/compilation order and exact emitted source bytes are recorded. Incidental cross-language source-line equality is not required.

Ten edit runs each start from the same baseline (invalid edits never leak into a later run): E01 XOR K_3_0 with1, success using the changed oracle; E02 remove m1_0 import from m0_0, unresolved-symbol error; E03 remove then restore that import before building, baseline success; E04 duplicate f_2_3 export in its module, duplicate-symbol error; E05 rename f_3_7 and both references in m2_7/m2_6 to `renamed_3_7`, baseline success; E06 change K_3_0 literal to `0xG`, lexical error; E07 change the addition of the two calls in f_1_4 to XOR, success using the changed oracle; E08 omit x from the f_2_4 call in f_1_4, arity error; E09 remove then restore that argument before building, baseline success; E10 swap independent m3_0/m3_1 compilation-input order, baseline success. Exact peer diagnostic wording/artifact bytes need not equal; category and semantic output must. Cold harness acquisition/build/validation/installation/cache transfer are charged, including compiler process spawn; the resulting guest has no arbitrary-spawn/network grant.

`W07_WEB` (`WEB-V2`) serves a task-list page and1024 task submissions. Origin is the bound loopback HTTPS fixture from W05; wrong origin is literal `https://outside.invalid`. Request body is compact UTF-8 `{"id":N,"title":"Task N"}`, integer N without leading zeros, header `Bearer fixture`. For N mod8:0 succeeds200 and inserts taskN;1 changes bearer to invalid→401;2 removes title→422;3 duplicates id→400;4 adds `"extra":0`→400;5 replaces one title byte with ff→400;6 has a per-request quota0→429;7 changes origin→403. Rejects have no insertion. Bodies are compact `{"id":N,"status":CODE}\n` with keys in that order. Every128 responses the browser displays accepted tasks in increasing ID, with an accessible list and each exact title; checkpoint the normalized `(role,name,value,children)` tree and DOM text, excluding generated backend IDs/timestamps. Final list has128 tasks. Browser activation/origin/CSP/JIT/quota enforcement and exact engine/profile are required; a server-only substitute cannot pass this cell.

`W08_GAMES` (`GAMES-V2`) is headless1920x1080,10000 frames at60Hz,1024 moving indexed quads. For q=0..1023 consume four SplitMix64 draws from seed `0x4A45542D47414D31`: px=u%131072, py=v%131072, vx=w%257-128, vy=z%257-128. Frame input direction is `(frame//30)%4`, giving `(dx,dy)=(1,0),(0,1),(-1,0),(0,-1)`; update px/py by vx+dx/vy+dy modulo131072. Centers are exact f32 `(p-65536)/65536`; vertices are in corner order `(-,-),(+,-),(+,+),(-,+)` with position offsets `(±1/128,±1/128)` and UV `(±1,±1)`. Quad q's six u16 indexes are `4*q+[0,1,2,0,2,3]`. Upload vertex data, begin a color pass cleared opaque black, set pipeline/vertex/index state, draw6144 indices once, end, submit, await fence and read back RGBA8. No depth, blending, multisampling, derivatives or atomics. Vertex shader writes `vec4(position,0,1)` and passes UV; fragment writes `vec4((uv.x+1)/2,(uv.y+1)/2,0.5,1)`. Exact source generated from this program is frozen before runs; S-GPU-WGSL-001 pins the language, not nonexistent fixture shader bytes. Compare exact CPU simulation and command stream; pixel oracle is generated by the pinned graphics reference on the same device/profile, including rasterization rules. Upload/readback/fence cost is included every frame. Device loss and fence timeout are separate fault runs; neither frees live storage by itself.

`W09_CLI` (`CLI-V2`) has8192 files at `d00/d01/d02/d03/f%07u.txt`, i=0..8191. A continuous SplitMix64 stream seed `0x4A45542D434C4931` supplies24 draws per file. Each draw emits `Key=` + sixteen lowercase hex digits + LF (21 bytes); append `Pad=000\n` (8 bytes), totaling512. Parse `ASCII-key=value` LF records, lowercase keys, preserve value bytes and record order. Arguments are `--root=input --out=output --mode=normalize --jobs=16`; write the transformed tree plus manifest lines `relative-path<TAB>64-lowercase-SHA256<LF>` in lexical path order. Separate malformed arguments (missing out, duplicate jobs, noninteger jobs, `../input`, unknown mode) reject before mutation. Cancel after exactly4096 committed files in a replay case; same tuple/input retry checks those outputs before completing the remainder.

`W10_DATA` (`DATA-V2`) has10485760 rows and exactly1024 logical value bytes per row (10GiB). Eight columns are id:u64(8), group:u64(8), measure:f64(8), dict:u32(4), flag:u8(1), value:i32(4), timestamp:u64(8), payload:bytes[983]. Row r has id=r,group=r%64,measure=(r%2048-1024)/8,dict=r%1024,flag=1 iff r%3=0,value=r%2001-1000,timestamp=1700000000000000+1000*r. Value is null iff r%10=0 and its stored integer is then0. A separate validity bitmap, low-bit-first per8 rows, has1 for nonnull; its1310720 bytes are additional physical input, not falsely included in10GiB. Payloads consume a continuous little-endian SplitMix64 byte stream seed `0x4A45542D44415431`, row then byte order. On disk each full column is contiguous in listed column order, then the bitmap; scalars are little-endian. Dictionary table is id0..1023→`key%04u`.

Query: filter flag=1 and id%7=0, group by dict, join the dictionary and output ascending dict with count_rows,count_value,sum_measure,sum_value. Float sums consume ascending id with nearest-even addition/no FMA; value sum uses checked i64, ignores null and emits null only for no nonnull values. Serialize `dict<TAB>key<TAB>count_rows<TAB>count_value<TAB>16hex-f64-sum-bits<TAB>decimal-sum-or-null<LF>`. Three separate corrupted-input runs mutate respectively row1048575 dict to1024, truncate at the start of row5242879's payload, and truncate the final bitmap byte; each errors before publishing result. Successful10GiB work is not replaced by an early-error run. Memory512MiB; scratch-root read/write is granted for spill, actual spill bytes measured, no prescribed algorithm.

`W11_BACKEND` (`BACKEND-V2`) starts32 accounts acct0000..acct0031 with balance1000 and an empty durable request log. Request id0..999 targets account id%32, adds `(id%17)-8`, and stores `(id,account,delta,new_balance)` atomically with the balance. Key is `req%04u`; same-key/same-body replay returns the recorded result, changed-body conflict has no effect. Per-account ID order is preserved; up to32 different accounts run concurrently. Complete requests0..499, lose only the reply for499 after commit, drain/fence old writers, restart, and migrate v1→v2 by adding tx_count derived from the durable log per account. Reconcile/replay499 without repeating its delta, then500..999; v2 increments tx_count atomically. Final balance is1000+sum of deltas for that account and tx_count its number of unique IDs. Receipt records the unknown interval, actual fence and migration, not exactly-once network delivery. Database/service/library and durability backend are equal pinned profile inputs.

`W12_AI` (`MNIST-ONNX-V1`) uses S-MNIST-MODEL-001 and S-MNIST-DATA-001, with pinned sample bundle S-MNIST-SAMPLE-002. Read all10000 images/labels in IDX order; each image is batch1 `1x1x28x28` f32, each pixel converted by nearest-even f32(pixel/255), no shuffle/augmentation/inversion. Run the sole model input and sole `1x10` logits output; ambiguous graph inputs/outputs reject. Argmax ties choose lowest index. Before timing, a pinned ONNX reference implementation/profile generates all10000 reference logits/predictions; its source/build/numerical-library versions and complete output digest must be in the corpus manifest. Missing oracle bytes keep the cell uncovered. Check supplied sample outputs as an independent smoke vector; finite logits must satisfy `abs(actual-reference)<=1e-4+1e-5*abs(reference)`, with NaN/Inf rejected, and all predicted classes must equal reference predictions. Ground-truth labels measure accuracy `correct/10000`; the trained model is NOT required or claimed to predict every label correctly. This peer-library tolerance does not permit Jet tiers to change their selected model execution contract.

Choose exactly CPU or GPU before running. Model loading/preprocessing/transfers and device/library costs remain visible; GPU cannot fall back to CPU. Training is excluded. Model-zoo accuracy and unexecuted source identities are not our observations.

`W13_GUI` (`GUI-V2`) is a document/list editor with100 initial lines `Item %04u` (0..99), selected line0 and cursor0 counted in Unicode scalar values. Ten blocks each have100 events: first40 key presses cycle Home,End,Left,Right (cursor clamped to selected line); next30 pointer moves use x=(17*global_event)%1920,y=(31*global_event)%1080 without changing selection; next20 text insertions insert `Jet Ω <block>-<local-index>` at cursor and advance it; last10 alternate activate-add-line and undo five times. Add appends an empty line/selects it; undo restores the pre-add document, selection and cursor. Event times are global_event/60 seconds. After each block checkpoint exact document/selection/cursor and normalized accessibility tree (window, editable text, list items, add/undo buttons); render the selected document and list, not an idle window. Framebuffer comparison uses the same pinned backend/font/scale reference, not an assumed cross-platform glyph hash. Every presented frame has16,666,667ns deadline;10 full checkpoints and1000 events are required.

`W14_EMBEDDED` (`EMBEDDED-V2`) has64KiB working memory,1ms period/deadline,10000 packets. Each32-byte LE packet: seq:u16@0=i,timestamp_us:u32@2=1000*i,accel_x:i16@6=(i%257)-128,accel_y:i16@8=(3*i%257)-128,accel_z:i16@10=1000,temperature_mC:i16@12=25000+(i%1001)-500,status:u16@14=0,reserved[12]@16=0,CRC32C:u32@28. CRC covers bytes0..27, reflected polynomial0x82F63B78, initial/final XOR0xFFFFFFFF, no byte reversal. Output for each valid on-time sequence is LEu16 `clamp(32768+64*(25000-temperature_mC),0,65535)`; CRC/sequence/deadline failure emits PWM0 plus its named error. Three separate fault runs at i=500 flip one CRC bit, repeat seq499, or deliver after1ms; later seq resumes at501, so loss does not poison all remaining packets. Deadline is a checked criterion, not instant effect reclamation. Energy needs a real meter; unavailable energy is not zero or a pass.

### Profiles, grants, and quotas

Each grant has a unique workload-specific ID and explicitly denies the remainder. Harness/compiler acquisition in `W06_BUILD` is separate from guest permissions.

| workload | required profile | grant ID and body | explicitly denied remainder | quota/deadline |
|---|---|---|---|---|
| `W01_NUM` | `PROFILE-PAIRED-X86-LINUX-V1` | `G-W01-NUM={monotonic_clock,deterministic_random}` | filesystem, network, secrets, process, device, UI, sensor, host pointer | `Q-W01-256MiB-120s`: 256 MiB, 1 CPU, 16 MiB I/O; soft 110 s, hard 120 s |
| `W02_TEXT` | `PROFILE-PAIRED-X86-LINUX-V1` | `G-W02-TEXT={monotonic_clock,deterministic_random}` | filesystem, network, secrets, process, device, UI, sensor, host pointer | `Q-W02-128MiB-30s`: 128 MiB, 1 CPU; soft 25 s, hard 30 s |
| `W03_FILES` | `PROFILE-PAIRED-X86-LINUX-V1` | `G-W03-FILES={input_root_read,output_root_write,monotonic_clock,deterministic_random}` | network, secrets, process, device, UI, sensor, host pointer, undeclared filesystem | `Q-W03-512MiB-3GiB-300s`: 512 MiB, total I/O 3 GiB; soft 270 s, hard 300 s |
| `W04_CONC` | `PROFILE-PAIRED-X86-LINUX-V1` | `G-W04-CONC={monotonic_clock,deterministic_random,task_slots_within_quota}` | filesystem, network, secrets, process, device, UI, sensor, host pointer | `Q-W04-256MiB-64x-120s`: 256 MiB, 64 workers, queue 128; soft 100 s, hard 120 s |
| `W05_NET` | `PROFILE-PAIRED-X86-LINUX-V1` | `G-W05-NET={loopback_client_server,TLS_fixture_S-TLS-RFC8448-001,monotonic_clock,deterministic_random}` | external network, secrets, process, device, UI, sensor, host pointer, undeclared filesystem | `Q-W05-256MiB-32c-120s`: 256 MiB, 32 connections, 64 KiB body; soft 100 s, hard 120 s |
| `W06_BUILD` | `PROFILE-PAIRED-X86-LINUX-V1` | `G-W06-BUILD={guest:source_dependency_lock_read,artifact_write,clock,random; harness:compiler_process_spawn,local_package_fixture_read,cache_read_write}` | guest external network, guest secrets, guest arbitrary process, guest device, guest UI, guest sensor, guest host pointer, undeclared filesystem | `Q-W06-2GiB-600s`: guest/harness 2 GiB, 4 workers; soft 540 s, hard 600 s; compiler acquisition is cold cost |
| `W07_WEB` | `PROFILE-BROWSER-X86-V1` | `G-W07-WEB={declared_origin,user_activation,quota,loopback_client_server,TLS_fixture,clock,random}` | raw filesystem, secrets, process, device, sensor, host pointer, external network | `Q-W07-512MiB-120s`: 512 MiB, 32 requests; soft 100 s, hard 120 s |
| `W08_GAMES` | `PROFILE-GPU-X86-V1` | `G-W08-HEADLESS-GPU={GPU_command,transfer,fence,headless_surface,scripted_input,clock,random}` | display server, window manager, network, secrets, process, UI, sensor, host pointer, undeclared filesystem | `Q-W08-2GiB-60Hz`: 2 GiB; soft frame 16,000,000 ns, hard frame 16,666,667 ns, total hard 180 s |
| `W09_CLI` | `PROFILE-PAIRED-X86-LINUX-V1` | `G-W09-CLI={input_root_read,output_root_write,clock,random,task_slots_within_quota}` | network, secrets, process, device, UI, sensor, host pointer, undeclared filesystem | `Q-W09-512MiB-300s`: 512 MiB, parallelism 16; soft 270 s, hard 300 s |
| `W10_DATA` | `PROFILE-PAIRED-X86-LINUX-V1` | `G-W10-DATA={dataset_input_read,output_artifact_write,scratch_root_read_write,clock,random}` | network, secrets, process, device, UI, sensor, host pointer, undeclared filesystem | `Q-W10-512MiB-600s`: 512 MiB; spill bytes observed if any; soft 540 s, hard 600 s |
| `W11_BACKEND` | `PROFILE-PAIRED-X86-LINUX-V1` | `G-W11-BACKEND={loopback_client_server,durable_volume_read_write,clock,random}` | secrets, process, device, UI, sensor, host pointer, external network, undeclared filesystem | `Q-W11-2GiB-600s`: 2 GiB, 32 connections; soft 540 s, hard 600 s |
| `W12_AI` | `PROFILE-AI-LANE-V1` | `G-W12-AI={model_input_read,selected_CPU_or_GPU_compute,selected_transfer,clock,random}` | network, secrets, process, UI, sensor, host pointer, undeclared filesystem, unselected device | `Q-W12-8GiB-120s`: 8 GiB, batch = 1; soft 100 s, hard 120 s |
| `W13_GUI` | `PROFILE-GUI-X86-V1` | `G-W13-GUI={display_framebuffer,input_accessibility,clock,random}` | raw filesystem, network, secrets, process, sensor, host pointer | `Q-W13-1GiB-60Hz`: 1 GiB; soft frame 16,000,000 ns, hard frame 16,666,667 ns, total hard 180 s |
| `W14_EMBEDDED` | `PROFILE-NOMMU-EMBEDDED-V1` | `G-W14-EMBEDDED={sensor_input,protocol_io,control_output,watchdog,clock,energy_telemetry_if_physical}` | filesystem, network, secrets, process, UI, host pointer, desktop device | `Q-W14-64KiB-1ms`: 64 KiB, period 1 ms, hard deadline 1 ms; energy bound absent without meter |

Whole-workload profile matching is mandatory: `PROFILE-PAIRED-X86-LINUX-V1` covers W01–W06 and W09–W11, `PROFILE-BROWSER-X86-V1` covers W07, `PROFILE-GPU-X86-V1` covers W08, `PROFILE-AI-LANE-V1` selects exactly one CPU/GPU lane for W12, `PROFILE-GUI-X86-V1` covers W13, and `PROFILE-NOMMU-EMBEDDED-V1` covers W14. A source pin or declared profile is not physical readiness; absent host, device, browser, board, energy, model, or generated-fixture receipts remain `unknown`/`unavailable`.

### Required positive and negative vectors

`CV-01` **Envelope and canonical encoding.** Decode the exact `JPM\0`, major `1`, minor `0`, flags `0` header; accept canonical zero and boundary lengths; reject truncated headers, unknown mandatory version, nonzero reserved flags, overlong LEB32, overflow, section length past file end, duplicate singleton sections, invalid order, and trailing unauthenticated bytes. The rejection must be bounded and must not allocate from an attacker-controlled length before the limit check.
`CV-02A` **Canonical JPM identity.** Alter exactly one JPM code byte, import type, or public type identity in an otherwise valid artifact. The expected result is a changed `JPMDigest`, verification/activation receipt identity, and source-bound cache key; link/admission under the old artifact identity is refused.
`CV-02B` **Graph/package identity.** Alter exactly one source/debug digest, authority request, lock/graph policy, or package receipt input while keeping JPM bytes identical. `JPMDigest` MUST remain unchanged; the affected graph/lock/receipt identity and the one canonical `NativeCacheKey/2` change or refuse admission. There is no separate executable key that can hide a graph mutation.
`CV-02C` **Debug-only identity.** Alter only a canonical debug section. The artifact/package identity changes as declared, while the executable result MAY remain equal; a separately named executable-result equality is an observation, not an undefined second key. A valid content digest alone must not authorize execution: admission without the matching package/authority receipt is denied.
`CV-03` **Block-local SSA.** Feed a valid two-instruction block with `r0` produced before `return`, then mutate one operand to use a later result, packed kind `03`, an out-of-range index, a result from another block, or a wrong block-argument arity. The valid input executes; each malformed input is rejected before engine selection with `bad_ssa`/`bad_cfg`, with no start call, host effect, allocation based on the malformed count, or verification receipt claiming success. No native jump target may be manufactured from an integer.
`CV-04` **Arithmetic and traps.** Exercise wrapping/checked integers, divide/shift/conversion traps, arbitrary-precision `Int`/`Fraction`, floor/truncated remainders, F32/F64, NaNs, signed zero, subnormals, FMA/contraction, v128, and scalar fallback. Every result is defined or traps; LLVM poison/undef/host UB is invalid. Source inspection found TIR/MIREval `Int /`-to-`Fraction` and direct truncating helper paths, but did not prove the latter reachable or observe a user failure. Resolve this owning-stream integration gate without choosing new source law.
`CV-05` **Memory and tables.** For each fixture provide a declared memory/table, an exact load/store or table operation, and a boundary mutation: `address+offset` overflow, range one byte beyond max, growth one page beyond maximum, stale generation, wrong element type, or forged host-pointer-looking integer. The expected state is unchanged on every rejection (`memory.grow` returns `-1` without growth where specified); expected failures are `memory_oob`, `table_oob`, `bad_type`, `stale`, stack exhaustion, or profile admission refusal as applicable. A bounded linear-memory pass does not prove C temporal safety; safe-language sema and unsafe-boundary vectors remain separate.
`CV-06` **Concurrency and atomics.** Exercise declared shared memory only, private resource tables, explicit transfer, the closed Atomic carrier family, SeqCst Core operations, fan-out/join, channel cancellation, parked tasks, and transaction one-run semantics. Include a table/value fixture that inserts two live aliases, removes/replaces one entry, grows the table, drops the value, and proves both bindings remain live until their explicit close; receipts/generations/charges/quiescence identify which alias was closed. The Core API has no caller-selected order argument; foreign low-level orderings MUST NOT silently add one. Reject implicit shared heap, non-sendable capture, invalid carriers, task drop without join/use/detach, and prohibited data races. Lock-free Atomic64 is required with no silent lock fallback.
`CV-07` **Host ABI and buffers.** Pass values, handles, callbacks, futures, streams, and cancellation at limits. Every terminal synchronous/async/callback result declares `Result<T,Error>` with the exact JPH result TypeId, 64-byte header outcome, error domain/code, and `Error.flags`/`OperationOutcomeRef`; accepted control replies are not terminal values. Snapshot host input into owned immutable bytes before semantic validation unless an immutable/exclusive lease covers use; checking then copying/re-reading is forbidden. Concurrent writable reads may reject incoherent data. Test stale/forged handles, mutable reuse, returned-resource attachment binding/ownership, reentry, and cancel-after-commit; distinguish typed operation errors from instance termination and retain charges until the matching quiescence/fence proof.
`CV-08` **Authority and TOCTOU.** Admit a directory/volume binding with a no-follow policy, queue a path open, then replace a symlink/reparse target, parent generation, browser permission, DNS policy, or authority epoch before broker admission; separately revoke after admission. Each fixture records `{request_id, caller_context_digest, authority_digest, revocation_epoch, target_generation, expected_root_or_endpoint, effect_phase}` and the exact typed outcome/error flags. The expected pre-admission state is no host effect and `stale`/`denied`/`unavailable`; already-admitted bounded work may finish with its recorded byte/time bound and residual-effect receipt. Secret/network/file access after revocation must not be newly admitted, IDs/generations must not be reused, and lexical `..` cleanup alone is not a falsifier.
`CV-09` **Resource exhaustion and cancellation.** Exhaust guest/broker memory, code/cache, stack, queues, tasks, handles, IO, fuel, and wall/CPU; overflow charges; invoke noncooperative native work. Ordinary errors need not kill a sound instance. Invalid state, exhausted instance fuel/deadline, failed invariant, or engine failure is a hard containment trap terminating the isolated instance/tasks; shared guest heap is not safely resumable. Cancellation is not rollback.
`CV-10` **Cache/compiler/native trust.** Feed a valid JPM with a cache whose single canonical `NativeCacheKey/2` differs in exactly one field: target, compiler/engine, interface, fuel schedule/codec charge, semantic bundle, mitigation/debug policy, provider/shim, or foreign-runtime identity. Mutate a trusted cache, race two builders for one key, map after quarantine without a `CacheReaderLease`, or supply a native adapter without the declared trust transition. Missing cache may compile; corrupt/untrusted cache, key mismatch, publication race, and native authority mismatch must fail as `bad_cache`/integrity/trust errors, not silently fall back to hostile native execution.
`CV-11` **Package, supply chain, and offline state.** Exercise digest/signature/provenance failure, expiry, rollback/freeze, revoked signer/compiler, CAS corruption, offline valid cache, missing dependency, and mutable tag. Freshness, subject digest, lock identity, and execution authorization remain distinct.
`CV-12A` **Local lifecycle and pointer recovery.** For each journal point (`install`, `validate`, `prepare`, `activate`, `ready`, `drain`, `stop`, `fail`, `recover`, `delete`), crash immediately before and after the write, then replay with duplicate retries, lease loss, mixed generations, and readiness timeout. The expected state is either the old complete generation/lock or the new complete pair with matching local receipts; no half-prepared generation becomes active.
`CV-12B` **External unknown migration.** Crash around volume/route/secret/device/backend commit and lose the reply. The expected state is an `ExternalCutoverRecord`/operation record with `unknown` or `fenced`, candidate B quarantined, and A retained only where expected-old backend state is confirmed; reconciliation is required before retry, restore, or route activation. A stop request is not actual exit; helpers remain charged until exit/fencing/quiescence.
`CV-12C` **Quiescence and rollback.** Crash/retry drain, close, helper exit, fence, migration, and code rollback with outstanding calls/callbacks/leases/writers. Require scoped closed-world `QuiescenceProof`, retained charges, residual effects, duplicate-safe idempotency, and explicit poststate. Rollback changes a generation pointer and records residual committed effects; it cannot undo an accepted database/file/network effect.
`CV-13` **Debug and optimized values.** Break at source, portable instruction, host call, async completion, trap, and native lowering; inspect an optimized-away variable, inlined call, tail transfer, generated closure, and foreign stack. The debugger must say unavailable or optimized-away; it must not fabricate a value or source location. Browser DWARF and native transformations are separate vectors.
`CV-14` **Tier/profile parity.** Run every positive and negative vector under every named peer/profile row in the matrix: interpreter, baseline/JIT, AOT/cache, browser backend, JIT-forbidden mobile, no-MMU/embedded, gVisor, rootless, and VM-backed profiles where the row is applicable. If a row cannot execute a required vector, emit `unsupported`/`unavailable` with the missing feature/authority/limit and classify the cell `failing` or `uncovered`; never omit it or create an exclusion profile.
`CV-15` **Failure ownership and zero-copy.** A request is independently killable only with its own state/isolation or a specified recovery protocol. Mutable zero-copy transfers require exclusive ownership/lease; cancellation or trap may leave partial mutation unless an explicit copy-on-write/commit protocol was requested, and the caller must not treat it as completed output. Immutable sealed buffers are the simple default. Existing Wasmtime plugin reuse-after-trap behavior requires an explicit plugin-lifetime amendment and full caller cutover; this proposal does not silently change it.

### Conformance method and threat boundary

Use golden vectors, differential reference execution, metamorphic rewrites, bounded fuzzing, version matrices, and cross-tier receipts. Soundness means accepted bytes obey JPM/host-import rules, not that all source is semantically safe. State attacker, assets, TCB, boundary, and residual failure. Compromised kernel/admin/hardware/driver/trusted compiler and cache/timing/power channels remain outside these claims; revocation cannot retract read secrets or issued effects.

<a id="performance"></a>
## Performance protocol and gates

No measurement in this draft is an actual result. The protocol below is the only acceptable basis for a later performance claim.

### Canonical finite corpus

The canonical corpus has exactly 46 candidate/configuration rows, 14 workload rows, and 6 comparison layers. Its expansion is the Cartesian product `46 × 14 × 6 = 3,864`; every cell joins one candidate, one nonempty `source_ids` list, one configuration, one workload, one layer, one intended shape, one readiness/status result, one reason, one falsifier, and one absent-observation projection. A representation, device, or orchestration row never silently becomes a complete runtime.

`candidate_kind` distinguishes executable runtimes, format specifications, orchestration layers, historical entries, device representations, and the owned JPM proposal. `config_state` is one of `pinned_configured`, `source_pinned_config_unbound`, `source_moving_config_unbound`, `historical_only`, `spec_representation`, `device_representation`, or `owned_proposal`. `source_ids` is a nonempty list of source-record foreign keys. `P14`, `A14`, `DEVSP`, and `WEBGPU` shape vectors are intended overlap only; they are not coverage or measurements.

| candidate_id | candidate_kind | family_id | implementation/version identity | source_ids | config_id | config_state | shape_vector |
|---|---|---|---|---|---|---|---|
| `rust-1.98.1-release` | executable | `F_RUST` | rustc 1.98.1 default release, 2026-09-03, x86_64-unknown-linux-gnu | [`S-RUST-REL-001`,`S-RUST-MANIFEST-002`,`S-RUST-CHECKSUM-003`,`S-RUST-PROFILE-004`,`S-RUST-PLATFORM-005`] | `CFG-RUST-1981-RELEASE` | `pinned_configured` | `P14` |
| `rust-1.98.1-optimized` | executable | `F_RUST` | rustc 1.98.1 optimized lane, same target/source | [`S-RUST-REL-001`,`S-RUST-MANIFEST-002`,`S-RUST-CHECKSUM-003`,`S-RUST-PROFILE-004`,`S-RUST-PLATFORM-005`] | `CFG-RUST-1981-OPTIMIZED` | `pinned_configured` | `P14` |
| `jpm1-reference` | executable | `F_JPM` | proposed JPM/1 reference interpreter | [`S-JET-PROP-V3`] | `CFG-JPM-REFERENCE` | `owned_proposal` | `P14` |
| `jpm1-jit` | executable | `F_JPM` | proposed JPM/1 JIT | [`S-JET-PROP-V3`] | `CFG-JPM-JIT` | `owned_proposal` | `P14` |
| `jpm1-aot` | executable | `F_JPM` | proposed JPM/1 AOT/cache | [`S-JET-PROP-V3`] | `CFG-JPM-AOT` | `owned_proposal` | `P14` |
| `jvm-hotspot-se25-unbound` | executable | `F_JVM_HOTSPOT` | JVMS SE 25/HotSpot | [`S-MANAGED-001`] | `NONE-UNBOUND-JVM-HOTSPOT` | `source_moving_config_unbound` | `A14` |
| `jvm-graal-native-image-unbound` | executable | `F_GRAAL_NATIVE_IMAGE` | Graal Native Image latest context only | [`S-MANAGED-001`] | `NONE-UNBOUND-GRAAL` | `source_moving_config_unbound` | `A14` |
| `clr-net-unbound` | executable | `F_CLR` | ECMA-335/.NET CLR | [`S-MANAGED-001`] | `NONE-UNBOUND-CLR` | `source_moving_config_unbound` | `A14` |
| `nativeaot-unbound` | executable | `F_NATIVEAOT` | .NET NativeAOT | [`S-MANAGED-001`] | `NONE-UNBOUND-NATIVEAOT` | `source_moving_config_unbound` | `A14` |
| `wasm-core-3.0` | format_spec | `F_WASM_CORE` | WebAssembly Core 3.0 representation | [`S-WASM-001`] | `CFG-WASM-CORE-30` | `spec_representation` | `A14` |
| `wasm-component-wit` | format_spec | `F_COMPONENT_WIT` | Component Model/WIT representation | [`S-WASM-001`] | `CFG-COMPONENT-WIT` | `spec_representation` | `A14` |
| `wasi-p2` | format_spec | `F_WASI_P2` | WASI Preview 2 capability interface | [`S-WASM-001`] | `CFG-WASI-P2` | `spec_representation` | `A14` |
| `wasi-p3` | format_spec | `F_WASI_P3` | WASI Preview 3 capability/async interface | [`S-WASM-001`] | `CFG-WASI-P3` | `spec_representation` | `A14` |
| `wasmtime-46-default` | executable | `F_WASMTIME` | Wasmtime v46.0.0, `423be7a4e4d30bb377c836d317521d1eb874e157` | [`S-WASM-WMT-001`] | `CFG-WASMTIME-46-DEFAULT` | `source_pinned_config_unbound` | `A14` |
| `wasmtime-46-pooling` | executable | `F_WASMTIME` | Wasmtime v46.0.0 pooling allocator | [`S-WASM-WMT-001`] | `CFG-WASMTIME-46-POOLING` | `source_pinned_config_unbound` | `A14` |
| `wasmer-7.4.2-default` | executable | `F_WASMER` | Wasmer v7.4.2, `7a48a071c7682a409d148cf37dc8da58322a123d` | [`S-WASM-WMR-002`] | `CFG-WASMER-742-DEFAULT` | `source_pinned_config_unbound` | `A14` |
| `wasmer-7.4.2-cranelift` | executable | `F_WASMER` | Wasmer v7.4.2 Cranelift | [`S-WASM-WMR-002`] | `CFG-WASMER-742-CRANELIFT` | `source_pinned_config_unbound` | `A14` |
| `wamr-2.4.5-fast-interp` | executable | `F_WAMR` | WAMR-2.4.5, `25bd7eb63e828e4bd242cc9b38d260b4b31c6605` | [`S-WASM-WAMR-003`] | `CFG-WAMR-245-FAST-INTERP` | `source_pinned_config_unbound` | `A14` |
| `wamr-2.4.5-aot` | executable | `F_WAMR` | WAMR-2.4.5 `wamrc` AOT + `iwasm` | [`S-WASM-WAMR-003`] | `CFG-WAMR-245-AOT` | `source_pinned_config_unbound` | `A14` |
| `wamr-2.4.5-tiered` | executable | `F_WAMR` | WAMR-2.4.5 JIT/fast-JIT tier-up | [`S-WASM-WAMR-003`] | `CFG-WAMR-245-TIERED` | `source_pinned_config_unbound` | `A14` |
| `spin-4.1.0-local` | executable | `F_SPIN_LOCAL` | Spin v4.1.0 peeled `c0b3726aa4857961e20cf8616a0df5f0741af73d` | [`S-WASM-SPIN-004`] | `CFG-SPIN-410-LOCAL` | `source_pinned_config_unbound` | `A14` |
| `spinkube-operator-0.6.1` | orchestration | `F_SPIN_OPERATOR` | Spin Operator v0.6.1 peeled `96c9d1d1d69c2c04e005375c44f323fb10e31002` | [`S-WASM-SKUBE-005`] | `CFG-SPINKUBE-061` | `source_pinned_config_unbound` | `A14` |
| `spin-historical-sip` | historical_executable | `F_SPIN_HISTORICAL` | SIP-001/SIP-007/SIP-011, 2022/2023 | [`S-WASM-SPIN-004`] | `NONE-HISTORICAL-SPIN-SIP` | `historical_only` | `A14` |
| `wasmcloud-2.9.0-runtime` | executable | `F_WASMCLOUD` | wasmCloud v2.9.0, `68ebece9c537f8bb4b5c9999f274ec68d60f35a9` | [`S-WASM-WCLOUD-006`] | `CFG-WASMCLOUD-290` | `source_pinned_config_unbound` | `A14` |
| `oci-image-1.1.1` | format_spec | `F_OCI_IMAGE` | OCI Image v1.1.1 | [`S-DEPLOY-OCI-011`] | `CFG-OCI-IMAGE-111` | `spec_representation` | `A14` |
| `oci-runtime-1.2.0` | orchestration | `F_OCI_RUNTIME` | OCI Runtime v1.2.0 | [`S-DEPLOY-OCI-011`] | `CFG-OCI-RUNTIME-120` | `spec_representation` | `A14` |
| `docker-rootful-unbound` | orchestration | `F_DOCKER_ROOTFUL` | Docker rootful runtime, exact release/config absent | [`S-DEPLOY-OCI-011`] | `NONE-UNBOUND-DOCKER-ROOTFUL` | `source_moving_config_unbound` | `A14` |
| `docker-rootless-7d6c8` | orchestration | `F_DOCKER_ROOTLESS` | Docker rootless docs commit `7d6c8bf81ab88fc6f5c4893b6259864d29de574c` | [`S-DEPLOY-ROOTLESS-007`] | `CFG-DOCKER-ROOTLESS-DOCS` | `source_pinned_config_unbound` | `A14` |
| `containerd-2.4.0-v2` | orchestration | `F_CONTAINERD` | containerd v2.4.0 peeled `a7fe631d96c08fb14cf8eff0afdc280e99c30a94` | [`S-DEPLOY-CONTD-010`] | `CFG-CONTAINERD-240-V2` | `source_pinned_config_unbound` | `A14` |
| `gvisor-unbound` | orchestration | `F_GVISOR` | gVisor Sentry/Gofer, release/config absent | [`S-GVISOR-001`] | `NONE-UNBOUND-GVISOR` | `source_moving_config_unbound` | `A14` |
| `kata-4.2.0` | orchestration | `F_KATA` | Kata v4.2.0, `c7351e797efff8bfc6bd73da0eb1909be12e2cfe` | [`S-DEPLOY-KATA-009`] | `CFG-KATA-420` | `source_pinned_config_unbound` | `A14` |
| `firecracker-1.17.0` | orchestration | `F_FIRECRACKER` | Firecracker v1.17.0 peeled `95f868c8e345b1cc8faccd1a3c910b4989dc3f58` | [`S-DEPLOY-FC-008`] | `CFG-FIRECRACKER-1170` | `source_pinned_config_unbound` | `A14` |
| `v8-isolates-unbound` | executable | `F_V8` | V8 commit `491e7b14177fa914ece280f91fc0c8dfc5b2aa37`, exact feature/config absent | [`S-V8-001`] | `NONE-UNBOUND-V8` | `source_pinned_config_unbound` | `A14` |
| `beam-otp-29.1-unbound` | executable | `F_BEAM` | OTP 29.1 source/config not immutably captured | [`S-BEAM-001`] | `NONE-UNBOUND-BEAM-291` | `source_moving_config_unbound` | `A14` |
| `linux-ebpf-b72e29e-config-unbound` | executable | `F_LINUX_EBPF` | Linux commit `b72e29e0f7ee329d89f86db8700c8ea99b4a370a`, verifier/config absent | [`S-EBPF-001`] | `NONE-UNBOUND-EBPF-B72` | `source_pinned_config_unbound` | `A14` |
| `solana-sbpf-unpinned` | executable | `F_SOLANA_SBPF` | Solana SBPF moving `main`, no immutable commit returned | [`S-SBPF-001`] | `NONE-UNBOUND-SBPF` | `source_moving_config_unbound` | `A14` |
| `nacl-historical` | historical_executable | `F_NACL` | historical NaCl SFI paper | [`S-NACL-001`] | `NONE-HISTORICAL-NACL` | `historical_only` | `A14` |
| `pnacl-historical` | historical_executable | `F_PNACL` | historical PNaCl paper | [`S-PNACL-001`] | `NONE-HISTORICAL-PNACL` | `historical_only` | `A14` |
| `sfi-historical` | historical_executable | `F_SFI` | historical SFI model evidence | [`S-NACL-001`] | `NONE-HISTORICAL-SFI` | `historical_only` | `A14` |
| `riscv-raw-v20240411` | format_spec | `F_RISCV_RAW` | RISC-V unprivileged ISA v20240411 | [`S-RISCV-001`] | `CFG-RISCV-RAW-20240411` | `spec_representation` | `A14` |
| `riscv-profile-v1.0` | format_spec | `F_RISCV_PROFILE` | RVA20/RVI20/RVA22 profile v1.0 | [`S-RISCV-PROFILE-002`] | `CFG-RISCV-PROFILE-10` | `spec_representation` | `A14` |
| `llvm-bitcode` | format_spec | `F_LLVM` | LLVM bitcode/IR | [`S-LLVM-001`] | `CFG-LLVM-BITCODE` | `spec_representation` | `A14` |
| `mlir-bytecode` | format_spec | `F_MLIR` | MLIR bytecode | [`S-MLIR-001`] | `CFG-MLIR-BYTECODE` | `spec_representation` | `A14` |
| `spirv-device` | device_representation | `F_SPIRV` | SPIR-V device representation | [`S-SPIRV-001`] | `CFG-SPIRV-DEVICE` | `device_representation` | `DEVSP` |
| `ptx-device` | device_representation | `F_PTX` | NVIDIA PTX device representation | [`S-PTX-001`] | `CFG-PTX-DEVICE` | `device_representation` | `DEVSP` |
| `webgpu-device` | device_representation | `F_WEBGPU` | WebGPU adapter/device API | [`S-WEBGPU-001`] | `CFG-WEBGPU-DEVICE` | `device_representation` | `WEBGPU` |

### Paired workload set

Use the existing canonical paired corpus/manifests as the executable home; this draft defines obligations rather than a second benchmark catalog. Every pair fixes source revision, compiler flags, target, dataset generator/version/seed/content digest, input count, concurrency, external service versions, correctness oracle, failure/cancellation schedule, security policy, authority grants, quotas, and installation state. The pinned candidate IDs and exact source/configuration anchors are listed in [Sources](#sources) under the frozen peer manifest. A peer is selected from a frozen manifest, not from an unpinned “strongest” label:

```text
peer_manifest = {
  peer_id, family_id, implementation, version_or_commit, source_url,
  configuration_id, default_or_optimized, compiler_id, runtime_id,
  flags_features, target_cpu_isa_os, host_profile, isolation_backend,
  package_install_scope, cache_state, grants_digest, limits_digest,
  security_policy_digest, attacker_assets, tcb_digest,
  known_unsupported_workloads, historical_or_current
}
```

The optimized and useful-default rows MUST each name this manifest and be frozen before measurement. Moving `HEAD`, historical docs, or an unpinned branch is `unknown`/`unavailable` until refreshed and cannot serve as a current strongest baseline. A peer must perform the same work with no weaker security policy, lower durability, omitted validation, hidden precomputation, or uncharged runtime provisioning. For a passing pair, attacker model/assets, security policy, TCB, typed enforcement backend identity/version/digest, isolation strength, lifecycle/durability policy, and cost policy MUST byte-match; otherwise the cell is `inconclusive`/`failing`.

| Cell | Paired program and required observation |
|---|---|
| Numerics | Integer wrapping/checked arithmetic; F32/F64 vector, matrix, and FFT work separately; exact outputs or predeclared error bounds; no relaxed math unless both sides use the same contract. |
| Text | UTF-8 validation and parse-transform-serialize with malformed inputs; hash exact output and errors. |
| Files | Many small files and streaming large files; same no-follow/path policy, durability, volume, and authority. |
| Concurrency | Fan-out/join, channel contention, cancellation, outstanding work, queue/fairness, and OS worker accounting. Include JVM virtual threads after JEP 491, BEAM actors, CLR tasks, Wasm async whenever the peer advertises it; if async/thread support is absent, retain the peer row with `unavailable`/`failing` and the exact missing feature. |
| Networking | Bounded request/response and streaming service; same transport/TLS/client generator, redirects, cancellation, errors, and network policy. |
| Build/run | Cold source-to-runnable, repeated edit-to-verdict/build, cold new-host artifact-to-first-result, and warm native-cache run. Include validation, compiler/runtime acquisition, installation, and cache transfer. |
| Web | Authenticated request pipeline, validation, static/content response, errors, server work, and browser tier. Record user activation/quota/API denials. |
| Games | Deterministic fixed-step simulation plus render command production; frame latency, allocation, upload/readback/fences, and required GPU work. |
| CLI/scripts | Recursive file/index transformation, argument/error handling, bounded parallelism, time-to-first-use, and repeat invocation. |
| Data | Fixed dataset parse/filter/group/join/aggregate; output correctness, peak memory, and spill behavior. |
| Backend | Durable request/transaction service with cancellation, restart, migration, readiness, rollout, and recovery; throughput and tail latency. |
| AI/ML | Same model weights, preprocessing, precision, device library, transfers, cache, and OOM/device-loss behavior. A fast native library binding is not itself a language/runtime win. |
| GUI | Interactive document/list editing and drawing with input/accessibility behavior; event-to-frame latency and memory, not an idle window. |
| Embedded | Periodic sensor/protocol/control loop under declared memory/deadline/energy limits, including no-MMU classification and denied services. |

### Family × required-workload applicability matrix

This is a **PROPOSED proof-obligation matrix**, not a shipping or status report. Each cell names the comparison shape: `P` = direct overlap and a direct proof is required; `A` = overlap only through a named adapter/projection and the conversion, authority, lifecycle, and full-cost proof is required; `X` = reasoned semantic non-overlap of the representation itself, with the reason recorded rather than called a loss; `U` = the selected environment cannot provide the required operation, which is a failing or uncovered cell, never a waiver. A cell initially marked `P` or `A` becomes `U` for a claimed profile when the profile cannot execute it. Columns are the required workloads in the paired set: Num, Text, Files, Conc, Net, Build, Web, Games, CLI, Data, Backend, AI, GUI, Embedded.

| Family/configuration | Num | Text | Files | Conc | Net | Build | Web | Games | CLI | Data | Backend | AI | GUI | Embedded | Applicability/proof reason |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Rust control (fixed named toolchain/config) | P | P | P | P | P | P | P | P | P | P | P | P | P | P | P | Explicit control configuration; absent physical/profile/corpus binding is `unknown`/`unavailable`, never silently omitted. |
| JPM/1 reference/JIT/AOT | P | P | P | P | P | P | P | P | P | P | P | P | P | P | Direct selected machine; prove every cell and tier. |
| JVM/HotSpot/Graal | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Managed runtime adapter; preserve GC/reflection/JNI and measure full provisioning. |
| .NET CLR/NativeAOT | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Managed/AOT adapter; preserve CTS/GC/interop and trimming costs. |
| Wasm core | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Core module plus declared host/WASI adapter; validate each import. |
| Components/WIT/WASI | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Component projection; prove canonical ABI, async, resources, and limits. |
| Wasmtime | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Engine configuration is part of the peer; include fuel/epoch/debug/cache. |
| Wasmer | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Runtime/host adapter; record exact component/WASI feature set. |
| WAMR | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Embedded/interpreter/AOT/JIT profile; AOT-debug limits are measured, not waived. |
| Spin v4.1.0 local/operator (current pinned candidate) | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Current candidate only when the pinned operator/runtime/configuration manifest and required workload support are available; otherwise each cell is `U`/`unavailable`. |
| Spin cloud/deployment (historical evidence) | U | U | U | U | U | U | U | U | U | U | U | U | U | U | Historical deployment evidence is not a current denominator; retain the row and reason, never relabel it passing. |
| wasmCloud | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Component/capability/OCI projection; include readiness and fleet costs. |
| OCI/Docker/containerd | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Process/rootfs projection; same-work and image/boot/volume cost required. |
| Rootless/gVisor/Kata/Firecracker | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Isolation backend, not one ISA; host network/device/filtering obligations remain. |
| V8/JavaScript isolates | A | A | A | A | A | A | A | A | A | A | A | A | A | A | JS/browser adapter; origin, user activation, quota, API absence, and process risk stay visible. |
| BEAM/OTP | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Actor/managed-runtime adapter; preserve mailbox, supervision, hot code, and NIF costs. |
| Linux eBPF/Solana SBPF | P | X | X | P | P | X | X | X | X | X | X | X | X | P | Verifier/helper/chain-machine overlap is narrow; every `X` needs a non-overlap rationale, not a hidden omission. |
| NaCl/PNaCl/SFI | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Historical/native SFI projection; include validator, outer sandbox, and debug cost. |
| RISC-V profiles | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Raw ISA needs a declared OS/JPH/runtime; ISA portability is not host-service portability. |
| LLVM bitcode/MLIR bytecode | A | A | A | A | A | A | A | A | A | A | A | A | A | A | Compiler representation projection only after a specified execution/runtime boundary. |
| SPIR-V/PTX/WebGPU | P | X | X | X | X | X | A | P | X | A | X | P | X | X | Device-only overlap for numerics/game/AI/data; non-overlap cells record missing general host semantics. |

A reasoned `X` is not a permission to omit work: if the owner asks whether the family can perform that job through an adapter, replace `X` with `A` and run the full proof. For reproducible coverage, materialize one row keyed by `(family_id, implementation/configuration, workload_id, comparison_layer)` for every matrix cell. Serialize `P` as `comparable`, `A` as `adapter-required`, and `X` as `representation-only` or `device-only` with its reason; use `unknown`, `unavailable`, or `failing` when the frozen manifest or profile cannot establish the planned disposition. A profile limitation is `U`, not `X`; report whether the cause is missing service, device, executable-memory policy, API/authority restriction, or incompatible semantics. Only `comparable` and proof-closed `adapter-required` cells can enter C-010; every other status remains visible.

### Total status and denominator join

Every expanded cell receives the same fields:
`{intended_shape, intended_status, readiness_status, observation_status, final_status, reason_code, falsifier_code, denominator_eligible, receipt_id, metric_values, normalized_ratio, cost_ratio}`. The classifier below is the sole status function for this corpus. It initializes absent observation, receipt, metric, and ratio fields and a false denominator before branching. No source pin or declared profile is executable readiness.

```python
def classify_cell(candidate, workload, layer, shape):
    kind = candidate["candidate_kind"]
    state = candidate["config_state"]
    ident = candidate["candidate_id"]
    intended = {"P": "direct-overlap",
                "A": "adapter-required",
                "X": "representation-only"}[shape]
    if kind == "device_representation":
        intended = "device-only"
        result = ("DEVICE_ONLY", "device-only", "R-DEVICE", "F-DEVICE")
    elif shape == "X":
        result = ("REPRESENTATION_ONLY", "representation-only",
                  "R-REPRESENTATION", "F-REPRESENTATION")
    elif state == "spec_representation":
        if layer == "L1_REPRESENTATION":
            intended = "representation-only"
            result = ("REPRESENTATION_ONLY", "representation-only",
                      "R-REPRESENTATION", "F-REPRESENTATION")
        else:
            intended = "adapter-required"
            result = ("ADAPTER_UNPROVED", "adapter-required",
                      "R-ADAPTER-UNPROVED", "F-ADAPTER-GATE")
    elif state == "historical_only":
        result = ("HISTORICAL", "unavailable", "R-HISTORICAL", "F-HISTORICAL")
    elif state in ("source_pinned_config_unbound",
                   "source_moving_config_unbound"):
        result = ("UNBOUND", "unavailable", "R-CONFIG-UNBOUND",
                  "F-CONFIG-UNBOUND")
    elif state == "owned_proposal":
        result = ("PROPOSED", "unknown", "R-PROPOSED-UNRATIFIED",
                  "F-PROPOSED")
    elif state == "pinned_configured" and ident in (
            "rust-1.98.1-release", "rust-1.98.1-optimized"):
        if layer in ("L4_ISOLATION", "L6_LIFECYCLE_ORCHESTRATION"):
            intended = "adapter-required"
        if workload in ("W07_WEB", "W08_GAMES", "W12_AI",
                        "W13_GUI", "W14_EMBEDDED"):
            result = ("PROFILE_MISMATCH", "unavailable",
                      "R-PROFILE-MISMATCH", "F-PROFILE-MISMATCH")
        else:
            result = ("PROFILE_UNBOUND", "unknown",
                      "R-PROFILE-UNBOUND", "F-PROFILE-UNBOUND")
    else:
        raise ValueError("candidate outside the closed finite snapshot")
    readiness, status, reason, falsifier = result
    return {
        "intended_shape": shape,
        "intended_status": intended,
        "readiness_status": readiness,
        "final_status": status,
        "reason_code": reason,
        "falsifier_code": falsifier,
        "observation_status": "UNMEASURED",
        "denominator_eligible": False,
        "receipt_id": "ABSENT",
        "metric_values": "ABSENT",
        "normalized_ratio": "ABSENT",
        "cost_ratio": "ABSENT",
    }
```

The function handles `oci-runtime-1.2.0` as `spec_representation` even though its `candidate_kind` is orchestration: it is representation-only at L1 and adapter-required at later layers. It handles SPIR-V/PTX/WebGPU as device-only, historical rows as unavailable, unbound source/configuration rows as unavailable, and owned JPM rows as unknown. Exact required-profile mismatch is unavailable; a declared but physically or corpus-unbound matching profile is unknown. `comparable` and denominator eligibility require exact executable/source/target/profile/grant/limit/security/TCB/lifecycle binding, a closed correctness proof, and observed metrics; no current row reaches either state.

### Metrics, receipt, and statistical rules

Record every required metric with an explicit `name`, numeric `value`, SI or binary `unit`, `direction` (`lower_is_better` or `higher_is_better`), raw numerator/denominator, normalized ratio, uncertainty interval, and applicability disposition. For lower-is-better time/bytes/cost, `normalized_ratio = Jet / peer`; for higher-is-better throughput, `normalized_ratio = peer / Jet`; the normalized **COST ratio** is always `Jet_total_cost / peer_total_cost` and includes the declared cold/warm accounting. Report raw throughput as well as `peer_throughput / Jet_throughput`, never hide orientation in a label. Report shared-runtime amortization separately from full cold installation. Record warmup, repetitions, confidence interval, clock resolution, thermal state, independent reruns, OS/runtime/compiler/kernel/device versions, flags, source/artifact/cache/attestation hashes, and whether page/file/CPU caches were cold or warm. Each receipt names a fixed Rust baseline ID/configuration when Rust is a comparator; no Rust parity claim is valid without that baseline and an applicability reason.

Statistical rules are fixed before measurement. Each meaningful paired metric retains every valid repetition and reports median, p05, p95, and a paired nonparametric bootstrap percentile 95% interval from 10,000 resamples. The resampling seed is deterministic from `(workload_id, peer_id, source_digest, dataset_digest)`. At least five valid paired repetitions are required for a provisional interval; fewer are `inconclusive`, not silently pooled. No outlier is deleted after seeing the result. A strict threshold gate passes only when the entire interval is on the winning side; an interval that overlaps the threshold is `inconclusive`. The receipt declares one `cost_policy_digest` and the explicit weighted canonical-unit formula for total cost; cold and warm cells use the same policy and disclose amortization rather than changing units.

A performance receipt has this minimum shape:

```text
receipt_schema = "jet-comparison/1"
status = observed | expected | inconclusive | failing | uncovered
workload_id, source_digest, dataset_digest, oracle_digest
peer_id, stack_layers, compiler_id, runtime_id, kernel_id, cpu_isa, os_id, device_id
flags, target, profile, grants_digest, limits_digest
artifact_digest, package_lock_digest, cache_digest, attestation_digest
cache_state, install_state, repetitions, warmup, clock_resolution
rust_baseline_id, rust_baseline_config, applicability, non_overlap_reason
cost_policy_digest, attacker_model_id, attacker_assets
security_policy_digest, tcb_digest
enforcement_backend_id, enforcement_backend_version, enforcement_backend_digest
isolation_strength
lifecycle_durability_policy_id, lifecycle_durability_policy_version, lifecycle_durability_policy_digest
metrics = [{
  name, value, unit, direction, numerator, denominator,
  normalized_ratio, cost_ratio, uncertainty_interval, confidence,
  observed_v_expected, notes
}]
correctness, tier_parity
security, confidence_interval, variance, notes
```

`normalized_ratio` is lower-is-better `Jet/peer` or higher-is-better `peer/Jet`; `cost_ratio` is the explicit total-cost ratio `Jet_total_cost/peer_total_cost`. Every metric row states units and direction, and every fixture states whether the cell is direct, adapter/projection, reasoned non-overlap, or unavailable. `attacker_model_id`, `attacker_assets`, `security_policy_digest`, `tcb_digest`, the typed enforcement backend identity/version/digest, `isolation_strength`, lifecycle/durability policy identity/version/digest, and `cost_policy_digest` MUST byte-match between peers for a passing pair. A mismatch is `inconclusive` or `failing` before any ratio is considered. A Rust comparison names the exact pinned baseline toolchain, flags, libraries, runtime provisioning, reference profile, and source/manifest digest; if no such pinned baseline is available, the cell is `uncovered`/`unavailable`, not silently omitted.

A zero or below-resolution denominator, unstable run, missing first-party battery, absent peer, unsupported feature, or unmatched security policy is inconclusive/failing, not a ratio pass. Ratios do not measure safety or user experience. Every required cell/metric remains visible.

`C-010` **Per-cell gate.** After correctness/security/tier parity, every non-Rust peer requires the receipt's orientation-correct `normalized_ratio < 1.00` per required metric with the declared confidence interval entirely supporting the strict inequality. For lower-is-better metrics this is `Jet/peer`; for higher-is-better throughput it is `peer/Jet`; the explicit COST ratio remains `Jet_total_cost/peer_total_cost`. Rust uses the same canonical normalized ratio and passes parity only when `normalized_ratio <= 1.05` against the named pinned baseline with the same confidence and cost-policy rules. No baseline means `uncovered`/`unavailable`; no average, median across cells, omitted metric, or applicability relabeling can convert a loss into a win.

`C-011` **Full-cost gate.** Cold measurements MUST include artifact transfer, runtime provisioning/acquisition, validation, compilation, installation, and first result. Warm runs MUST state shared-runtime and native-cache amortization. A warm JPM versus cold peer, or a preinstalled runtime versus a downloaded JPM, is invalid.

`C-012` **Security gate.** Any correctness, security, authority, lifecycle, or tier failure invalidates speed evidence for that fixture. A faster unsafe or less durable configuration is not a passing comparison.

<a id="alternatives"></a>
## Alternatives and same-program trade-offs

The alternatives are not strawmen. Each can be the right choice for a job.

### Standard Wasm substrate plus Jetpack integration

For a portable HTTP function, compile the same handler to a Wasm component with WIT/WASI imports, publish it through the existing package graph, and run it under Wasmtime/wasmCloud/Spin. This preserves core validation, browser reach, WIT interoperability, and a large ecosystem. It leaves Jet semantic parity, package/authority ownership, engine versioning, and lifecycle unification split across components and host systems. Modern WASI async and component resources narrow the gap; they do not make the standard component binary a Jet-specific source/compiler/CoreLib contract.

**Trade:** For unchanged Wasm component jobs, the existing validation/interoperability/documented runtime path is a source-backed incumbent strength; its ecosystem breadth relative to JPM is **UNKNOWN**, not a measured superlative. Falsifier: the paired Wasm/JPM component workload with equal host policy, cold install, debug, and lifecycle receipts shows JPM equal or lower full cost without losing reach. A Wasm projection remains mandatory for browser/ecosystem reach.

### Owned stable typed JPM/1 (recommended direction)

For the same handler, lower checked Jet semantics to JPM/1, validate block-local SSA and imports, attach authority requests and source/debug identity in the Jetpack graph, and run through interpreter/JIT/AOT/browser tiers. For a durable service, the same generation owns service readiness, volumes, secrets, migrations, and receipts rather than delegating those facts to a runtime-specific manifest. For a managed language, JPM is the outer verified execution/authority boundary while the language runtime remains a declared guest library/process.

**Trade:** one semantic and package/authority seam, explicit native/browser tiers, and a single receipt model versus new format/verifier/compiler/tooling/ecosystem costs and no pre-existing JPM binary corpus. This is the coherent recommendation, not a demonstrated winner.

### Portable RISC-V or SFI guest

For the same numeric or CLI program, a frozen RV64 subset could provide a familiar instruction set, or NaCl-style SFI could validate native code. A favorable-target native-performance result is **UNKNOWN** until a matched RV64/SFI/JPM workload records equal work, validation, and security; the falsifier is the required numeric/CLI paired cell under fixed target flags. Raw RISC-V still needs a guest EEI, memory/trap policy, import whitelist, meter, and control-flow validator. SFI requires code layout, reliable disassembly, indirect-target masks, compiler conventions, and an outer sandbox. A controlled RISC-V/SFI machine therefore converges on the same policy/verification work while retaining lower-level UB and debugging costs.

**Trade:** reuse native compiler/ISA ecosystem versus larger validator/target/UB/outer-sandbox surface. JPM/1 is preferred for a new Jet contract because its closed typed semantics do not inherit generic LLVM UB or raw privilege/ECALL behavior.

### Managed-runtime-first

For the same backend, ship a Java/HotSpot, CLR, V8, or BEAM runtime and use its mature libraries, collectors, schedulers, and debugging. This can be the shortest path for existing applications and gives strong language-specific observability. It does not unify foreign language meaning, package authority, browser/mobile/no-MMU behavior, or host lifecycle without an outer broker. Native agents, JNI/NIFs, reflection, class loading, and target-specific AOT remain explicit costs.

**Trade:** For unchanged managed applications, documented library/collector/debug support is a scoped incumbent strength; relative ecosystem breadth and footprint are **UNKNOWN** until the same application and full provisioning/authority receipts are measured. The falsifier is a managed-first/JPM adapter pair that closes those costs without preserving the managed runtime. Support managed-first adapters rather than pretending JPM replaces them.

### Container/microVM-first

For the same service, package a native executable and its rootfs as OCI, run rootless, gVisor, Kata, or Firecracker, and use mature volumes, secrets, health, and rollout tools. For unchanged native applications and operational workflows, documented OCI compatibility and lifecycle tooling are scoped strengths subject to host/kernel requirements; no universal “strongest” claim is made. The falsifier is a full-cost native OCI/JPM projection pair with equal durability, readiness, and isolation policy. OCI images select host variants; they do not provide one cross-CPU application ISA. gVisor has syscall gaps; VM-backed paths add boot/device/memory costs; snapshots do not roll back database or network effects.

**Trade:** OCI's documented compatibility for unchanged native applications is broader at that layer, while operations and isolation remain host/kernel/architecture dependent and the application-language/authority story remains separate. Rootless/gVisor/Kata/Firecracker are containment options with distinct limits, not a universal security ranking. Falsifier: equal native-service cells with pinned backend, filtering, boot, device, and rollback receipts.

### Selected coherent design and kill condition

The recommendation is a **JPM/1 core plus explicit adapters and projections**, not a claim that JPM replaces every layer. It owns Jet portable semantics and the Jetpack graph; it consumes JPH/1 host services; it can lower to browser Wasm; it can wrap managed runtimes and native processes; it can export OCI; and it records the same authority/lifecycle receipts around each path. The recommendation is killed, not softened, if JPM cannot pass the conformance/security gates or cannot meet `C-010`/`C-011` on every required non-Rust cell against the strongest standard substrate. If only a subset passes, the honest outcome is a narrower proposal with the failing jobs explicitly outside its replacement claim.

<a id="decisions"></a>
## Decisions and owner-only amendments

All items here are **PROPOSED and unratified**. No current law changes until the owner ratifies a corresponding amendment.

1. **Adopt a new stable JPM/1 machine contract.** Amend the current portable-format law to recognize the fixed JPM/1 envelope, typed block-local SSA, defined arithmetic/traps, bounded memory, typed references, and no LLVM UB/poison semantics. Keep compiler-private TIR/MIR private and separately identified.
2. **Preserve arbitrary-precision `Int`.** Amend the portable semantic law, if needed, to state explicitly that JPM `i64` is a low-level primitive and cannot narrow Jet `Int`; exact `Int`, `Fraction`, and checked/floor/mod/remainder/shift operations remain shared Prelude/CoreLib semantics. Resolve the observed-in-source reachability conflict as an integration gate, not as a new source-language decision.
3. **Make the FP baseline an all-tier proposal.** Where JPM exact FP requires explicit NaN payload/signaling, signed-zero, gradual-underflow, nearest-even, FMA, and no implicit reassociation/contraction rules stronger than current inspected law, ratification MUST amend every tier together. A portable-only normalization is not sufficient.
4. **Preserve current atomics law.** Core lowering MUST retain the closed carrier family, SeqCst behavior, lock-free Atomic64 requirement, and no silent lock fallback. The current API has no caller order argument; foreign low-level orderings MUST NOT silently become a new public Jet parameter.
5. **Adopt one semantic reference path.** Amend execution-tier law so reference interpreter, baseline/JIT, AOT/cache, browser lowering, and native adapters must preserve one checked Prelude/CoreLib meaning after Source/AST -> TIR -> canonical MIR. Existing trusted native Jet and native Library contracts remain until a separate clean-cutover amendment.
6. **Adopt JPH/1 by reference.** Amend host/plugin law to use the canonical typed host ABI, broker-local generation-bound handles, explicit ownership/async/cancel/reentrancy, and authority checks. Do not duplicate host layouts in JPM.
7. **Extend, do not fork, the package graph.** Amend package/lock/Hangar/receipt schemas to carry JPM digests, interface/debug/authority identities, profile requirements, cache identities, source ownership, and generation state. Do not add a second resolver, trust database, deploy manifest, or sandbox authority.
8. **Keep foreign formats as explicit projections.** Amend compatibility law to define Wasm component/browser, JVM/.NET/JS/BEAM, native-process, OCI, gVisor, Kata, and Firecracker adapters. An adapter's trust transition, copying, lifecycle, and unsupported capabilities must be receipt-bearing.
9. **Require the comparison proof protocol.** Ratification must be contingent on the conformance and paired-workload gates in [conformance](#conformance) and [performance](#performance), including the format kill gate. A recommendation is complete while its empirical result is unknown; unknown must not be relabeled passing.
10. **Adopt no universal-optimum promise.** The final law must state the irreducible conflicts and must not promise unchanged foreign binaries, zero-copy everywhere, mandatory native speed, universal browser/device availability, exactly-once distributed rollback, or hostile-kernel security.
11. **Do not resurrect retired Jetpack commands.** Proposed portable/runtime inspection MUST reuse established `jet inspect` planes and current `jet run`/`jet build` ownership. `jetpack run`, `jetpack build`, `jetpack test`, and `jetpack fmt` remain retired redirects, not new adoption commands.
12. **Amend failure ownership and plugin lifetime.** Hard containment traps terminate the affected authority-isolated instance and guest tasks; ordinary typed host errors do not. A request survives only with independent state/isolation or a specified recovery protocol. Host input becomes an owned immutable snapshot before semantic validation unless an immutable/exclusive lease is enforced. Mutable zero-copy cancellation/trap may be partial, not completed output. Existing Wasmtime plugin reuse-after-trap behavior requires explicit amendment and caller cutover.

These recommendations need no user interaction or new Tower writes in this drafting wave. They are directions for a later owner decision. The required finding-disposition marker for this draft is:

```text
finding_disposition = report-only
action = none
reason = proposed comparison and owner amendment; current law unchanged; no ratification or implementation evidence
```

<a id="adoption"></a>
## Adoption boundary

This draft grants **no current permission** to emit JPM/1 artifacts, change Jetpack defaults, replace the existing Wasmtime plugin contract, change current native ABI rules, or treat a proposed service/profile/command as registered. Existing commands and current laws remain in force. A future ratification must name every changed promise and provide a clean cutover plan.
Current command ownership is part of the adoption boundary: reuse established `jet run` and `jet build`; do not describe `jetpack run/build/test/fmt` as accepted portable entry points. Any additional inspect or runtime command is proposed until explicitly registered.

Conditional adoption requires, in order:

1. Ratify the machine, host, package, lifecycle, and compatibility amendments as one coherent contract. Do not enable a format while leaving authority or receipts implicit.
2. Produce a reference decoder/verifier/interpreter and the golden/malicious corpus. The artifact must fail closed when mandatory profile services or limits are absent.
3. Establish interpreter/JIT/AOT/browser/host-profile differential conformance, source/debug mappings, cache trust, authority revocation, lifecycle recovery, and adapter receipts.
4. Run the paired corpus with full cold-install and warm-cache accounting. Publish every cell/metric, including losses and unknowns. Apply the strict per-cell gates and format kill gate.
5. Only after those gates, change a default or remove an incumbent path. A compatibility adapter may exist for a named transition, but it MUST have a removal condition and MUST NOT become a second permanent semantic/policy graph.
6. Preserve editable `.jet/deps` source, upstream baselines, licenses, lock identities, and local changes. No adoption step may overwrite owned source or use GC to delete a referenced artifact.

Report-only findings must include the marker above and one of these reasons: `unratified`; `implementation absent`; `runtime evidence absent`; `security vector failing`; `performance gate unknown`; `required target unavailable`; or `foreign semantics require adapter`. “No action” means no current-law mutation, not that the evidence is unimportant.

<a id="sources"></a>
## Sources and provenance
All external evidence below was retrieved 19 September 2026 unless a source states another date. This section cites primary specifications, official documentation, and repository paths; source inspection is not runtime, benchmark, security, or conformance evidence.

* **Jet law and repository evidence:** `Docs/spec/architecture.md`; `Docs/spec/spec.md`; `Docs/spec/syntax-decisions.md:774-811,1132-1141,1275-1281,2997-3006`; `crates/jet-foundation/src/MIR.rs:1-5,23,6678-6704`; `crates/jet-codegen/src/Codegen/TIR/mir.rs:2971-3015`; `crates/jet-codegen/src/Codegen/TIR/lower/expressions.rs:4923-4989`; `crates/jet-codegen/src/Codegen/MIREval.rs:24687-24771`; `crates/jet-foundation/src/Prelude/Core/Division.rs:1-11,80-145`; `crates/jet-foundation/src/Prelude/Core/FixedArithmetic.rs:350-462`; `crates/jet-foundation/src/Prelude/Core/Atomic.rs:1-7,188-235,238-307,325-445`; `crates/jet-foundation/src/Authority.rs`; `crates/jet-pkg-model/src/Authority.rs`; `crates/jet-pkg-model/src/Manifest.rs`; `crates/jet-pkg-model/src/Lock.rs`; `crates/jet-env-model`; `crates/jetpack`; `crates/jet-cli/src/CLI.rs`; `crates/jetpack/src/CLI/parse.rs`.
* **Machine and semantic boundary:** `crates/jet-foundation/src/MIR.rs:1-5,23,6678-6704`; `crates/jet-codegen/src/Codegen/TIR/mir.rs:2971-3015`; `crates/jet-codegen/src/Codegen/TIR/lower/expressions.rs:4923-4934`; `crates/jet-codegen/src/Codegen/MIREval.rs:24687-24695`; `crates/jet-foundation/src/Prelude/Core/Division.rs:1-11,80-145`; `crates/jet-foundation/src/Prelude/Core/FixedArithmetic.rs:441-462`; and `crates/jet-foundation/src/Prelude/Core/Atomic.rs:1-7,188-235,238-307,325-437`.
* **Architecture and proof inputs:** the proposed chapters below state the architecture and proof contract directly. The external comparison sources are linked in the following bullets; no hidden cache capture is required to interpret this specification.
* **Wasm:** [Core 3.0 specification](https://webassembly.github.io/spec/core/); [Component Model README](https://raw.githubusercontent.com/WebAssembly/component-model/main/README.md); [WIT](https://raw.githubusercontent.com/WebAssembly/component-model/main/design/mvp/WIT.md); [Canonical ABI](https://raw.githubusercontent.com/WebAssembly/component-model/main/design/mvp/CanonicalABI.md); [Concurrency](https://raw.githubusercontent.com/WebAssembly/component-model/main/design/mvp/Concurrency.md); [WASI P2](https://raw.githubusercontent.com/WebAssembly/WASI/wasi-0.2/docs/Preview2.md); [WASI P3 release](https://wasi.dev/releases/wasi-p3); [WASI security](https://wasi.dev/security); [Wasmtime security](https://docs.wasmtime.dev/security.html); [Wasmtime cache/precompile](https://docs.wasmtime.dev/cli-cache.html); [Wasmer runtime](https://docs.wasmer.io/runtime/); [WAMR](https://github.com/bytecodealliance/wasm-micro-runtime); [wasmCloud](https://wasmcloud.com/docs/overview/); [Spin deployment](https://developer.fermyon.com/cloud/deployment-concepts).
* **Managed runtimes:** [JVMS SE 25](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-2.html); [HotSpot GC Guide](https://docs.oracle.com/en/java/javase/25/gctuning/); [JEP 444](https://openjdk.org/jeps/444); [JEP 491](https://openjdk.org/jeps/491); [JEP 486](https://openjdk.org/jeps/486); [JVMTI](https://docs.oracle.com/en/java/javase/25/docs/specs/jvmti.html); [Graal Native Image](https://www.graalvm.org/latest/reference-manual/native-image/); [ECMA-335](https://ecma-international.org/publications-and-standards/standards/ecma-335/); [.NET GC](https://learn.microsoft.com/en-us/dotnet/standard/garbage-collection/fundamentals); [NativeAOT](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/); [AssemblyLoadContext](https://learn.microsoft.com/en-us/dotnet/core/dependency-loading/understanding-assemblyloadcontext); [V8 embedding](https://v8.dev/docs/embed); [current V8 sandbox](https://chromium.googlesource.com/v8/v8/+/HEAD/src/sandbox/README.md); [V8 mitigations](https://v8.dev/docs/untrusted-code-mitigations); [OTP docs](https://www.erlang.org/doc/system/).
* **Deployment/isolation:** [OCI Image v1.1.1](https://raw.githubusercontent.com/opencontainers/image-spec/v1.1.1/manifest.md); [OCI Image Index](https://raw.githubusercontent.com/opencontainers/image-spec/v1.1.1/image-index.md); [OCI Runtime v1.2](https://raw.githubusercontent.com/opencontainers/runtime-spec/v1.2.0/runtime.md); [containerd Runtime v2](https://raw.githubusercontent.com/containerd/containerd/main/docs/runtime-v2.md); [Docker rootless](https://docs.docker.com/engine/security/rootless/); [gVisor architecture](https://gvisor.dev/docs/architecture_guide/intro/); [gVisor compatibility](https://gvisor.dev/docs/user_guide/compatibility/linux/amd64/); [Kata architecture](https://github.com/kata-containers/kata-containers/blob/main/docs/design/architecture/README.md); [Firecracker design](https://raw.githubusercontent.com/firecracker-microvm/firecracker/main/docs/design.md); [TUF 1.0.36](https://theupdateframework.github.io/specification/latest/); [in-toto statement](https://raw.githubusercontent.com/in-toto/attestation/main/spec/v1/statement.md); [SLSA provenance 1.2](https://slsa.dev/spec/v1.2/build-provenance).
* **Portable ISA and verification:** [NaCl SFI](https://css.csail.mit.edu/6.858/2018/readings/nacl.pdf); [PNaCl](https://css.csail.mit.edu/6.858/2012/readings/pnacl.pdf); [Linux eBPF verifier](https://raw.githubusercontent.com/torvalds/linux/master/Documentation/bpf/verifier.rst); [SBPF](https://raw.githubusercontent.com/anza-xyz/sbpf/main/doc/bytecode.md); [RISC-V EEI/ISA](https://docs.riscv.org/reference/isa/v20240411/unpriv/intro.html); [RISC-V profiles](https://docs.riscv.org/reference/rva20-rvi20-rva22/v1.0/profilevsplat.html); [LLVM UB](https://llvm.org/docs/UndefinedBehavior.html); [LLVM compatibility](https://llvm.org/docs/DeveloperPolicy.html#ir-backwards-compatibility); [MLIR bytecode](https://mlir.llvm.org/docs/BytecodeFormat/).
* **Host/device evidence:** [Linux `openat2`](https://man7.org/linux/man-pages/man2/openat2.2.html); [WHATWG origins](https://html.spec.whatwg.org/multipage/browsers.html#origins); [W3C WebGPU](https://www.w3.org/TR/webgpu/); [Web Audio](https://webaudio.github.io/web-audio-api/); [SPIR-V](https://registry.khronos.org/SPIR-V/specs/unified1/SPIRV.html); [PTX](https://docs.nvidia.com/cuda/parallel-thread-execution/index.html); [Zephyr](https://docs.zephyrproject.org/latest/introduction/index.html).
* **Blow and debugging:** [Standup #39, WebAssembly/debugging passage](https://www.youtube.com/watch?v=UJuCzXmlcjg&t=3585s); [LambdaConf 2024, VM/bytecode passage](https://www.youtube.com/watch?v=7BaWley751Y); [Java JDWP](https://docs.oracle.com/en/java/javase/21/docs/specs/jdwp/jdwp-spec.html); [Chrome Wasm debugging](https://developer.chrome.com/docs/devtools/wasm); [Wasm DWARF](https://yurydelendik.github.io/webassembly-dwarf/).

### Citation IDs and reproducibility

The following IDs are stable labels for material factual claims in this proposal. “Current” means the cited repository path/specification version was inspected on 2026-09-19; it does not mean runtime behavior was exercised. Moving-head sources are explicitly non-reproducible until pinned.

| ID | Primary source | Claim boundary and limitation |
|---|---|---|
| S-JET-001 | `crates/jet-foundation/src/MIR.rs:1-5,23,6678-6704`; `crates/jet-codegen/src/Codegen/TIR/mir.rs:2971-3015` | Source/AST -> TIR -> canonical MIR schema 3 boundary; private compiler representation, no runtime proof. |
| S-JET-002 | `Docs/spec/syntax-decisions.md:774-811,1132-1141,1275-1281,2997-3006`; `crates/jet-foundation/src/Prelude/Core/Atomic.rs:1-7,188-235,238-307,325-437` | Current arithmetic/shift/atomic source law and SeqCst wording; exact FP gaps remain. |
| S-JET-003 | `crates/jet-pkg-model/src/Manifest.rs`; `crates/jet-pkg-model/src/Lock.rs`; `crates/jet-foundation/src/Authority.rs`; `crates/jet-pkg-model/src/Authority.rs` | Current graph/lock/authority ownership; no complete portable runtime execution result. |
| S-WASM-001 | [WebAssembly Core 3.0](https://webassembly.github.io/spec/core/); [Component Model](https://github.com/WebAssembly/component-model/tree/main); [WASI P3](https://wasi.dev/releases/wasi-p3) | Versioned standards/project pages; proposal-stage features and implementation parity remain explicit unknowns. |
| S-MANAGED-001 | [JVMS SE 25](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-2.html); [JEP 491](https://openjdk.org/jeps/491); [Native Image](https://www.graalvm.org/latest/reference-manual/native-image/); [.NET NativeAOT](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/) | Managed/AOT boundaries and current JEP correction; no cross-runtime benchmark. |
| S-DEPLOY-001 | [OCI Image v1.1.1](https://github.com/opencontainers/image-spec/tree/v1.1.1); [Docker rootless](https://docs.docker.com/engine/security/rootless/); [Firecracker networking](https://github.com/firecracker-microvm/firecracker/blob/main/docs/network-setup.md) | Image/rootless/VM mechanisms and host responsibilities; moving docs must be pinned for a run. |
| S-HOST-001 | [Linux `openat2(2)`](https://man7.org/linux/man-pages/man2/openat2.2.html); [WHATWG origins](https://html.spec.whatwg.org/multipage/browsers.html#origins); [WebGPU](https://www.w3.org/TR/webgpu/) | Host authority/device constraints; no broker implementation or escape test. |

A source with no immutable release/commit in the URL is `SOURCE-INSPECTED` but not a reproducible current peer baseline; the peer manifest must pin it before measurement. Historical Spin pages remain historical evidence and cannot establish current feature support.

### Corpus source registry

The following stable source IDs are the foreign keys used by the canonical candidate/configuration table and workload fixtures. They are source-inspection identities, not downloaded inputs, execution receipts, or benchmark results. Moving sources remain context only and do not establish a configured current candidate.

| source_id | immutable identity / primary link | exact locator or fact boundary | status/limitation |
|---|---|---|---|
| S-JET-PROP-V3 | This proposal, owned canonical contract | competitor/performance/source/coverage clauses | owned proposed JPM/config facts; no external release or runtime proof |
| S-JET-REPO-001 | Jet commit `d1d07de82ce6d01bb781f5a24c7e57423cf167ba`; https://github.com/jet-language/jet/tree/d1d07de82ce6d01bb781f5a24c7e57423cf167ba | package/lock/authority/CLI paths at the pinned commit | primary-pinned source inspection; no execution |
| S-RUST-REL-001 | https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/ | official release announcement dated 2026-09-03; Rust 1.98.1 point release and vtable-miscompilation fix | primary-pinned release; no local toolchain run |
| S-RUST-MANIFEST-002 | https://static.rust-lang.org/dist/channel-rust-1.98.1.toml | `manifest-version=2`, `date=2026-09-03`; x86_64-unknown-linux-gnu package URLs/hashes; cargo package git hash `48a229ceaefd4985c50990b14116b6d856af0985` | primary-pinned manifest; package availability is source fact, not installed proof |
| S-RUST-CHECKSUM-003 | https://static.rust-lang.org/dist/2026-09-03/rustc-1.98.1-x86_64-unknown-linux-gnu.tar.gz.sha256 and corresponding `rust-std`, `cargo`, `rustfmt`, `rust-src` checksum URLs | rustc `a6e35741daaac7978e7f485b564a783d13b6740a1ecf3e80c2e71696ca5cabb2`; rust-std `eddab0358cbd12aeb897716aab00d1db7b59696e85b9ac4982e72259a9a976b1`; cargo `3f1215b2a3b88c7aaa008b561bd4f39d6c6672fa7821e562ab6ba1a6d6f37f61`; rustfmt `16db0d6bb3535cfa84c009cffb161aac32d48f6e0648507a1f3368323cac3ec7`; rust-src `411c3dccf3782ed6eb784357999cd1bffeb65a6dd55196817992a6421c0f9689` | primary-pinned artifact identities; checksums are not benchmark results |
| S-RUST-PROFILE-004 | https://doc.rust-lang.org/cargo/reference/profiles.html | release defaults: opt-level=3, debug=false, strip=none, debug-assertions=false, overflow-checks=false, lto=false, panic=unwind, incremental=false, codegen-units=16, rpath=false; split-debuginfo target-dependent | primary official docs; target-dependent split value must be recorded in any future receipt |
| S-RUST-PLATFORM-005 | https://doc.rust-lang.org/nightly/rustc/platform-support.html | `x86_64-unknown-linux-gnu` listed Tier 1 with host tools; Linux kernel/glibc requirements stated there | official support table is moving channel documentation; candidate release/manifest remains pinned |
| S-MNIST-MODEL-001 | https://github.com/onnx/models/blob/4f43949841cb55a0b98dc8fcd045431ccafd9f96/validated/vision/classification/mnist/model/mnist-12.onnx ; https://github.com/onnx/models/blob/4f43949841cb55a0b98dc8fcd045431ccafd9f96/validated/vision/classification/mnist/README.md | `mnist-12.onnx`; Git LFS SHA-256 `5c688690f8bacf667d4c2074af5ad0646ca328d7ab03eccf944a65b320171bdd`, 26,143 bytes; README contract: trained CNN, alternating conv/maxpool, ONNX 1.9/opset 12, input `1x1x28x28` f32 batch=1, grayscale `[0,1]`, output `1x10` logits before Softmax | primary-pinned source identity; model bytes not downloaded/executed |
| S-MNIST-SAMPLE-002 | https://github.com/onnx/models/blob/4f43949841cb55a0b98dc8fcd045431ccafd9f96/validated/vision/classification/mnist/model/mnist-12.tar.gz | Git LFS SHA-256 `a53a59dcaca8804a0f6dfda9a3cf2e082979589391dbde73640b60684f1d24e9`, 26,741 bytes; bundled sample outputs are the declared numeric smoke-vector source | primary-pinned sample identity; bundle not downloaded/executed |
| S-MNIST-DATA-001 | https://github.com/tensorflow/datasets/blob/1401448b0c6c7aaf12bb5ee666a73fd6898650d1/tensorflow_datasets/url_checksums/mnist.txt | `t10k-images-idx3-ubyte.gz` URL `https://storage.googleapis.com/cvdf-datasets/mnist/t10k-images-idx3-ubyte.gz`, 1,648,877 bytes, SHA-256 `8d422c7b0a1c1c79245a5bcf07fe86e33eeafee792b84584aec276f5a2dbc4e6`; `t10k-labels-idx1-ubyte.gz` URL `https://storage.googleapis.com/cvdf-datasets/mnist/t10k-labels-idx1-ubyte.gz`, 4,542 bytes, SHA-256 `f7ae60f92e00ec6debd23a6088c31dbd2371eca3ffa0defaefb259924204aec6` | primary-pinned checksum manifest; dataset not downloaded/executed |
| S-TLS-RFC8448-001 | https://www.rfc-editor.org/rfc/rfc8448#section-2 and https://www.rfc-editor.org/rfc/rfc8448#section-3 | Public test RSA key in section 2; certificate DER and handshake transcript in section 3; these are explicit isolated corpus inputs, not production PKI | primary public fixture; no handshake executed |
| S-GPU-WGSL-001 | https://github.com/gpuweb/gpuweb/tree/358eebc8e7bf2d6efa41a4b8b3fbc3a715288204 | pinned editor-source `spec/index.bs` SHA-256 `c9675917738bbcde2e7931221f7c8808b1784e8f371aade64d6366a19a1aa19f`; `wgsl/index.bs` SHA-256 `11b1020a2fec581050599ae8e035a7138de551129c0856e055b41b4342d658fd` | primary pinned source, not a published snapshot; no GPU execution |
| S-WASM-001 | https://webassembly.github.io/spec/core/ ; https://github.com/WebAssembly/component-model/tree/main ; https://raw.githubusercontent.com/WebAssembly/WASI/wasi-0.2/docs/Preview2.md ; https://wasi.dev/releases/wasi-p3 | Core 3.0, Component Model/WIT, WASI P2/P3 facts | Core is versioned spec; component/WASI links are context-only until immutable release/config is captured |
| S-MANAGED-001 | https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-2.html ; https://docs.oracle.com/en/java/javase/25/gctuning/ ; https://openjdk.org/jeps/444 ; https://openjdk.org/jeps/491 ; https://openjdk.org/jeps/486 ; https://docs.oracle.com/en/java/javase/25/docs/specs/jvmti.html ; https://www.graalvm.org/latest/reference-manual/native-image/ ; https://ecma-international.org/publications-and-standards/standards/ecma-335/ ; https://learn.microsoft.com/en-us/dotnet/standard/garbage-collection/fundamentals ; https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/ ; https://learn.microsoft.com/en-us/dotnet/core/dependency-loading/understanding-assemblyloadcontext | managed verification/GC/reflection/AOT/interop boundaries only | primary context; no exact current JDK/Graal/.NET build/config in capture; excluded as current baseline |
| S-WASM-WMT-001 | Wasmtime v46.0.0, commit `423be7a4e4d30bb377c836d317521d1eb874e157`; https://github.com/bytecodealliance/wasmtime/releases/tag/v46.0.0 | `docs/security.md#webassembly-core`; `crates/wasmtime/src/config.rs` anchors at pinned commit | primary-pinned; configuration candidate, not measurement |
| S-WASM-WMR-002 | Wasmer v7.4.2, commit `7a48a071c7682a409d148cf37dc8da58322a123d`; https://github.com/wasmerio/wasmer/releases/tag/v7.4.2 | README secure-by-default; CLI backend/config source at pinned commit | primary-pinned; no Component/P3 parity claim |
| S-WASM-WAMR-003 | WAMR `WAMR-2.4.5`, commit `25bd7eb63e828e4bd242cc9b38d260b4b31c6605`; https://github.com/bytecodealliance/wasm-micro-runtime/releases/tag/WAMR-2.4.5 | README key features; build_wamr interpreter/AOT/JIT anchors | primary-pinned; AOT execution is documented, AOT debugging experimental and multi-thread Wasm debugging unsupported |
| S-WASM-SPIN-004 | Spin v4.1.0, tag object `1b5f23c9dac6115a7dd0f412be8fe1107a3d25d8`, peeled commit `c0b3726aa4857961e20cf8616a0df5f0741af73d`; https://github.com/spinframework/spin/releases/tag/v4.1.0 | README what-is-spin/usage; manifest v2 source; wasm32-wasip2 | primary-pinned current local path; signing/attestation/quotas/deployment compatibility remain unknown |
| S-WASM-SKUBE-005 | Spin Operator v0.6.1, tag object `cd3121a1500d99b2543d9f7861f8be36933b1de3`, peeled commit `96c9d1d1d69c2c04e005375c44f323fb10e31002`; https://github.com/spinframework/spin-operator/releases/tag/v0.6.1 | README; RuntimeClass `wasmtime-spin-v2`; SpinAppExecutor sample | primary-pinned current operator shape; no deployment result; chart discrepancy retained |
| S-WASM-WCLOUD-006 | wasmCloud v2.9.0, commit `68ebece9c537f8bb4b5c9999f274ec68d60f35a9`; https://github.com/wasmCloud/wasmCloud/releases/tag/v2.9.0 | README and `crates/wash-runtime/Cargo.toml` features/dependencies | primary-pinned; no readiness/fleet result |
| S-DEPLOY-OCI-011 | OCI Image v1.1.1 https://github.com/opencontainers/image-spec/tree/v1.1.1; OCI Runtime v1.2.0 https://github.com/opencontainers/runtime-spec/tree/v1.2.0 | manifests/indexes/config/layers and runtime lifecycle | primary-pinned versioned specs; no common syscall/CPU ABI, authority policy, or host filtering |
| S-DEPLOY-ROOTLESS-007 | Docker docs commit `7d6c8bf81ab88fc6f5c4893b6259864d29de574c`; https://github.com/docker/docs/blob/7d6c8bf81ab88fc6f5c4893b6259864d29de574c/content/manuals/engine/security/rootless/_index.md | prerequisites/known-limitations: user namespace, newuidmap/newgidmap, >=65,536 subordinate IDs, storage/kernel/cgroup requirements, unsupported AppArmor/checkpoint/overlay network/SCTP | primary-pinned docs; runtime Docker/containerd/runc still must be pinned for a run |
| S-DEPLOY-CONTD-010 | containerd v2.4.0, tag object `4a069347af913d2e76e6df0dd643cd7db7224d44`, peeled commit `a7fe631d96c08fb14cf8eff0afdc280e99c30a94`; https://github.com/containerd/containerd/releases/tag/v2.4.0 | runtime-v2 architecture/usage and rootless docs at pinned commit | primary-pinned; exact shim/runtime/snapshotter config absent |
| S-DEPLOY-FC-008 | Firecracker v1.17.0, tag object `29d66eb25954dbe3608eb91c180a6079f54d2ee8`, peeled commit `95f868c8e345b1cc8faccd1a3c910b4989dc3f58`; https://github.com/firecracker-microvm/firecracker/releases/tag/v1.17.0 | design networking/threat containment/internal architecture/sandboxing | primary-pinned; Firecracker does not filter traffic; host TAP/firewall/egress is separate; snapshot continuity not guaranteed |
| S-DEPLOY-KATA-009 | Kata v4.2.0, commit `c7351e797efff8bfc6bd73da0eb1909be12e2cfe`; https://github.com/kata-containers/kata-containers/releases/tag/4.2.0 | architecture compatibility/container creation/environments/virtualization | primary-pinned; hypervisor/guest assets/RuntimeClass must be frozen in run |
| S-GVISOR-001 | https://gvisor.dev/docs/architecture_guide/intro/ and https://gvisor.dev/docs/user_guide/compatibility/linux/amd64/ | Sentry/Gofer boundary and syscall compatibility-gap facts | primary context; exact release/config not captured, unavailable current candidate |
| S-V8-001 | V8 commit `491e7b14177fa914ece280f91fc0c8dfc5b2aa37`; https://github.com/v8/v8/commit/491e7b14177fa914ece280f91fc0c8dfc5b2aa37 | commit search identity only | primary-pinned commit but exact V8 feature/config source was not captured; current candidate unavailable |
| S-BEAM-001 | https://www.erlang.org/doc/system/index.html | OTP 29.1 actor/private-heap/mailbox/supervision/BeamAsm facts | primary context; no immutable OTP 29.1 release/config capture; unavailable current candidate |
| S-EBPF-001 | Linux commit `b72e29e0f7ee329d89f86db8700c8ea99b4a370a`; https://github.com/torvalds/linux/commit/b72e29e0f7ee329d89f86db8700c8ea99b4a370a | commit search identity only | primary-pinned commit but verifier source/config matrix not captured; whole-workload adapter cells unavailable |
| S-SBPF-001 | https://github.com/anza-xyz/sbpf/tree/main/doc/bytecode.md | SBPF VM/verifier/JIT/meter facts | moving `main`, no immutable commit returned; unavailable current candidate |
| S-NACL-001 | https://css.csail.mit.edu/6.858/2018/readings/nacl.pdf | historical NaCl SFI validation/process/debug facts and historical figures | historical paper; no current production candidate and no JPM measurement |
| S-PNACL-001 | https://css.csail.mit.edu/6.858/2012/readings/pnacl.pdf | historical constrained LLVM subset facts | historical paper; no current production candidate |
| S-RISCV-001 | https://docs.riscv.org/reference/isa/v20240411/unpriv/intro.html | RISC-V ISA/EEI versioned reference | primary versioned spec; raw ISA still needs guest EEI/runtime/authority |
| S-RISCV-PROFILE-002 | https://docs.riscv.org/reference/rva20-rvi20-rva22/v1.0/profilevsplat.html | RISC-V profile v1.0 constraints and non-guarantees | primary versioned spec; profiles do not define boot/ABI/devices/policy |
| S-LLVM-001 | https://llvm.org/docs/UndefinedBehavior.html and https://llvm.org/docs/DeveloperPolicy.html#ir-backwards-compatibility | LLVM UB/poison/compatibility facts | primary docs, moving; representation/adapter only, not sandbox |
| S-MLIR-001 | https://mlir.llvm.org/docs/BytecodeFormat/ | MLIR bytecode version/upgrade/dialect assumptions | primary docs, moving; no execution/trap/isolation contract |
| S-SPIRV-001 | https://registry.khronos.org/SPIR-V/specs/unified1/SPIRV.html | SPIR-V device representation | primary registry page without immutable release in URL; device-only |
| S-PTX-001 | https://docs.nvidia.com/cuda/parallel-thread-execution/index.html | PTX device representation | primary docs, moving; device-only |
| S-WEBGPU-001 | https://www.w3.org/TR/webgpu/ | WebGPU adapter/device/feature/limit/validation/device-loss boundary | primary standards page without immutable release in URL; device-only |

### 1. Jet repository provenance

The checkout remote is `https://github.com/jet-language/jet`; immutable checkout inspected: commit `d1d07de82ce6d01bb781f5a24c7e57423cf167ba` (resolved through `Tools/agent/jet-env`). These are source facts, not execution results.

#### S-JET-REPP-001 — package/lock/authority/CLI source

* Manifest: [`Manifest.rs#L1-L9`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Manifest.rs#L1-L9) says `package.jet` is Jet syntax, replaces old TOML `jet.toml`, and has no back-compat alias. [`Manifest.rs#L272-L290`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Manifest.rs#L272-L290) exposes package metadata, policy, authority, import boundaries, dependencies, and raw text. [`Manifest.rs#L312-L345`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Manifest.rs#L312-L345) distinguishes path/git/registry/foreign dependencies and marks branch/`@latest` selectors moving while revision selectors are not. [`Manifest.rs#L352-L373`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Manifest.rs#L352-L373) is the parser and canonical manifest-path helper.
* Lock: [`Lock.rs#L1-L5`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Lock.rs#L1-L5) defines `.jet/lock` schema v1 as the sole lockfile. [`Lock.rs#L25-L70`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Lock.rs#L25-L70) records source, exact locked revision, fingerprint, content hash, dependency/effect/authority fields, and envelope. [`Lock.rs#L454-L491`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Lock.rs#L454-L491) covers output envelope, pinned toolchain, and source-channel exact refs. [`Lock.rs#L871-L927`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Lock.rs#L871-L927) covers locked revision/browser and the full lock graph, including authority, toolchains, browsers, source channels, and build stamp.
* Canonical rights tree: [`foundation Authority.rs#L1-L5`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-foundation/src/Authority.rs#L1-L5), [`#L32-L99`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-foundation/src/Authority.rs#L32-L99), and [`#L101-L177`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-foundation/src/Authority.rs#L101-L177) define root/right parsing, descendant coverage, one `Holds` carrier, and typed host-import decisions. [`Authority.rs#L179-L208`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-foundation/src/Authority.rs#L179-L208) defines tighten-only child scopes and the shared `HostImportDecision`.
* Package-to-host authority: [`pkg Authority.rs#L99-L137`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Authority.rs#L99-L137) requires a lent scope to be a tighten-only child and to satisfy declared needs. [`Authority.rs#L168-L215`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Authority.rs#L168-L215) checks typed imports and no-follow roots. [`Authority.rs#L309-L312`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Authority.rs#L309-L312) identifies the checked manifest snapshot.
* Plugin bridge: [`Plugin.rs#L17-L27`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Prelude/Plugin.rs#L17-L27) defines the typed transport envelope and per-call Component Model type checking. [`Plugin.rs#L44-L89`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Prelude/Plugin.rs#L44-L89) records documented plugin fuel/memory/table/wire/timeout constants, canonical authority state, fuel and epoch interruption, and store limits. [`Plugin.rs#L134-L197`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-pkg-model/src/Prelude/Plugin.rs#L134-L197) validates wire rights and maps import names to declared rights.
* CLI registry: [`CLI.rs#L1-L19`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-cli/src/CLI.rs#L1-L19) declares one in-code command/flag registry. [`CLI.rs#L671-L704`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-cli/src/CLI.rs#L671-L704) lists inspect/provenance and related nested actions. [`CLI.rs#L900-L905`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-cli/src/CLI.rs#L900-L905) derives groups from `COMMANDS`; [`CLI.rs#L924-L980`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-cli/src/CLI.rs#L924-L980) renders shared action help. [`CLI.rs#L1053-L1060`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-cli/src/CLI.rs#L1053-L1060) defines `inspect`; [`CLI.rs#L1110-L1198`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jet-cli/src/CLI.rs#L1110-L1198) defines `run`, `check`, `test`, `build`, and `package` ownership/usages.
* Jetpack distinction: [`parse.rs#L572-L625`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jetpack/src/CLI/parse.rs#L572-L625) dispatches Jetpack's package/environment/lock/trust/hangar verbs, while [`parse.rs#L627-L667`](https://github.com/jet-language/jet/blob/d1d07de82ce6d01bb781f5a24c7e57423cf167ba/crates/jetpack/src/CLI/parse.rs#L627-L667) explicitly routes retired `run`, `test`, and `build` spellings back to `jet`.

Limitation: these are source-inspection facts at one immutable checkout. They do not prove a built binary, runtime enforcement, or portable ABI execution.

### 2. Pinned external primary sources

All entries below were retrieved 2026-09-19. A URL with a full commit is immutable. A release/tag is treated as pinned only where the named release/tag was resolved; annotated tag object and peeled commit are recorded when available. Named anchors are Markdown headings or source symbols when line numbers are not part of the upstream file interface.

#### Wasm engines and integrated runtimes

* **S-WASM-WMT-001 — Wasmtime v46.0.0, commit `423be7a4e4d30bb377c836d317521d1eb874e157`.** Release: https://github.com/bytecodealliance/wasmtime/releases/tag/v46.0.0. Security source: [`docs/security.md#webassembly-core`](https://github.com/bytecodealliance/wasmtime/blob/423be7a4e4d30bb377c836d317521d1eb874e157/docs/security.md#webassembly-core) documents Wasm memory bounds, typed control flow, imports/exports as the external boundary, and capability-oriented WASI; `#defense-in-depth` and `#filesystem-access` state additional implementation/configuration trade-offs. Config source: [`crates/wasmtime/src/config.rs`](https://github.com/bytecodealliance/wasmtime/blob/423be7a4e4d30bb377c836d317521d1eb874e157/crates/wasmtime/src/config.rs), anchors `InstanceAllocationStrategy::OnDemand`, `Config::new`, `Config::enable_compiler`, `Config::target`, `Config::debug_info`, `Config::guest_debug`, and pooling limit methods. Documented candidate facts: default allocation strategy is on-demand; compiler/target/debug/guest-debug/pooling are explicit configuration surfaces; `Config::new` has documented defaults. Limitation/gap: this capture did not execute Wasmtime or establish a current performance/security winner; exact compiled Cargo feature set and host profile remain manifest fields.
* **S-WASM-WMR-002 — Wasmer v7.4.2, commit `7a48a071c7682a409d148cf37dc8da58322a123d`.** Release: https://github.com/wasmerio/wasmer/releases/tag/v7.4.2. [`README.md`](https://github.com/wasmerio/wasmer/blob/7a48a071c7682a409d148cf37dc8da58322a123d/README.md) states secure-by-default behavior (no file/network/environment access unless enabled) and embeddability. Backend/config source: [`lib/cli/src/backend.rs`](https://github.com/wasmerio/wasmer/blob/7a48a071c7682a409d148cf37dc8da58322a123d/lib/cli/src/backend.rs), anchors `RuntimeOptions`, `BackendType::enabled`, `RuntimeOptions::get_available_backends`, and `BackendType::get_engine`, documents Singlepass/Cranelift/LLVM/V8 selection and SIMD/threads/reference-types/multi-value/bulk-memory feature flags. WASI source: [`lib/cli/src/commands/run/wasi.rs`](https://github.com/wasmerio/wasmer/blob/7a48a071c7682a409d148cf37dc8da58322a123d/lib/cli/src/commands/run/wasi.rs), anchor `struct Wasi`, documents `--volume`, `--cwd`, env forwarding, offline/local package inputs, `--net` with optional ruleset, async threads, HTTP client, multiple-WASI-version denial, and cache disable. Limitation/gap: no pinned Component Model/WASI-P3 parity claim was established here; no runtime/benchmark result.
* **S-WASM-WAMR-003 — WAMR `WAMR-2.4.5`, commit `25bd7eb63e828e4bd242cc9b38d260b4b31c6605`.** Release: https://github.com/bytecodealliance/wasm-micro-runtime/releases/tag/WAMR-2.4.5. [`README.md#key-features`](https://github.com/bytecodealliance/wasm-micro-runtime/blob/25bd7eb63e828e4bd242cc9b38d260b4b31c6605/README.md#key-features) documents interpreter, AOT, JIT, fast JIT, and dynamic tier-up; [`doc/build_wamr.md#configure-interpreters`](https://github.com/bytecodealliance/wasm-micro-runtime/blob/25bd7eb63e828e4bd242cc9b38d260b4b31c6605/doc/build_wamr.md#configure-interpreters) and `#configure-aot-and-jits` document `WAMR_BUILD_INTERP`, `WAMR_BUILD_FAST_INTERP`, `WAMR_BUILD_AOT` (documented default enabled), `WAMR_BUILD_JIT`, `WAMR_BUILD_FAST_JIT`, and multi-tier JIT combinations. The same build source documents target/platform, shared memory/thread manager, memory bounds flags, instruction metering, global heap, stack, and debug-build flags. [`wamr-compiler/README.md`](https://github.com/bytecodealliance/wasm-micro-runtime/blob/25bd7eb63e828e4bd242cc9b38d260b4b31c6605/wamr-compiler/README.md) documents building `wamrc` and running its AOT output with `iwasm`. [`doc/source_debugging_aot.md#debugging-with-aot`](https://github.com/bytecodealliance/wasm-micro-runtime/blob/25bd7eb63e828e4bd242cc9b38d260b4b31c6605/doc/source_debugging_aot.md#debugging-with-aot) says **AOT debugging is experimental**. [`doc/source_debugging_interpreter.md#attentions`](https://github.com/bytecodealliance/wasm-micro-runtime/blob/25bd7eb63e828e4bd242cc9b38d260b4b31c6605/doc/source_debugging_interpreter.md#attentions) says **debugging a multi-thread Wasm module is not supported**; native threads are a separate case. [`doc/embed_wamr.md#the-runtime-initialization`](https://github.com/bytecodealliance/wasm-micro-runtime/blob/25bd7eb63e828e4bd242cc9b38d260b4b31c6605/doc/embed_wamr.md#the-runtime-initialization) documents bounded pool initialization and default maximum thread number 4 when unset. Limitation/gap: these are documented build/runtime options, not an executed feature matrix, artifact digest, or measured winner.
* **S-WASM-SPIN-004 — Spin v4.1.0, annotated tag object `1b5f23c9dac6115a7dd0f412be8fe1107a3d25d8`, peeled commit `c0b3726aa4857961e20cf8616a0df5f0741af73d`.** Release: https://github.com/spinframework/spin/releases/tag/v4.1.0. [`README.md#what-is-spin`](https://github.com/spinframework/spin/blob/c0b3726aa4857961e20cf8616a0df5f0741af73d/README.md#what-is-spin) identifies Spin as a framework using the Component Model and Wasmtime; [`README.md#usage`](https://github.com/spinframework/spin/blob/c0b3726aa4857961e20cf8616a0df5f0741af73d/README.md#usage) documents `spin new`, `spin build`, `spin up`, and `wasm32-wasip2`. [`crates/manifest/src/schema/v2.rs`](https://github.com/spinframework/spin/blob/c0b3726aa4857961e20cf8616a0df5f0741af73d/crates/manifest/src/schema/v2.rs), anchors `AppManifest`, `AppDetails`, `ensure_profile`, `ComponentDependency`, and `TriggerDependency`, documents manifest version 2, application targets such as `spin-up:3.3`/`spinkube:0.4`, profile validation, registry/path/component dependencies, and HTTP dependencies requiring a SHA-256 digest. SIP sources are explicitly historical: [`001-spin-deploy.md`](https://github.com/spinframework/spin/blob/c0b3726aa4857961e20cf8616a0df5f0741af73d/docs/content/sips/001-spin-deploy.md) is dated 2022-03-18/updated 2022-03-31 and proposes Bindle/Hippo deployment; [`007-deployment-auth.md`](https://github.com/spinframework/spin/blob/c0b3726aa4857961e20cf8616a0df5f0741af73d/docs/content/sips/007-deployment-auth.md) is dated 2022-09-30 and proposes login/deployment auth; [`011-component-versioning.md`](https://github.com/spinframework/spin/blob/c0b3726aa4857961e20cf8616a0df5f0741af73d/docs/content/sips/011-component-versioning.md) is dated 2023-01-24 and proposes embedded SDK/version markers. Limitation/gap: current cloud deployment, signing/attestation, quotas, and version compatibility are not established by those SIPs.
* **S-WASM-SKUBE-005 — Spin Operator v0.6.1, signed annotated tag object `cd3121a1500d99b2543d9f7861f8be36933b1de3`, peeled commit `96c9d1d1d69c2c04e005375c44f323fb10e31002`.** Release: https://github.com/spinframework/spin-operator/releases/tag/v0.6.1. [`README.md`](https://github.com/spinframework/spin-operator/blob/96c9d1d1d69c2c04e005375c44f323fb10e31002/README.md) says the operator watches SpinApp custom resources and realizes desired state in Kubernetes. [`config/samples/spin-runtime-class.yaml`](https://github.com/spinframework/spin-operator/blob/96c9d1d1d69c2c04e005375c44f323fb10e31002/config/samples/spin-runtime-class.yaml) selects RuntimeClass `wasmtime-spin-v2` with handler `spin`; [`config/samples/spin-shim-executor.yaml`](https://github.com/spinframework/spin-operator/blob/96c9d1d1d69c2c04e005375c44f323fb10e31002/config/samples/spin-shim-executor.yaml) documents `createDeployment: true` and that runtime class. Release metadata names image `ghcr.io/spinframework/spin-operator:v0.6.1` and release assets for CRDs/runtime class/shim executor. Limitation/gap: the checked source `charts/spin-operator/Chart.yaml` still says chart/appVersion `0.6.0`; use release asset/image identity, not an inferred chart version. No current deployment success or performance result was observed.
* **S-WASM-WCLOUD-006 — wasmCloud v2.9.0, commit `68ebece9c537f8bb4b5c9999f274ec68d60f35a9`.** Release: https://github.com/wasmCloud/wasmCloud/releases/tag/v2.9.0. [`README.md`](https://github.com/wasmCloud/wasmCloud/blob/68ebece9c537f8bb4b5c9999f274ec68d60f35a9/README.md) documents `wash`, `wash-runtime`, Kubernetes operator/OCI chart, deny-by-default component capabilities, and built-in/Ingress/plugin capability categories. [`crates/wash-runtime/Cargo.toml`](https://github.com/wasmCloud/wasmCloud/blob/68ebece9c537f8bb4b5c9999f274ec68d60f35a9/crates/wash-runtime/Cargo.toml), anchors `[features]` and `[dependencies]`, pins the source-level runtime to Wasmtime component-model features and enables `wasmtime-wasi` P3 plus P2/P3 HTTP features. Limitation/gap: README install examples reference mutable `main` overlays; those examples are not a pinned deployment baseline. No execution, readiness, or fleet measurement was made.

#### Deployment/isolation

* **S-DEPLOY-ROOTLESS-007 — Docker rootless docs, immutable docs commit `7d6c8bf81ab88fc6f5c4893b6259864d29de574c`.** Source [`security/rootless/_index.md`](https://github.com/docker/docs/blob/7d6c8bf81ab88fc6f5c4893b6259864d29de574c/content/manuals/engine/security/rootless/_index.md) anchors `#how-it-works` and `#prerequisites`; [`security/rootless/troubleshoot.md`](https://github.com/docker/docs/blob/7d6c8bf81ab88fc6f5c4893b6259864d29de574c/content/manuals/engine/security/rootless/troubleshoot.md) anchors `#known-limitations` and `#networking-errors`. Facts: daemon and containers run in a user namespace; `newuidmap`/`newgidmap` and at least 65,536 subordinate UIDs/GIDs are prerequisites; supported storage drivers are overlay2 with kernel >=5.11, fuse-overlayfs with kernel >=4.18, btrfs with stated conditions, and vfs; cgroups require cgroup v2 and systemd; AppArmor, checkpoint, overlay network, and SCTP exposure are unsupported; inspect IPs are namespaced; host/global capabilities are not granted by container `--cap-add`; user-mode networking characteristics depend on the selected RootlessKit network/port drivers. Limitation: docs are configuration/limitation evidence only; no rootless run was exercised and no throughput claim is made.
* **S-DEPLOY-FC-008 — Firecracker v1.17.0, annotated tag object `29d66eb25954dbe3608eb91c180a6079f54d2ee8`, peeled commit `95f868c8e345b1cc8faccd1a3c910b4989dc3f58`.** Release: https://github.com/firecracker-microvm/firecracker/releases/tag/v1.17.0. [`docs/design.md#host-networking-integration`](https://github.com/firecracker-microvm/firecracker/blob/95f868c8e345b1cc8faccd1a3c910b4989dc3f58/docs/design.md#host-networking-integration) and `#threat-containment` state that network devices use host TAP and **Firecracker performs no network traffic filtering; egress must be filtered at host level**. `#internal-architecture` states one process encapsulates one microVM; `#sandboxing` documents seccomp and recommended jailer/cgroup/namespace/privilege controls. [`docs/network-setup.md#on-the-host`](https://github.com/firecracker-microvm/firecracker/blob/95f868c8e345b1cc8faccd1a3c910b4989dc3f58/docs/network-setup.md#on-the-host) gives host TAP/routing/firewall setup. [`docs/snapshotting/snapshot-support.md#snapshot-files-management`](https://github.com/firecracker-microvm/firecracker/blob/95f868c8e345b1cc8faccd1a3c910b4989dc3f58/docs/snapshotting/snapshot-support.md#snapshot-files-management) says snapshot files are trusted by Firecracker, users must implement authentication/encryption/lifecycle, and only a 64-bit CRC partial-corruption check is provided; `#overview`/`#limitations` say network/vsock packet loss and connection continuity are not guaranteed. [`docs/jailer.md#jailer-operation`](https://github.com/firecracker-microvm/firecracker/blob/95f868c8e345b1cc8faccd1a3c910b4989dc3f58/docs/jailer.md#jailer-operation) documents chroot, cgroups, namespaces, device setup, and privilege drop. Limitation: host filtering, snapshot trust, and lifecycle are separate integration obligations; no VM was started.
* **S-DEPLOY-KATA-009 — Kata Containers v4.2.0, immutable release commit `c7351e797efff8bfc6bd73da0eb1909be12e2cfe`.** Release: https://github.com/kata-containers/kata-containers/releases/tag/4.2.0. [`docs/design/architecture/README.md#compatibility`](https://github.com/kata-containers/kata-containers/blob/c7351e797efff8bfc6bd73da0eb1909be12e2cfe/docs/design/architecture/README.md#compatibility) documents OCI runtime, CRI/containerd/CRI-O compatibility, and shim-v2; `#container-creation`/`#environments` describe a guest VM and agent/container layering. [`docs/design/virtualization.md#firecrackerkvm`](https://github.com/kata-containers/kata-containers/blob/c7351e797efff8bfc6bd73da0eb1909be12e2cfe/docs/design/virtualization.md#firecrackerkvm) lists Firecracker's documented limitations (no virtio-fs/filesystem sharing, no hotplug, no VFIO/passthrough, limited CRI support) and distinguishes them from richer hypervisors. Limitation: architecture comparison is documentation, not a measured isolation or boot result.
* **S-DEPLOY-CONTD-010 — containerd v2.4.0, annotated tag object `4a069347af913d2e76e6df0dd643cd7db7224d44`, peeled commit `a7fe631d96c08fb14cf8eff0afdc280e99c30a94`.** Release: https://github.com/containerd/containerd/releases/tag/v2.4.0. [`docs/runtime-v2.md#architecture`](https://github.com/containerd/containerd/blob/a7fe631d96c08fb14cf8eff0afdc280e99c30a94/docs/runtime-v2.md#architecture) states containerd coordinates content/snapshotters and invokes runtime shims over the v2 API; runtime selection/configuration is described under `#usage`. [`docs/rootless.md`](https://github.com/containerd/containerd/blob/a7fe631d96c08fb14cf8eff0afdc280e99c30a94/docs/rootless.md) documents user namespaces/RootlessKit, rootless snapshotter kernel requirements, and cgroup v2/systemd conditions. [`docs/namespaces.md`](https://github.com/containerd/containerd/blob/a7fe631d96c08cf8eff0afdc280e99c30a94/docs/namespaces.md) explicitly says namespaces are administrative and not a security feature. Limitation: no containerd daemon or shim was run.
* **S-DEPLOY-OCI-011 — OCI Image v1.1.1 and OCI Runtime v1.2.0.** Immutable versioned sources: https://github.com/opencontainers/image-spec/tree/v1.1.1 and https://github.com/opencontainers/runtime-spec/tree/v1.2.0. The image spec version documents manifests/indexes/config/layers; the runtime spec version documents process lifecycle/configuration. Limitation: OCI does not by itself supply a common syscall/CPU ABI, authority policy, or host filtering.

#### Mutable-source repair status

* **V8:** the prior current-HEAD claim is not used as a current strongest baseline. A current commit search returned `491e7b14177fa914ece280f91fc0c8dfc5b2aa37` on 2026-09-16 (`https://github.com/v8/v8/commit/491e7b14177fa914ece280f91fc0c8dfc5b2aa37`), but the exact V8 feature/config source needed for this comparison was not captured at that commit. Status: `unknown`/`unavailable` pending pinned source/config capture; do not retain `HEAD`.
* **Linux eBPF:** a commit search returned `b72e29e0f7ee329d89f86db8700c8ea99b4a370a` (`https://github.com/torvalds/linux/commit/b72e29e0f7ee329d89f86db8700c8ea99b4a370a`), but the verifier source/config matrix was not captured at that exact commit. Status: `unknown`/`unavailable` pending exact file capture; do not retain `master`.
* **Solana SBPF:** no immutable commit was returned by the repository commit lookup for the prior `main` source. Status: `unknown`/`unavailable`; do not call it current strongest.
* **Graal Native Image:** the inspected managed capture used “latest” without an exact release/commit. Status: `unknown`/`unavailable`; do not call it current strongest.
* **Editor drafts and any remaining `main`/`HEAD` URLs:** historical/context only until an immutable primary revision and exact source anchor are captured. No unpinned source is a comparator baseline.


<a id="coverage"></a>
## Coverage and remaining unknowns

This is a finite coverage record, not an ongoing work ledger. It states where the comparison chapters account for each scope row and what remains unproved.

The matrix is instantiated as a rectangular status record for every named family and required workload:

```text
coverage_cell = {
  family_id, implementation_config_id, workload_id, comparison_layer,
  status = comparable | adapter-required | representation-only | device-only
         | unknown | unavailable | failing,
  applicability_reason, primary_source_or_anchor, falsifier_or_expected_receipt
}
```

No grouped family prose substitutes for these rows. `representation-only` and `device-only` identify semantic non-overlap; `unavailable` identifies an environment/feature that could have been required but cannot run; `unknown` identifies missing evidence; `failing` identifies an observed or specified gate failure. Only `comparable` and proof-closed `adapter-required` rows enter a performance denominator.

### Approved structural snapshot

The finite record contains 46 candidate/configuration rows × 14 workloads × 6 layers = 3,864 cells. All candidate rows carry nonempty `source_ids` lists whose foreign keys resolve to the source registry; every workload and layer appears in the Cartesian expansion. The approved classifier snapshot has the following dispositions:

| final status | cells | denominator meaning |
|---|---:|---|
| `adapter-required` | 700 | intended adapter/projection overlap, proof not closed |
| `device-only` | 252 | device representation/API, not a complete stack |
| `representation-only` | 140 | representation/spec overlap only |
| `unavailable` | 2,412 | required environment, profile, or runnable configuration absent |
| `unknown` | 360 | proposal or declared profile lacks executable/corpus/readiness evidence |
| `comparable` | 0 | no exact executable binding and closed correctness proof |
| `failing` | 0 | no executed failure result is claimed |

All 3,864 rows remain `observation_status=UNMEASURED`, `receipt_id=ABSENT`, with absent metric/ratio fields and `denominator_eligible=false`. The counts above are structural applicability/readiness dispositions, not runtime, benchmark, conformance, security, or deployment observations. The strict metric formulas, uncertainty rules, Rust parity threshold, and denominator predicate remain solely in [the performance protocol](#performance).

| Scope row | Covered here | Evidence status and exact remaining unknown |
|---|---|---|
| Replacement meaning, threat model, non-goals, irreducible conflicts | [summary](#summary), [goals](#goals), [alternatives](#alternatives) | Architecture and threat boundary are proposed; no hostile-kernel or universal-optimum claim is made. |
| Stable representation versus compiler IR | [terminology](#terminology), [competitors](#competitors), [alternatives](#alternatives) | JPM/1 contract is specified by the architecture and machine clauses; LLVM/MLIR semantics are source-backed; no runtime exercise. |
| Verifier, compiler/cache, interpreter/JIT/AOT/browser/debug | [downside ledger](#downside-ledger), [conformance](#conformance), [performance](#performance) | Proposed vectors are complete at comparison level; no implementation, fuzz run, debugger parity, or cache test exists. |
| Isolation, capabilities, host calls, resources, cancellation, side channels, FFI | [competitors](#competitors), [conformance](#conformance), [downside ledger](#downside-ledger) | Host evidence documents constraints; exact JPH/1 contract and runtime enforcement belong to host chapters; covert-channel elimination remains out of scope. |
| Package/lock/Hangar/receipt, source ownership, supply chain, offline cache | [summary](#summary), [decisions](#decisions), [adoption](#adoption), [sources](#sources) | Existing graph is source-backed; exact field/state schema is specified in the package and graph clauses; no TUF/in-toto implementation or offline exercise. |
| Lifecycle, readiness, migration, rollout, partition, rollback | [competitors](#competitors), [conformance](#conformance), [performance](#performance) | Docker/Firecracker/Spin evidence is documented; distributed exactly-once and external-effect rollback remain explicitly impossible/unknown per workload. |
| Linux/macOS/Windows/browser/mobile/no-MMU | [competitors](#competitors), [coverage](#coverage) | Host evidence establishes platform constraints; full GUI/backend/browser-version/device matrices and measurements remain unknown. |
| GPU/ML, GUI, audio, network, CLI, data, backend, embedded | [competitors](#competitors), [performance](#performance), [conformance](#conformance) | Required cells are specified. Device availability, native library parity, Linux GUI backends, energy, and latency are not measured. |
| Developer path, disassembly, source maps, diagnostics, replay | [summary](#summary), [competitors](#competitors), [conformance](#conformance) | Blow/JVM/Wasm debugger facts are cited; JPM tooling quality and replay coverage are unknown. |
| JVM/HotSpot/Graal | [competitors](#competitors), [downside ledger](#downside-ledger) | Current JEP 491, GC, Native Image, JVMTI facts corrected; no JVM benchmark or complete Graal release matrix. |
| .NET CLR/NativeAOT | [competitors](#competitors), [downside ledger](#downside-ledger) | Current docs establish managed/AOT/debug boundaries; no full CLR version/target/performance matrix. |
| Wasm core/Components/WASI/Wasmtime/Wasmer/WAMR/Spin/wasmCloud | [competitors](#competitors), [alternatives](#alternatives), [performance](#performance) | WAMR AOT execution is supported but AOT debugging is experimental; Spin evidence is historical and current runtime/quotas/signing remain unknown; Component final binary/version and Wasmer/WAMR P3 matrices remain unknown. Every 46 × 14 × 6 corpus cell remains visible in the canonical status record. |
| OCI/Docker/containerd/rootless/gVisor/Kata/Firecracker | [competitors](#competitors), [downside ledger](#downside-ledger) | Rootless and VM paths are treated as real comparators. Runtime, syscall, boot, memory, and escape measurements remain unknown. |
| V8/JS isolates and BEAM/OTP | [competitors](#competitors), [downside ledger](#downside-ledger) | Current V8 sandbox maturity and residual process boundary are stated; BEAM actor/NIF distinction is stated. No runtime measurements. |
| eBPF/SBPF, NaCl/PNaCl, RISC-V/SFI | [competitors](#competitors), [alternatives](#alternatives) | Current source captures provide verifier/EEI/SFI facts; no execution or current NaCl production test. |
| LLVM/MLIR representation candidates | [competitors](#competitors), [downside ledger](#downside-ledger) | UB/poison, compatibility, dialect assumptions are cited; final JPM machine chapter must supply exact semantics. |
| SPIR-V/PTX/WebGPU device contrasts | [competitors](#competitors), [performance](#performance) | Device-only boundary and async loss/limits are cited; cross-vendor conformance and energy/performance are unknown. |
| Conformance and malicious corpus | [conformance](#conformance) | Fixture specifications and expected dispositions exist; no vector has been executed. |
| Performance and ratification gate | [performance](#performance), [alternatives](#alternatives) | Metrics, receipts, cells, and strict gates exist; no actual result or numeric speedup is claimed. |
| Rust control configuration | [performance](#performance), [conformance](#conformance) | A named Rust compiler/runtime/target/profile/cache/security baseline is required for parity. A source-pinned but physically or corpus-unbound lane remains `unknown`/`unavailable` under the canonical classifier, never `comparable` or denominator-eligible and never silently waived. |
| Decisions and adoption boundary | [decisions](#decisions), [adoption](#adoption) | Recommendations are concrete and unratified; no current law, default, Tower state, or implementation is changed. |

The decisive remaining unknowns are empirical rather than rhetorical: whether JPM/1 validation and ABI costs fit cold-start budgets; whether its compiler/libraries and debug tools can match required tiers; whether its browser, no-MMU, mobile, GUI, audio, GPU/ML, and foreign-runtime adapters cover the required jobs; whether authority and lifecycle receipts hold under malicious concurrency and crash schedules; and whether every required per-cell/per-metric gate beats the strongest standard substrate. Until those observations exist, this draft is a complete comparison protocol and recommendation, not a demonstrated platform.
