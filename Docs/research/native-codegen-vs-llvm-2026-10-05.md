# Jet-native code generation versus LLVM

Research dated 2026-10-05. This answers an architectural question, not a directive to build another backend or a claim that the native cutover has finished. The proposed milestones below are decision gates, not a parallel work ledger.

## The short answer

Jet can generate its own machine code for fast builds and use LLVM for optimized release builds. This is technically ordinary, not speculative: Jai uses the two-backend shape, Zig has demonstrated substantially faster development compilation with its self-hosted x86-64 backend, and Go ships its own optimizing compiler and machine-code generators. The difficult part is not emitting an `add` instruction. It is preserving every language and runtime contract, delivering a usable debugger and linker, supporting multiple ABIs, and keeping that system correct as the language and hardware evolve.

A Jet optimizer can match or beat LLVM on particular programs. The most promising way is to retain and exploit Jet semantics before lowering them to pointers, runtime calls and machine arithmetic. It is much less credible to promise that a small new general-purpose machine backend will beat LLVM across x86-64, AArch64, RISC-V and WebAssembly, across all workloads. Those are different claims. No evidence reviewed here establishes the latter.

The recommended direction is the existing separation: one checked semantic pipeline, one runtime meaning, a deliberately fast Jet O0/O1 machine backend, and LLVM-backed release compilation. Make Jet-specific optimizations feed both paths. Improve native O1 with a bounded scalar pipeline, then use actual code-quality gaps to decide whether an independent release optimizer is worth funding. LLVM should remain the release reference while that decision is unresolved; “we own code generation” does not require “we reimplement all of LLVM.”

## What “the native backend works” does and does not finish

The starting context is the [native backend design](jet-backend-design-2026-10-01.md), the [adaptive-specialization investigation](adaptive-runtime-specialization-2026-10-04.md), and [Jet's philosophy](../spec/philosophy.md). The design specifies checked MIR lowering to typed LIR, an x86-64 System V emitter, linear-scan allocation, images, relocation, runtime calls and replaceable functions. It also describes release emission through Rust/rustc. Thus “LLVM at release” initially means LLVM indirectly through rustc, not an already-established direct Jet-to-LLVM implementation.

The owner-supplied `~/.cache/jet-dev/scratch/BackendO1/HANDOFF.md`, read on this date, contains successive backend leads' handoffs, including BackendO8. It records continuing runtime-carrier and lowering work, ownership failures found in aggregate construction, a liveness-scaling repair, and runtime-pack/linking costs. These are historical engineering observations from that scratch snapshot, not a new test receipt or an inventory of what master supports. They illustrate why a successful scalar fixture is not equivalent to completing the backend. No compiler or benchmark was run for this report.

If “works” means complete language/Core parity on the initial supported platform, the remaining compiler work divides into three independent dimensions.

First, product-quality execution still needs reproducible objects and executables, correct initialization and cleanup, FFI, thread/runtime integration, debug locations and variable descriptions, reliable stack walking, profiling, code-memory protection, and a deployment story. A frame pointer helps stack walking; it does not supply DWARF variable locations, Windows unwind records or a source debugger. Hot replacement additionally needs safe publication and old-code lifetime management, not merely overwriting a function address. The existing design's direct static calls and indirect development calls are a useful separation; release binaries need not permanently pay the hot-swap indirection cost.

Second, compile speed is end to end. Lexing, parsing, checking, monomorphization, comptime execution, MIR/LIR construction, runtime preparation, debug emission, archive scanning and final linking all remain on the path. The handoff's million-line, ten-second target is an owner goal, not a throughput measurement. Its suggested pre-indexed runtime pack and lazy section loading address a different bottleneck from instruction selection. Caching a function's machine code cannot help when every edit still reparses everything or eagerly decodes every runtime section. If a backend consumes one half of a build, making that half ten times faster yields only about a 1.82-times end-to-end speedup. Nothing in Jai or Zig proves Jet's corresponding fraction.

Third, broader platform support and higher final-code quality are continuing bodies of work. They are not implied by completing Linux x86-64. Nor does the backend finish the whole language product: library correctness and performance, package distribution, IDE usability, application tooling and security remain their own responsibilities. This report estimates the code-generation work, not the entire Jet ecosystem.

Self-hosting is a fourth, separate property. A compiler written in Jet might still invoke rustc or LLVM; a compiler written in C++ might emit every instruction itself. Jai is an example of the latter. Jet-owned instruction selection does not by itself retire the Rust-built runtime or all bootstrap dependencies.

## What the comparison systems actually demonstrate

### Jai: copy the architecture, not an unqualified speed claim

The Jai community's overview explicitly describes an internal bytecode/IR with two backends: a fast, naive x64 generator and a slower, better-optimizing LLVM generator. It reports 250,000 lines/second for beta 0.0.045 on the Sokoban game and describes one million lines/second as a goal. These are community reports of a proprietary compiler, not independently reproduced measurements or a published backend implementation. They establish the reported architecture, not a portable performance guarantee. [1]

A separate community book describes `-x64`, `-llvm` and release builds and gives example compiler/link times. Importantly, its small hello example has similar total times for both backends and linking dominates; it also says LLVM is the default. Therefore “like Jai” should mean having a fast native development path and an optimizing production path, not assuming that Jai literally reserves LLVM exclusively for release in every configuration. Public descriptions can also lag the beta. Jet cannot infer Jai's current target coverage, optimizer completeness or staffing cost from these pages. [2]

The transferable lesson is that the frontend can supply one semantic representation to different lowering strategies. The non-transferable part is a lines/second headline without matched source, comptime workload, safety checks, machine, debug settings and link scope.

### Zig: the strongest directly relevant development-build evidence

Zig's 0.15.1 release notes say the self-hosted x86-64 backend became the default for Debug, except on NetBSD, OpenBSD and Windows because of linker deficiencies. They report roughly five-times lower compilation time than LLVM in most cases, while explicitly acknowledging slower emitted machine code. This is a development-throughput trade, not release-quality parity. The same notes describe AArch64 as unfinished in that release; passing many backend tests is still different from making it the default for real applications. [3]

They also report a separate threading improvement: compiling Zig with its native backend fell from 13.8 to 10.0 seconds on one system, a 27% reduction. Semantic analysis, code generation and linking overlap, with code generation split across threads. This supports investing in pipeline organization rather than treating backend pass count as the only compile-speed lever. The numbers are release-specific upstream observations, not Jet results. [3]

The 2020 design essay coauthored by Andrew Kelley describes declaration-granularity binary patching, indirect function references, and the repeated work needed for ELF/DWARF, PE/PDB, Mach-O and Wasm. Read beside the 2025 release notes, it is also a warning about schedules: an attractive prototype and architectural plan can precede a dependable default by years. [4]

### Go gc: a successful independent optimizing compiler, with a different contract

Go's compiler documentation describes early devirtualization, inlining and escape analysis before lowering language constructs, followed by generic SSA simplification, architecture-specific rewriting, dead-code elimination, scheduling values closer to their uses, register allocation, frame layout and machine-code emission. Its export format includes inlineable bodies, generic bodies and escape summaries, with lazy indexed decoding. An independent backend need not be a naive emitter; a limited, coherent optimizing pipeline can support a widely deployed systems/application language. [5]

Go also demonstrates that useful release improvement does not require replacing that backend with LLVM. Its PGO guide reports about 2–14% improvement on a representative set of programs in Go 1.22 and explains profile-driven inlining. This is a whole-PGO effect, not a percentage attributable to inlining alone. [6]

Go is not proof that Jet will compile equally quickly or beat LLVM. Its runtime, garbage collector, type system, generics strategy and optimization choices differ. Borrow/lifetime checks, Jet's exact integers and representation choices impose different costs. Transfer the export summaries, SSA and budgeted optimization ideas; do not transfer the headline performance without matched workloads.

### Cranelift: a useful middle point, including its e-graph mid-end

Cranelift is an optimizing compiler designed around fast, predictable compilation rather than LLVM's breadth of peak optimization. Its 2022 report describes x86-64, AArch64, s390x and newly added RV64GC support. WebAssembly is a major input language for Wasmtime; this does not mean Cranelift emits Wasm as a target backend. That input/output distinction matters to Jet's web story. [7]

The e-graph work is especially relevant. Cranelift keeps a control-flow/side-effect skeleton and represents alternative pure expressions with acyclic e-graphs. Eager rewrites and scoped elaboration choose and place expressions, subsuming parts of GVN and LICM without unrestricted equality-saturation fixpoint search. The report describes about 16% faster execution of SpiderMonkey.wasm at about the old optimizer's compile cost. That is a particular workload and an optimizer transition inside Cranelift, not “16% closer to LLVM” and not a universally available improvement. [7]

Likewise, regalloc2 reduced compile time 10–20% and improved runtime by up to 7% in the reported comparisons. This is evidence that better allocation can improve both axes, not evidence that register allocation contributes 7% of LLVM's total gain. The report emphasizes differential fuzzing and an independent IR interpreter; the allocator transition and the instruction-selector migration required substantial integration effort. [7]

ISLE eventually expressed lowering patterns for four architectures and optimizing rewrites. Its designer reports a year-long migration and 27,000 lines of DSL. This supports declarative, checkable lowering rules once a pattern library becomes large, but does not justify giving Jet a new metacompiler before handwritten patterns become a real maintenance problem. [8]

Jet's existing decision to retire its Cranelift execution adapter is not reopened here. Cranelift is prior art for Jet's native optimizer and correctness techniques, not a proposal to maintain a second runtime semantics path.

### QBE: deliberately stop before diminishing returns

QBE's stated aim is 70% of industrial compilers' performance in 10% of the code. That is a design aspiration, not a benchmark conclusion that Jet can budget as a guaranteed 30% loss. Its public description lists SSA, sparse conditional constant propagation, dead instruction elimination, registerization of small stack slots, copy elimination, loop-sensitive spilling, a linear allocator and x86 addressing-mode matching. Its targets include amd64, arm64 and riscv64, with Linux/macOS mentioned for amd64. It emits assembly for a host assembler/linker rather than supplying a complete replacement platform toolchain. [9]

The lesson is a constrained O1 feature budget. QBE explicitly declines the endless quest for the last few percent. Jet can use that discipline for development builds, but it cannot declare QBE-style runtime trade-offs sufficient for its strict release performance gate. QBE's list is not evidence of Windows PE/PDB support or sophisticated automatic vectorization.

### Makarov's MIR: a compact optimizing pipeline is real

MIR here means Vladimir Makarov's independent project, not Jet MIR or Rust MIR. Its documented pipeline includes inlining, SSA, GVN/redundant-load elimination, dead stores/code, register-pressure-aware motion, coalescing, instruction combination and priority-based linear scan with live-range splitting. The project explicitly prefers simple implementations over maximum generated-code performance. It has x86-64, AArch64, ppc64le, s390x and riscv64 generators. Its platform disclaimer is narrower than a universal deployment guarantee. [10]

Its published sieve comparison on an i5-13600K reports 249 microseconds for MIR generation versus 27.1 milliseconds for GCC -O2; generated execution takes 1.74 seconds versus GCC's 1.6 seconds. Crucially, the compiler timing compares already-formed MIR against C compilation, and small-input startup costs matter. These results show that compact optimizing code generation can be very fast and competitive on a scalar kernel. They do not establish a 109-times end-to-end Jet build improvement or LLVM parity across applications. Makarov's accompanying article also warns that disabling compiler passes does not proportionally eliminate initialization costs. [10, 11]

### TPDE: LLVM IR does not inherently require LLVM's slow backend

TPDE separates access to an existing SSA IR from a fast compilation framework. Its paper describes one analysis pass followed by a pass combining selection, allocation and encoding. Its LLVM-IR adapter compiled SPECint 2017 inputs 8–24 times faster than LLVM -O0 with comparable runtime performance. Those are backend measurements against -O0, not comparisons against LLVM -O2/-O3 release output. [12]

The project documentation describes ELF-based x86-64 and AArch64 support and a roughly 10–20-times LLVM -O0 speed advantage. It also distinguishes the reusable core from TPDE-LLVM and LLVM-based encoding-generation tools; the core can be built without LLVM. [13]

For Jet, the architectural lesson is to avoid repeatedly translating and copying IR when a backend can read its SSA representation through a small adapter. TPDE is also a possible experimental comparator for whether an LLVM-IR seam itself is expensive. Adopting its C++ implementation, build tools or dependencies would require an owner decision; it is not the recommended replacement for the already-directed Jet-authored backend. It supplies neither evidence of release-quality optimization nor all required object formats.

### Copy-and-patch: exceptional baseline speed, with a stencil cost

Xu and Kjolstad's copy-and-patch compiler generates code by copying precompiled binary stencils and patching holes for values, addresses and branches. Their paper reports two orders of magnitude faster compilation than LLVM -O0 and three than higher optimization levels for its TPC-H query setting, with code 14% faster than LLVM -O0. Its Wasm compiler was 4.9–6.5 times faster to compile than Liftoff and 39–63% faster to execute on the stated Coremark/PolyBenchC tests. These are specific baseline/domain comparisons, not a claim to beat LLVM release optimization. [14]

The heavy compilation work moves to stencil construction. Each ISA, ABI and relevant operation/register-state variant needs a suitable library and correct patch rules. Crossing stencil boundaries can restrict global register allocation, vectorization and instruction combination; increasing the variant set raises generator complexity and footprint. [INFERENCE] A narrowly defined comptime evaluator or query compiler might benefit, but a second whole-language stencil backend would duplicate much of Jet's existing native work. Do not add it merely because it wins a baseline benchmark. Revisit only if measured O0 generation latency dominates and the stencil footprint is acceptable.

### LuaJIT, V8 and HotSpot: narrow fast compilers can coexist with expensive optimization

LuaJIT combines an assembly interpreter with a trace compiler, SSA optimization and tuned native generators. Its project documentation describes performance reaching the range of static compilers after removing dynamic-language overhead. DynASM is an assembler/code-generation aid, not a general optimizer or cross-platform object/link toolchain. LuaJIT's success shows how strongly a narrow IR, chosen hot paths and carefully tuned backends can perform; it does not imply a general Jet AOT backend can reproduce its wins. Its CPU/OS support also cannot be read as evidence that arbitrary AOT object emission exists for those combinations. [15, 16]

V8's documented Ignition → Sparkplug → Maglev → TurboFan hierarchy is a particularly clear cost continuum. Sparkplug is near-immediate baseline code generation. Maglev uses a small SSA/CFG IR, performs work during graph construction, selects representations and uses simple allocation. V8 reports Maglev compiling about ten times slower than Sparkplug and ten times faster than TurboFan. TurboFan's reported 4.35-times JetStream improvement is against the interpreter, not against a statically typed LLVM AOT program. [17]

HotSpot uses the client compiler C1 to execute and collect profiles before the optimizing server compiler C2 takes over. Oracle documents faster warm-up and potentially better peak performance through better profiling, but also a fivefold default code-cache enlargement for tiered compilation. Escape analysis can eliminate scalar-replaceable allocations and locks. [18]

Jet should borrow the separation of budgets and profiling, not automatically the speculative runtime machinery. It already knows types, ownership and generic instances statically. A recorded production profile can help release inlining and layout without carrying a compiler, deoptimization maps and code caches in every binary. The adaptive-specialization report covers the remaining advantage of fresh runtime constants and changing workloads. An onboard optimizer is an optional deployment mechanism, not a prerequisite for Jet's own AOT machine code.

## Where LLVM's optimization gains come from

There is no defensible universal fraction such as “inlining contributes 40%, SROA 20%, and the remaining passes 40%.” The fraction depends on the frontend's IR, benchmark, target CPU, runtime visibility, optimization level, profiles and which baseline counts as unoptimized. Passes enable and undo one another. Inlining can expose an allocation to SROA; SROA exposes scalar constants to GVN; simplification enables vectorization; vectorization changes scheduling and register pressure. Crediting the full resulting gain to every enabling pass double-counts it.

LLVM's release/20.x pipeline source explicitly runs SROA and EarlyCSE early, repeats LICM around loop rotation, runs later SROA after unrolling, applies GVN, SCCP and dead-bit elimination, and runs InstCombine and CFG cleanup repeatedly. Its comments explain several of these ordering dependencies. The new pass manager organizes module, call-graph-SCC, function and loop work; a list of pass names is not a list of independent investments. [19, 20]

GCC makes the same trade in a different architecture: tree/GIMPLE analyses and transformations precede RTL/machine work. Its documented -O1 enables scalar replacement, constant/copy propagation, dead-code/store elimination, loop motion and limited inlining; -O2 adds broader inlining, redundancy elimination, vectorization and scheduling; -O3 adds more loop reshaping and a different vector cost model. Exact defaults depend on target/version. This is evidence for tiers, not for assuming every additional pass improves every program. [21]

There is useful empirical evidence, but it must retain its scope. Bruzzone and Cazzola's June 2026 preprint studies LLVM 21.1.8 using 113 cumulative -O3 pass prefixes on 30 PolyBench/C kernels on one Alder Lake system. It reports a strongly skewed impact distribution: EarlyCSE has significant influence on 27 of 30 kernels; LICM, InstCombine, loop vectorization, rotation, unrolling, CFG simplification, SROA and induction-variable simplification dominate its ranking. Loop vectorization has large impact on about six kernels. The median non-regressing kernel needs 84.8% of the pipeline to reach 80% of its eventual speedup; 6.6–9.7% of transitions regress. This is a preprint, and prefix deltas measure contextual marginal effects, not causal standalone pass shares. [22]

That study does not settle Jet's allocation-heavy application workloads, broad inlining gains, or the contributions of machine scheduling and register allocation: its ablation is over IR passes, not independently varied machine-backend pipelines. Neither its “Pareto core” nor QBE's design slogan warrants promising that a small scalar optimizer obtains 80% of LLVM's gains everywhere.

For each requested pass, the practical assessment is as follows.

Inlining removes calls, but its larger value is exposing constants, layouts, control flow and ownership across boundaries. It can make closure/trait dispatch disappear and unlock allocation removal. It can also increase build time and instruction-cache pressure dramatically. Small, budgeted, profile-directed inlining is a high-value Jet investment; whole-program uncontrolled expansion is not.

SROA and related promotion turn addressable aggregates/local slots into independent SSA values. They are disproportionately important when the frontend emits every local through memory or boxes every aggregate. LLVM SROA itself primarily works on allocas; removing opaque runtime heap boxes additionally requires escape/allocation reasoning or higher-level representation lowering. Jet should avoid unnecessary boxes before asking a generic optimizer to rediscover values. Once locals already arrive as SSA values, part of the apparent LLVM gain has been obtained by construction, although aggregate scalarization still matters.

GVN/EarlyCSE eliminate repeated computations and redundant loads. Their gain grows with exposed scalar dataflow and accurate memory clobber facts. Local value numbering is cheap; global memory redundancy and partial redundancy elimination need more analysis. LLVM's empirical EarlyCSE ranking supports an early, simple version, not an invented universal percentage for GVN.

LICM moves loop-invariant work outside loops and can promote repeated memory access. Its benefit ranges from nothing to eliminating almost all repeated work in a pathological loop. It must respect aliasing, conditional execution, trapping operations and cleanup. Hoisting everything legal can still lose by lengthening live ranges and causing spills. Makarov's pressure-aware approach and Cranelift's placement work are useful guides.

Vectorization can dominate numerics, image processing, game kernels and scans when operations and dependence proofs permit it. It can contribute nothing to an I/O-bound service or pointer-chasing loop. It requires more than inserting SIMD opcodes: dependence tests, legal arithmetic, masks, scalar tails, reductions, target features and a profitability model. LLVM documents runtime pointer-disjointness checks and the difficulty of reordering floating-point reductions. Jet's stronger facts can remove some versioning overhead; they cannot make an intrinsically serial recurrence parallel. [23]

Scheduling controls latency hiding, dependency order and register pressure after instruction selection. Out-of-order processors do some of this dynamically, but compiler scheduling still affects critical paths, spills and code layout. The benefit is architecture/workload dependent and can be larger on simpler cores. Nothing reviewed supplies a portable percentage. LLVM's machine-code pipeline explicitly places scheduling and machine optimizations on both sides of allocation. [24]

Register allocation is necessary even at O0. The optimization question is the improvement from a cheap allocator to one with splitting, coalescing, rematerialization and better spill choices. Call-heavy code, SIMD code and high live-value pressure can make this decisive. Cranelift's up-to-7% runtime improvement is a concrete local comparison, not a pie-chart fraction of LLVM optimization. Jet's dated design uses conservative whole-value intervals and saves special instruction registers; splitting, physical constraints and separate floating/vector register classes are natural quality improvements, with real correctness costs.

Important gains also come from outside this requested list: library/runtime algorithms, allocation and copy elimination, bounds-check proofs, devirtualization, strength reduction, dead stores, branch/layout choices, LTO and PGO. LLVM cannot recover an algorithm that the frontend already erased into an opaque runtime call.

To answer “what fraction” for Jet, the proposed experiment is a fixed corpus and CPU matrix, not another literature average. Feed identical optimized Jet MIR to both code-generation routes. Measure the current release route separately from a direct LLVM-IR experiment so Rust frontend cost is not mislabeled LLVM backend cost. Compare native O0, native O1 and LLVM O0/O2/O3 under the same language, runtime, debug and CPU contracts. Record compiler phase time, peak memory, binary size, runtime, allocation/copy counts and correctness.

Then use both cumulative prefixes and leave-one-pass-family-out experiments, keeping the backend fixed, and separate backend-quality experiments keeping MIR fixed. Report interactions and regressions rather than forcing contributions to sum to 100%. If a descriptive share is useful, divide a family's measured time saving by the total baseline-to-release time saving for that particular cell, naming whether it is a prefix or removal delta. Shares may be negative or exceed 100%, and are undefined when the total gain is zero. Inlining × scalarization and vectorization × allocation need paired experiments. The existing required-cell policy must remain intact; a research kernel is not a substitute for an application cell.

## What Jet knows that generic LLVM does not automatically know

“No aliasing by default” needs a precise reading. Jet's law allows many overlapping read windows, but requires exclusive write windows and rejects moves/resizes that invalidate live windows. `Shared<T>`, arena IDs, unsafe pointers, FFI and callbacks remain important boundaries. Thus not every pair of Jet pointer values is disjoint. The [memory and copy decisions](../spec/syntax-decisions.md) distinguish read, write, take, independent copies and last-use moves; do not infer blanket `noalias` from a type name.

[INFERENCE] A Jet-specific optimizer can preserve place-root/projection identities and borrow lifetimes, then prove a write cannot affect a particular read. That can simplify dependence tests, remove bounds checks while an owner's size is stable, promote fields and eliminate load reloads across appropriately summarized calls. Overlapping read views can share; mutually exclusive write regions can avoid runtime disjointness guards. It must invalidate facts at mutation, escape, sharing or uncertain foreign calls.

Value semantics and explicit/derived ownership give Jet an unusually useful location for eliminating copies. A last-use transfer can forward storage; a temporary aggregate that never escapes can be scalarized; a borrowed parameter need not be cloned just to call a read method; a uniquely owned buffer may be reused when identity, lifetime and error observations permit it. These transformations are easiest before layouts and runtime cloning become low-level calls. The backend handoff's ownership bugs show why such rewrites need consumption/drop proofs rather than optimistic pointer reuse.

Effect information is helpful but not equivalent to machine-level “has no side effects.” Jet's effects document includes ambient capabilities, while panic and memory facts have their own treatment; deterministic mutable capabilities can be pure-callable. An empty ambient-effect row does not alone prove termination, non-trapping execution, freedom from mutation through a write parameter, or permission to discard allocation/cleanup. A sound optimizer needs read/write footprints, capture/escape summaries, failure and drop behavior as well as the effect row. The [MIR dead-value transform](../../Compiler/JetOptimizer/Source/Transforms/DeadValues.jet) separately checks infallibility for Prelude routes, and the [loop fact derivation](../../Compiler/JetOptimizer/Source/Facts/LoopVector.jet) separately requires no aliasing, no cross-iteration dependencies, no early exit and acceptable effects. Source inspection establishes these design distinctions, not native SIMD execution proof.

[INFERENCE] With those summaries, Jet can fuse traversal operations, specialize exact-integer fast paths using proved ranges, eliminate temporary containers, and choose layouts while language-level element/callback order is still available. Comptime and monomorphization can remove dispatch before a backend sees it. Exact `Int` is not an LLVM fixed-width integer with arbitrary wrap/undefined overflow: tagged-bigint fallback and observable failure ordering must remain correct. Floating-point reassociation likewise needs an existing permitted numerical contract, not a silent fast-math setting.

What can LLVM not do? It cannot infer missing Jet ownership, lifetime, callback-order or container laws from an opaque pointer and an external function symbol. It cannot retroactively change a public layout or unsafe/foreign contract merely for speed. It does not automatically understand Jet's value-copy or exact-integer abstractions. That is a loss of information, not a fundamental inability to emit fast instructions.

LLVM can exploit much of the information if Jet supplies it correctly. Its IR has `noalias`, capture restrictions, memory-effect attributes, alignment, lifetime intrinsics, alias scopes, range/assumption and loop metadata, and vector IR. Its `noalias` definition is specific and stronger than casually saying “these owners are different”; incorrect annotations introduce undefined behavior. Jet should map proven facts to those mechanisms or perform high-level transformations before LLVM lowering. [25]

Consequently, the strongest initial competitor to “Jet optimizer plus LLVM” is not a wholly independent machine optimizer. It is a better Jet frontend/mid-end feeding LLVM. An owned machine backend mainly buys latency, control, dependency size and predictable compilation. Semantic advantages should improve both paths; withholding them from LLVM to manufacture a native win would answer the wrong question.

## Platform reach is several projects, not four encoders

x86-64, AArch64 and RISC-V need instruction encoders, selection patterns, immediate/address legalization, register constraints and allocation, ABI classification, atomics/memory-order lowering, frame/unwind rules, relocations and CPU-feature policy. AArch64 also needs correct instruction-cache synchronization when generating code at runtime. RISC-V requires an explicit extension baseline and careful relocation/relaxation policy; “supports RISC-V” without an ABI and extension set is not a deployable contract.

Linux normally means ELF and its relocation, symbol, section, archive, TLS and dynamic-link conventions, plus DWARF/unwind information where required. A static freestanding ELF that exits via syscalls is much smaller in scope than PIC libraries, libc integration, dynamic loading and debuggable application binaries.

macOS needs Mach-O/dyld conventions, SDK/runtime linkage, debug/unwind behavior, code signing and appropriate restrictions on JIT mappings. Apple AArch64 differs from generic AArch64 ABI in relevant details; Apple's documentation specifically warns about different varargs placement and weaker memory ordering than Intel. Reusing the Linux emitter and changing an object header is insufficient. [26]

Windows needs COFF objects/PE images, imports/exports and CRT policy, Windows calling conventions, stack probing, unwind metadata and a debugger format strategy. The x64 convention passes integer arguments in RCX/RDX/R8/R9, reserves shadow space, and restricts prolog/epilog shapes for unwindability. These are not the Linux System V argument rules. Debugging can use an explicitly supported format/tool combination; a first-class Visual Studio experience entails further PDB/CodeView integration. [27]

WebAssembly is a different output machine: structured control flow, locals and operand stacks, linear-memory pointer width, imports/exports, validation and host/runtime integration. It does not use native physical-register allocation or ELF/Mach-O/PE. The interoperable Wasm object convention includes linking metadata and index/address relocations, and feature incompatibility can cause validation failure. The browser/runtime ultimately chooses native scheduling and allocation. Generating Wasm quickly and generating the best native x64 code are not the same optimization problem. [28]

A reusable middle-end and per-target ABI/layout seam reduce duplicate work. They do not eliminate these obligations. Existing system linkers are a sensible boundary where authorized: owning machine code does not require owning ld, dyld and link.exe replacements immediately. An LLVM cross-target generator also does not supply a missing sysroot, runtime port or platform SDK. Both native and LLVM options need an end-to-end deployment proof.

## Options and their trade-offs

The lowest-risk option is the current hybrid: Jet O0/O1, Rust/rustc/LLVM release initially, with high-level Jet optimization shared. It offers fast feedback without giving up mature release scheduling, SIMD, register allocation and target breadth. Its costs are two machine-code paths to test, a larger release toolchain dependency, and the Rust-emission seam's translation/control-flow/diagnostic complexity. Eventually going directly from an appropriate typed Jet IR to LLVM IR can remove a redundant frontend and preserve facts more directly, but that is a distinct integration project and owner decision, not necessary to prove native O0.

The recommended extension is a bounded Jet O1, roughly inspired by Go/QBE/MIR and Cranelift's scalar mid-end: cheap canonicalization, constants, dead code/stores, budgeted inlining, copy/aggregate elimination, value numbering, cautious loop motion, better instruction combination and better allocation. Keep a strict compile-work budget, use function-local data and parallelism, and cache lowered bodies with complete semantic/target keys. This captures many common scalar opportunities without promising automatic SIMD parity. It is a useful destination even if LLVM is never retired.

A stronger hybrid would use Jet semantic/loop transformations and LLVM for remaining release optimization and machine work. This is likely the best runtime-performance return per engineer-month: Jet supplies proofs and eliminates high-level costs; LLVM supplies target-specific heuristics. Profile-guided inlining/layout can improve it without shipping a JIT. Release caching and parallel codegen can reduce the compile-time penalty, although cross-function inlining introduces invalidation and scalability trade-offs.

A fully independent Jet release optimizer is possible. It offers control of every pass, predictable dependency policy and potential domain-specific wins. It commits Jet to long-term cost-model tuning, SIMD legality/profitability, scheduling, allocator quality, ISA evolution and miscompilation response across platforms. Matching LLVM on a selected scalar application suite is a reachable engineering aim; replacing LLVM's whole envelope is a continuing team responsibility, not a one-time backend task. It should be funded only when a direct comparison identifies release gaps that LLVM integration cannot economically close.

External compact engines or stencil techniques are alternatives for a scoped research experiment, not an automatic fifth production path. TPDE may isolate baseline backend cost; QBE and MIR offer inspectable compact designs; copy-and-patch targets extremely low generation latency. Introducing any of them into shipped Jet would trade implementation time for dependency, language/toolchain and platform-fit constraints. Do not maintain several semantic adapters just to avoid choosing an O1 budget.

## Effort estimates

All estimates in this section are [INFERENCE], not quotes, commitments or measured agent throughput. One person-month means a full-time experienced compiler/toolchain engineer for a month. Ranges include design, implementation, focused correctness/performance evidence and integration; they exclude the already-built baseline emitter, frontend language feature development, a complete Core port and third-party SDK work. They assume stable semantics, a usable Jet compiler, access to target machines and reusable test infrastructure. Immature bootstrap/runtime tooling moves the result toward or beyond the high end. Additional people shorten elapsed time only where the work can be divided cleanly.

After a functionally complete initial Linux x86-64 backend, budget roughly 6–12 person-months to make O0 a dependable development product: integration, useful debug/source information, lifecycle/W^X behavior, deterministic objects, ABI edge cases, linear-scale linking/runtime-pack access and differential regressions. Add about 4–8 for end-to-end incremental compilation/cache invalidation and parallel pipeline work if those seams are not already dependable. These are not estimates of how long today's unsupported constructs take to finish.

A bounded, release-tested native O1 scalar optimizer is roughly 12–24 additional person-months. Budgeted inlining and ownership-aware scalarization/copy removal take a large share; GVN/LICM and allocator splitting need correctness work beyond adding a rewrite. A small acyclic e-graph experiment would be roughly 3–6 person-months within or in addition to that effort, depending on whether it replaces existing analysis; it is not required to start O1.

Maintaining the existing rustc/LLVM release route is an ongoing integration cost rather than a fresh backend. A direct Jet-to-LLVM route, if approved, is roughly 6–12 person-months for the existing language/runtime contract on an initial target, and 3–6 more for high-quality ownership/effect metadata, profiles and release-pipeline tuning. This is not a quote for recreating LLVM. It buys access to its maintained machine generators but does not eliminate Jet runtime/ABI validation on other platforms.

For additional production baseline native targets, estimate 6–12 person-months for AArch64/ELF and 6–12 for RV64/ELF at an explicitly chosen scalar extension baseline, using the shared IR and allocator framework. A Wasm emitter plus runtime/host integration is roughly 6–12, distinct from native machine emission. Each new target also needs recurring SIMD/atomic/debug work if it is to reach release quality. A minimal private JIT encoder can be substantially cheaper; it is not the deliverable estimated here.

For formats/platforms, estimate a further 5–10 person-months for macOS Mach-O/ABI/deployment integration and 8–16 for Windows COFF/PE/ABI/unwind/debug integration. These assume using existing linkers where permitted, not writing full system linkers. ISA and platform costs overlap at their seam, so they should not be mechanically added without a concrete scope. An independent mature linker, especially dynamic linking and incremental debug handling, can add another 12–24 or more per substantial format family.

For an independent optimizing release tier on the initial native ISA, allow roughly 48–96 additional person-months for competitive scalar/interprocedural optimization, stronger allocation, instruction selection/scheduling, cost modeling and broad regression evidence. Serious automatic SIMD/loop optimization adds roughly 24–48. Extending comparable optimized quality across additional ISAs and formats can add 24–60 or more beyond baseline port costs. A broad LLVM-replacement effort is therefore plausibly 120–240-plus person-months overall, followed by a permanent team; no finite estimate guarantees beating LLVM on every required cell. A focused domain optimizer may produce a win far sooner without incurring that entire scope.

An exploratory copy-and-patch implementation for a narrow, frozen operation set on one ISA is roughly 3–6 person-months; shipping it as an additional whole-language tier can require 12–24 or more, plus ports and ongoing stencil maintenance. An opt-in runtime re-specializer is another roughly 12–24 beyond dependable codegen/profiling for guarded entry specialization and lifecycle, with full OSR/deoptimization potentially 24–48-plus. Neither belongs in the minimum path to independent AOT emission.

As a practical staffing shape, two or three strong engineers can pursue dependable O0, bounded O1 and initial release integration over several quarters, while platform/toolchain specialists work on separate targets. A broad independent release replacement is a multi-year, multi-engineer effort. Plan recurring allocator/ISA/ABI/performance maintenance rather than treating source-code completion as the end. Zig's multi-year path, Cranelift's year-long multi-contributor migrations and target additions, and MIR's narrow purpose support the caution, but do not numerically validate these estimates.

## Recommended decision gates

The first milestone is a dependable native development path on the initial platform. The acceptance evidence should cover the same language/Core observations and failure/cleanup behavior as release, correct runtime boundaries, debuggable source locations and stacks, deterministic output, bounded memory and measured compile/link throughput on small, large and incremental workloads. The million-line target needs named hardware, source composition, debug settings, runtime-pack state and cold/warm-cache distinctions. It must not be “met” by excluding checking, comptime or the linker. This is productization of the directed backend, not authorization to narrow semantic parity.

The second milestone is a bounded O1 plus a release-quality reference. Measure native versus the existing LLVM-backed release route with the same optimized MIR and runtime meaning. Prioritize measured copies/allocations and scalar code-quality losses before building a vectorizer. At the same time, evaluate whether direct LLVM IR materially improves facts, build time and maintainability over Rust emission. This comparison may justify the direct route; it may also show that fixing the existing route is sufficient. Any shipping choice or new dependency goes to the owner.

The third milestone is end-to-end platform proof, selected in owner priority order. AArch64 plus macOS is an attractive desktop combination; Windows may deserve priority if it dominates the intended audience. ELF/RV64 and Wasm are separate subsequent proofs, not hidden behind an x64-only performance claim. Every chosen combination needs real runtime, FFI, debug, object and deployment evidence. Share MIR optimization, layouts and ABI facts; do not create target-specific language semantics.

The fourth milestone is a decision on independent release optimization, based on a pass-family and workload gap analysis. If Jet-specific transformations plus LLVM meet the performance needs, keep that architecture and focus native investment on feedback latency. If LLVM repeatedly misses valuable Jet-specific opportunities, first determine whether preserving semantics or pre-lowering the transformation fixes them. Only the remaining machine-quality or dependency problem justifies a broader native release tier. Keep LLVM as an independent comparator until the complete selected corpus and platform scope pass; a single faster kernel is evidence for that kernel, not authorization to retire the release backend.

Runtime profiling is complementary to these milestones. Prefer an explicit, reproducible profile input for release optimization; revisit onboard specialization only for workloads where the adaptive-specialization proposal demonstrates an advantage over a well-profiled prebuilt binary. Never attach a mandatory optimizing compiler to every small CLI binary merely to have a tiered architecture.

## Questions for the owner

Is the desired independence “no LLVM/rustc in everyday builds,” “no Rust-built runtime,” or “no external compiler/linker anywhere, including release”? Each is a different endpoint and budget. The first does not require the third.

For development O1, what runtime slowdowns are acceptable relative to release, and on which actual workloads? This is an explicit development-tier trade-off question, not a request to weaken the strict release performance gate. Debug fidelity and optimized variable visibility also need a product choice.

Which platform combinations must have Jet-owned fast builds first: Linux x86-64, Apple AArch64/macOS, Windows x86-64/AArch64, RV64 Linux or Wasm? Is a supported LLVM-backed path on an additional platform acceptable until its native backend has full parity? No exception is assumed here.

Should the release seam remain Rust/rustc while native development stabilizes, or should direct LLVM IR become the next compiler-integration investment? Is LLVM an allowed optional release dependency in the long-term dependency policy?

What is the exact scope of “beat LLVM”: Jet programs compiled through Jet's best LLVM route, a matched peer-language suite, selected application domains, or every required performance cell and target? The first isolates backend quality; the second also measures language/runtime design; they should not be conflated.

How much recurring staffing should go to peak machine-code quality rather than frontend throughput, Core algorithms, platform reach and developer tooling? What evidence would justify the 120–240-plus-person-month independent release program instead of a 12–24-person-month bounded O1?

Which numerical, failure, allocation-observation and hot-reload contracts may optimization exploit, and which must remain exactly observable? Existing safety, ownership, effect and one-meaning-across-tiers rules remain binding. Any additional latitude must be ratified rather than silently borrowed from C/C++ undefined behavior or fast-math.

## Sources and evidence boundaries

Primary project documentation and papers were preferred. Upstream numbers below are attributed results, not measurements made on Jet. Jai's closed compiler necessitates secondary sources. Estimates, proposed transformations and recommendations are explicitly engineering inference. No build, test, formatter or performance run was performed; this is prose-only research.

[1] [Jai community overview, performance and compiler internals](https://github.com/Jai-Community/Jai-Community-Library/wiki/Overview). Secondary, mutable beta documentation; architecture and reported throughput, not independent reproduction.

[2] [The Way to Jai, chapter 4: compiler, backends and linking](https://github.com/Ivo-Balbaert/The_Way_to_Jai/blob/main/book/04A_More_info_about_the_compiler.md). Secondary examples; not a controlled backend benchmark.

[3] [Zig 0.15.1 release notes](https://ziglang.org/download/0.15.1/release-notes/), compiler sections “x86 Backend,” “aarch64 Backend,” “Incremental Compilation” and “Threaded Codegen.” Read through the [official website source](https://raw.githubusercontent.com/ziglang/www.ziglang.org/master/src/download/0.15.1/release-notes.html) because the rendered site returned HTTP 403. Release-specific historical evidence.

[4] Loris Cro and Andrew Kelley, [Zig's New Relationship with LLVM](https://kristoff.it/blog/zig-new-relationship-llvm/), 2020-09-28. Design intent and the platform/debug granularity problem, not proof of completion at publication.

[5] [Introduction to the Go compiler](https://go.dev/src/cmd/compile/README). Pipeline, machine generation and export summaries.

[6] [Go PGO user guide](https://go.dev/doc/pgo). Go 1.22 representative-program result and reproducible profile inputs.

[7] Chris Fallin, [Cranelift Progress in 2022](https://bytecodealliance.org/articles/cranelift-progress-2022). E-graph, regalloc2, target, caching and correctness results with the stated historical scope.

[8] Chris Fallin, [Cranelift's Instruction Selector DSL, ISLE](https://cfallin.org/blog/2023/01/20/cranelift-isle/), 2023-01-20. Implementation design and migration scale.

[9] [QBE compiler backend](https://c9x.me/compile/). Stated design aim, passes, supported targets and assembly interface.

[10] Vladimir Makarov, [MIR project README](https://github.com/vnmakarov/mir). Pipeline, targets, limitations and benchmark baseline/method details.

[11] Vladimir Makarov, [MIR: A lightweight JIT compiler project](https://developers.redhat.com/blog/2020/01/20/mir-a-lightweight-jit-compiler-project), 2020-01-20. Motivation, compiler initialization and language-boundary optimization costs.

[12] Schwarz, Kamm and Engelke, [TPDE: A Fast Adaptable Compiler Back-End Framework](https://arxiv.org/abs/2505.22610), 2025. SSA adapter, fused generation and SPECint 2017 backend comparison.

[13] [TPDE project documentation](https://docs.tpde.org/) and [repository](https://github.com/tpde2/tpde). Baseline quality aim, ELF x86-64/AArch64 support and component/dependency separation.

[14] Xu and Kjolstad, [Copy-and-Patch Compilation](https://arxiv.org/abs/2011.13127). Stencil method and query/Wasm measurements; ratios are not release-LLVM comparisons.

[15] [LuaJIT overview](https://luajit.org/luajit.html) and [platform status](https://luajit.org/status.html). Trace/SSA design and supported configurations.

[16] [DynASM](https://luajit.org/dynasm.html). Dynamic assembler role and scope.

[17] V8 team, [Maglev — V8's Fastest Optimizing JIT](https://v8.dev/blog/maglev), 2023-12-05. Chrome 117/M2 measurements and SSA/allocation design.

[18] Oracle, [Java HotSpot VM Performance Enhancements, Java 25](https://docs.oracle.com/en/java/javase/25/vm/java-hotspot-virtual-machine-performance-enhancements.html). Tiered compilation, code cache and escape analysis. C1/C2 names also appear in [OpenJDK JEP 165](https://openjdk.org/jeps/165).

[19] [LLVM 20 pipeline construction source](https://github.com/llvm/llvm-project/blob/release/20.x/llvm/lib/Passes/PassBuilderPipelines.cpp), especially function simplification and loop/vector pipelines. Source of actual ordering, not an exhaustive current-version pass inventory.

[20] [LLVM new pass manager](https://llvm.org/docs/NewPassManager.html) and [analysis/transform descriptions](https://llvm.org/docs/Passes.html). The latter warns its inventory is incomplete.

[21] [GCC optimization options](https://gcc.gnu.org/onlinedocs/gcc/Optimize-Options.html). Tier budgets and target/version-dependent defaults.

[22] Bruzzone and Cazzola, [A Multi-Dimensional, Per-Pass Empirical Study of the LLVM Optimization Pipeline](https://arxiv.org/abs/2606.31238), 2026-06-30; [full text](https://arxiv.org/html/2606.31238v1), sections III and IV-A. Preprint, LLVM 21.1.8, cumulative IR prefixes, 30 PolyBench kernels, one CPU; not a universal attribution of LLVM gains.

[23] [LLVM vectorizers](https://llvm.org/docs/Vectorizers.html), pointer runtime checks and reductions. Legality and numerical restrictions.

[24] [LLVM code generator](https://llvm.org/docs/CodeGenerator.html), code-generation stages. Selection, scheduling, allocation and late machine work.

[25] [LLVM language reference](https://llvm.org/docs/LangRef.html), parameter attributes and metadata. Exact `noalias`/capture semantics matter; documentation on main is version-sensitive.

[26] Apple, [Addressing architectural differences in macOS code](https://developer.apple.com/documentation/apple-silicon/addressing-architectural-differences-in-your-macos-code). Memory order, hardware/page differences and varargs.

[27] Microsoft, [x64 calling convention](https://learn.microsoft.com/en-us/cpp/build/x64-calling-convention?view=msvc-170). Shadow space, argument registers and unwindability.

[28] [WebAssembly object linking conventions](https://github.com/WebAssembly/tool-conventions/blob/main/Linking.md). Linking sections, relocations and feature validation; these are tool conventions, not the core Wasm specification.
