# Frontier research: make semantics useful without making them optional

[Master report](index.md) · [Historical foundations](01-foundations.md) · [Compiler assurance](04-trust.md)

The strongest research direction is a compiler that carries enough checked information to explain, optimize, and validate the same program. Jet already has ratified facts, a shared-lowering direction, inspection, and proof records. The opportunity is to connect them without duplicating meaning. This report answers questions 6–8 and separates paper results, proposed experiments, and executed model witnesses.

No paper artifact or proof assistant was executed in this campaign. No current Jet runtime result follows from the research. The fresh compiler baseline failed. A theorem about a paper's calculus does not establish the corresponding Jet feature.

<a id="q6"></a>
## Q6. The useful frontier is in effects, resource histories, semantic reuse, and checked optimization

**Direct answer.** Prioritize papers that solve a concrete information-loss problem in Jet: lost alias relations, hidden continuation reuse, order-insensitive resource facts, duplicated lowering, and optimizations whose legality is separate from their profitability. Do not add advanced syntax merely because a paper has a new type system.

### Types and resource behavior

| Primary work | Contribution and prerequisite | What it could improve in Jet | Important boundary and disposition |
|---|---|---|---|
| [Pure Borrow](https://arxiv.org/pdf/2604.15290), PLDI 2026 | Rust-style borrowing in Linear Haskell uses borrowers, lenders, lifetime tokens, and histories to represent physical mutation inside a pure account. | Study how lender restoration and disjoint mutation preserve a high-level value model. | The body leaves progress and bisimulation as conjectures; dependent claims must retain that qualification. Its parallel quicksort did not beat the industrial introsort comparator. Do not import the API or call the entire system proved. |
| [Law and Order for Typestate with Borrowing](https://arxiv.org/pdf/2408.14031) | An ordered partial monoid describes permitted operation traces. Ordered contexts prevent closing a resource before a deferred borrow uses it. | Express lifecycle obligations as transitions rather than independent flags. Check whether existing Jet facts can represent the same relation. | The calculus is sequential and permits divergence. Decidability depends on the resource algebra. User studies and broad performance evidence are absent. Research under [#2925](index.md#card-2925). |
| [From Linearity to Borrowing](https://www.andrewwagner.io/assets/papers/linbor.pdf) | Borrowing can be derived from a linear baseline using lifetime modalities, reborrowing, and semantic resource invariants. A naive callback borrow is unsound when a closure escapes. | Challenge callback, nested reference, and owner-restoration cases in Jet's existing memory model. | The presented termination and leak-freedom account depends on its calculus. Nontermination, non-lexical lifetimes, and interior mutation are not automatically covered. Use its counterexample shapes, not a new borrowing dialect. |
| [Polymorphic Reachability Types](https://arxiv.org/pdf/2307.13844) | Tracks one-step reachability and saturates it for overlap checks. Freshness may grow during evaluation. The simply typed system proves preservation of separation. | Preserve alias information through higher-order functions and module interfaces without pretending every value is uniquely owned. | Reachability is not uniqueness. The full bounded-polymorphic variant has undecidable subtyping; nested-reference qualifiers are constrained. Study a decidable fragment only if a real Jet case needs it. |
| [Bidirectional Higher-Rank Polymorphism with Intersection and Union Types](https://zenodo.org/records/14208223/files/paper_extended.pdf?download=1) | A bidirectional discipline controls where inference and checking occur for a richer type system. | A candidate method for local, actionable diagnostics when expert expressiveness exceeds global inference. | Richer expressiveness is not automatically better inference, editor latency, or beginner comprehension. No Jet translation or artifact replay was performed. Hold as prior art, not an adoption instruction. |
| [Bounded Sort Polymorphism with Elimination Constraints](https://hal.science/hal-05372721v2) | Explores constraints on elimination and sort-polymorphic definitions in a Rocq implementation. | Potentially useful if the compiler proof needs expressive but controlled generic proof terms. | The bounded collection obtained metadata and implementation availability, not complete theorem and quantitative evaluation bodies. No stronger conclusion is justified. |
| [Modular Borrowing Without Ownership or Linear Types](https://2024.splashcon.org/details/iwaco-2024-papers/5/Modular-Borrowing-Without-Ownership-or-Linear-Types), IWACO 2024 slides | Temporary freezing, tracked instances, and disjoint effects offer another account of scope and interference. | Ask whether an ownership complaint is really about interference or temporary permission, then solve it in the existing fact model. | The slides explicitly defer type-safe memory management and provide no complete formal proof or execution evidence. Do not equate the talk's “borrowing” with Jet memory safety. |

### Effects, protocols, and semantic composition

| Primary work | Contribution and prerequisite | Jet use | Limit that must travel with it |
|---|---|---|---|
| [Affect](https://iris-project.org/pdfs/2025-popl-affect.pdf), POPL 2025 | Affine typing tracks one-shot versus multi-shot continuation use. Mutable references can contain affine payloads; capture rules prevent duplicating them. | A rigorous challenge to any future handler or continuation design. A callback's ordinary effect set is not enough to authorize duplicating its continuation. | Safety is for its closed, well-typed calculus. Principal inference, a practical compiler, modules/GADTs, verified compilation, and optimization work remain future directions in the source. |
| [Modal Effect Types](https://2025.splashcon.org/details/OOPSLA/62/Modal-Effect-Types), OOPSLA 2025 | Absolute modalities replace an ambient effect context; relative modalities modify it. Boxing, masking, and restrictions prevent accidental handling and leakage. | Reduce repeated effect parameters while keeping higher-order behavior precise. Use as a model-comparison exercise against Jet facts and gates. | Full-body follow-up includes proof appendices and elaboration rules, but no replay. The value-polymorphic encoding has stated limitations; inference is not universally solved. |
| [Rows and Capabilities as Modal Effects](https://popl26.sigplan.org/details/POPL-2026-popl-research-papers/34/Rows-and-Capabilities-as-Modal-Effects), POPL 2026 | A parameterized modal framework encodes the considered row and capability systems with stated preservation results. | Test whether apparently separate effect mechanisms can be represented by one internal account. | “As expressive” applies to the selected formal systems, not all real languages. Inverse encodings, richer effects, and optimization are not established by that statement. |
| [Message-Observing Sessions](https://arxiv.org/abs/2403.04633) | Protocol types observe message relationships, not merely send/receive shapes. Trace constraints support compositional checks. | Model rules such as an acknowledgment corresponding to the actual request, or an event applying to the entity generation that produced it. | Core completeness excludes asynchronous FIFO, general value dependencies, recursion, and shared channels. Do not advertise distributed-system correctness from the finite core. |
| [Effects and Coeffects in Call-By-Push-Value](https://2024.splashcon.org/details/splash-2024-oopsla/93/Effects-and-Coeffects-in-Call-By-Push-Value) | Separates values and computations, and models produced effects alongside resource use. It explains why discarding a result does not justify discarding an effectful computation. | A useful semantic discipline for dead-code elimination, laziness, and cost reasoning. | The developed combination is a tick effect and resource coeffects. State, nontermination, and richer interactions are open extensions, not completed Jet-ready results. |
| [Session-Typed Effect Handlers](https://popl24.sigplan.org/details/POPL-2024-student-research-competition/5/Session-Typed-Effect-Handlers) and [Borrowing From Session Types](https://doi.org/10.1145/3763173) | Explore handler interactions as protocols and borrowing-based channel APIs. | Candidate counterexamples for task composition and resource transfer. | These captures are abstract/SRC or artifact-metadata level, not complete proof bodies. Keep them in the research queue rather than rank them as demonstrated replacements. |
| [Rhombus](https://2023.splashcon.org/details/splash-2023-oopsla/52/Rhombus-A-New-Spin-on-Macros-without-All-the-Parentheses), OOPSLA 2023 | Conventional notation, token-tree structure, binding spaces, patterns, and static information cooperate in an extensible language. | Study how domain notation can retain grouping and editor structure. | Jet's ratified language does not acquire a macro system through this comparison. Extensibility requires a parse, scope, semantic, tooling, and diagnostics contract. The paper's case studies are not general productivity measurements. |

### Optimization and implementation reuse

| Primary work | Useful mechanism | Transfer conditions and assessment |
|---|---|---|
| [egglog](https://doi.org/10.1145/3591239) and [DialEgg](https://www.azizzayed.com/publications/dialegg/zayedcgo24.pdf) | Equality saturation combines equivalent representations; Datalog-style reasoning supplies facts. | Keep equivalence soundness separate from extraction cost. Effectful operations, failure order, and floating-point laws need exact modeling. Prototype outside production under #2925; no dependency approval is implied. |
| [weval](https://doi.org/10.1145/3729259), [Deegen](https://arxiv.org/pdf/2411.11469), [semPy](https://doi.org/10.1145/3623476.3623529) | Reuse executable semantics through partial evaluation or generated execution engines. | Strong relevance to Jet's already-ratified one-lowering direction. None proves that Jet's complete FFI, tasks, garbage collection, targets, or tooling can be generated without additional work. Retain Rust/LLVM quality; do not trade it away by assumption. |
| [Exo 2](https://doi.org/10.1145/3669940.3707218) | Expert schedules are built from controlled transformation operations. | A good model for separating what a computation means from how it runs. Its compile-time and solver dependencies are part of the experiment. Do not assume its kernel results transfer to all Jet code. |
| [Alive2](https://doi.org/10.1145/3453483.3454030) and [AArch64 translation validation](https://doi.org/10.1145/3763147) | Independently compare before/after behavior and expose wrong transformations. | Bounded loops, unsupported operations, timeouts, and machine modeling limit the result. A reported counterexample is valuable even when a universal proof is unavailable. |
| [MEMOIR](https://doi.org/10.1109/CGO57630.2024.10444817) | Represents collections and fields in static single assignment form to preserve mutation and eliminate unnecessary copies. | Particularly relevant to Jet's historical list-copy and data-layout losses. Requires library/compiler co-design; a paper's speedup is not a Jet prediction. |
| [Marmoset](https://doi.org/10.4230/LIPIcs.ECOOP.2024.38) | Chooses recursive-data layouts under a structured optimization model. | Useful only where layout is not externally fixed. The paper's language subset omits foreign interfaces and general I/O; it cannot justify rearranging a C ABI object. |
| [Parsimony](https://doi.org/10.1145/3579990.3580019), [Indexed Streams](https://cutfree.net/PLDI_2023_indexed_streams.pdf), [Looplets](https://arxiv.org/pdf/2209.05250) | Preserve vector or iteration structure instead of asking a late optimizer to rediscover it. | Keep sparse/dense iteration, bounds, order, and reduction semantics explicit. Correct formal stream algebra does not by itself verify every eventual backend. |
| [Stateful incremental compilation](https://doi.org/10.1109/CGO57630.2024.10444865), [Static Basic Block Versioning](https://doi.org/10.4230/LIPIcs.ECOOP.2024.28), [CacheIR](https://bernsteinbear.com/assets/img/cacheir.pdf) | Reuse prior work, specialize on known types, and make inline-cache operations structured. | Benefit depends on invalidation, code growth, workload, and compilation latency. Use Jet's existing identity and decision records. Do not introduce another cache truth source. |

### Research ranking

The first experiment should combine explicit semantics, a small checked transformation, and a real counterexample. It answers whether the proposed trust architecture is usable. Next, examine temporal resource facts and information-preserving collection lowering. Richer user-visible effects or type syntax should wait until a named real program cannot express the required contract using current law.

This ranking is based on fit to observed Jet failure shapes, not paper prestige or claimed speedup. Reproducibility is still owed: pin versions, inspect artifacts, replay proofs, run baselines, and report unsupported cases. No dependency is selected in this investigation.

<a id="q7"></a>
## Q7. A breakthrough is possible; none is established here

**Direct answer.** This work has not created or proved a new computer-science result. It identifies a coherent research program and falsifiable hypotheses. Several ingredients have strong prior art. A novel combination is not itself a theorem, an algorithmic advance, or a useful product improvement.

### Hypothesis H1: one semantic derivation can serve checking, explanation, and optimization

**Claim to test.** A shared derivation over Jet's canonical facts can produce both a valid optimization justification and a faithful explanation without an independent semantic rule in each consumer.

**Prior art.** Proof-carrying compilation, executable semantics, egglog, modal effects, and Jet's own fact/ledger decisions all cover parts of this territory. The proposed contribution would be a precise derivation format and a demonstrated preservation/comprehension relation for a broad real language. Novelty requires a deeper literature review before publication.

**Experiment.** Use the same source slice for a bounds check, a mutation, a fallible operation, and a pure map. Derive the lowered operation and a machine-checkable justification. Project a short explanation. Change the source revision and require old results to become stale. Compare the explanation with actual execution and the theorem with deliberately invalid transformations.

**Disproof.** An explanation needs a second semantic interpreter; a valid derivation licenses a wrong transform; a source edit leaves a current-looking old fact; or the derivation cannot represent a required observation. A latency regression is a product problem even if the theorem succeeds.

### Hypothesis H2: temporal facts can rule out a broader class of lifecycle mistakes

**Claim to test.** The existing fact model can express resource identity, generation, and transition order well enough to reject stale-use and use-after-close mistakes without new mandatory beginner annotations.

**Prior art.** Typestate, generation handles, ordered borrowing, session types, and revisioned incremental systems already solve pieces. There is no novelty claim for adding a generation counter.

**Experiment.** Express the same “read, change, publish” and “allocate, release, reuse, write” histories in the current fact model. Include callbacks and deferred work. Measure false rejections and the amount of expert annotation needed. Challenge alias joins, failure exits, and cancellation.

**Disproof.** A stale capability is accepted; ordinary safe reuse becomes inexpressible; callbacks erase the relation; or the implementation must maintain a separate lifecycle law for each subsystem.

### Hypothesis H3: verified scheduling can preserve both meaning and expert performance

**Claim to test.** Semantic facts can license fusion, layout, and parallel scheduling while an independent cost decision chooses the profitable version. One canonical meaning can retain expert control without making the common path slower.

**Prior art.** Exo, MEMOIR, Marmoset, Indexed Streams, and conventional guarded optimization. Jet already has ratified acceleration work. This is a proposed application and synthesis, not an original scheduling theory yet.

**Experiment.** Start with a map/reduce and a mutable collection kernel. Separate legality, profitability, and the fallback. Compare scalar, vector, parallel, and layout alternatives on identical data with identical output rules. Include short inputs, bandwidth-bound work, expensive callbacks, NaNs, overflow, and observable effects.

**Disproof.** The optimized program changes failures or results; the “faster” path wins only after changing input or output rules; an expert rejection uses a different semantic implementation; or dispatch costs erase the benefit. The current campaign runs no Jet benchmark, so these remain obligations under existing performance cards.

### Hypothesis H4: counterexample-led teaching transfers beyond Jet syntax

**Claim to test.** A learner who predicts, observes, and repairs a behavior contrast learns a transferable programming principle better than a learner who only copies a working example.

**Prior art.** Prediction-versus-production research, SMoL misconceptions, rules of program behavior, and contextual visualization. This is not an unstudied teaching idea.

**Experiment.** Give both groups the same task, time, and tool access. Test immediate behavior prediction and a later structurally different problem. Include correct explanations, wrong-but-plausible models, abandonment, and accessibility. Human participants, not simulated readers, establish learning effects.

**Disproof.** Gains disappear on transfer, are explained by extra time, or require a visual feature unavailable in ordinary editors. The source-based RLI5 assessment in this bundle does not answer that empirical question. [#2926](index.md#card-2926) owns it.

### Executed model witnesses

Main executed a retained native-JavaScript demonstrator, not Jet. It found a stale attachment under `read → write → publish`, and a stale entity-slot write accepted without a generation check but rejected with one. Binary64 reassociation produced `1` for `(1e16 + -1e16) + 1` and `0` for `1e16 + (-1e16 + 1)`.

The first clock model was deliberately retained after it contradicted the intended equal-time premise. Nominal one-second binary64 schedules produced 60 fixed steps at 30 and 60 Hz, but 59 at 144 Hz. Their accumulated wall times were not exactly equal. This is evidence that a clock representation is part of the contract, not proof that fixed-step simulation is inherently frame-dependent. The corrected exact-tick comparison and its complete provenance are recorded in [the model results](05-correctness.md#model-results).

These witnesses validate the need for the named premises. They do not prove Jet has the defects, prove a proposed type system sound, or establish learning effectiveness.

<a id="q8"></a>
## Q8. Win by composing advances with fundamentals, then measure each promised domain

**Direct answer.** Jet can pursue new advances and fundamentals together when the research preserves information that both correctness and performance need. One semantic lowering can reduce divergent behavior and repeated implementation. Precise effects can justify optimization and explanation. A first-party command loop can reduce setup while producing auditable evidence. These are credible mechanisms, not current superiority claims.

The eight-area mission is recorded in [agent memory's owner ruling](../../spec/contributing/agent-engineering.md). AI/ML applications here means users' numerical and model workloads. It does not permit an AI dependency, assistant, or optional AI mode in the proposed Jet tooling.

| Mission area | Strong comparison target | Fundamental workload | Possible Jet advance | Required matched evidence |
|---|---|---|---|---|
| Web | TypeScript/JavaScript and mature browser/server frameworks | Render, edit, validate, handle failures, build and deploy | Same typed shape and diagnostic facts across server/client tooling | Same UI behavior, browser constraints, payload, startup, interaction latency, and recovery; no substituted mock backend |
| Games | C++ engines, Unity/C#, Rust/Bevy, Godot | Input, assets, fixed simulation, rendering, lifecycle, reload | Generation-safe entities, typed time/space, replayable transitions | Same scene and assets; frame distributions, memory, determinism boundary, failure scenarios; not a toy numeric kernel |
| CLI and scripts | Python, Go, Rust, shell tools where appropriate | Install, parse arguments, stream input, transform files, report errors | Coherent `new/run/check/test/fix/explain`, direct I/O, structured verdicts | First-result latency, streaming and early-exit behavior, clean/offline setup, Unicode and path behavior |
| Data analysis | Python/pandas, R, Julia, SQL engines | Ingest, clean, aggregate, join, visualize, export | Shared table/shape facts and information-preserving collection lowering | Identical missing-value, grouping, ordering, numeric and memory semantics; comparable library quality |
| Backend services | Go, Rust, Java, Erlang/Elixir and domain frameworks | Requests, persistence, tasks, shutdown, recovery | Structured failure and lifecycle with one inspectable runtime model | Same load and correctness contract; latency distribution, sustained throughput, resource bounds, restart behavior |
| AI/ML applications | Python with native numerical libraries, C++, Julia | Tensor preparation, inference integration, training-loop plumbing | Efficient typed data movement and explicit foreign accelerator boundaries | Same model, data, device, precision, kernels and transfer costs; no benchmark credit for changing accuracy |
| GUI applications | C#/desktop frameworks, Swift, Kotlin, Flutter and native toolkits | State, events, layout, accessibility, packaging | Typed state transitions and source-linked inspection | Same interactions, keyboard and accessibility behavior, startup, rendering, memory, platform integration |
| Embedded | C, C++, Rust, Zig, Ada | Cross-build, interrupts, volatile/device I/O, bounded resources | Explicit layout/time/effects and auditable unsafe boundaries | Real target or justified simulator, ABI, footprint, worst-case constraints, device semantics, and failure handling |

### The score cannot average away a loss

The standing Jet performance rule requires every matched cell to win against non-Rust comparators and permits Rust parity only within the ratified noise allowance. This report does not alter that rule. Failed, timed-out, unsupported, or behavior-mismatched runs cannot become a speed ratio. A workload win cannot cover a different workload's loss.

Historical Tower records already identify losses in tensors, HTTP, loops, parsing, copies, parallel crossover, and missing execution paths. Some implementation cards are now marked done; their current evidence still belongs to the deferred validation campaign. [#2858](index.md#card-2858), #2859, and [#2919](index.md#card-2919) own the integrated measurements. This investigation supplies no new Jet scoreboard.

For development experience, retain five independent quantities: verdict fidelity, latency, actionability, context economy, and repair determinism. Measure the full edit-to-answer loop on the same tasks. Count necessary setup, repeated annotations, external concepts, and failure recovery alongside runtime. Do not use fewer characters as the sole ergonomic result.

## Promotion gates for research

1. State the claimed property and strongest relevant prior art.
2. Define the semantic model, assumptions, and exact counterexample that would disprove the claim.
3. Build a retained non-production artifact and reproduce its result independently.
4. Compare against the strongest matched implementation, including failures and cost.
5. Show that beginners retain the obvious path, experts retain control, and an enterprise reviewer can inspect the claim.
6. Raise a concrete owner ballot before a new public interface, dependency, or major architecture change.
7. Integrate through the existing semantic and evidence mechanisms. A successful isolated prototype is not a release qualification.

The compiler-proof commitment is proposed as [D-COMPILER-PROOF1](index.md#decisions). It remains an owner choice, not a retroactive claim that Jet is verified.

## Generalize every finding

| Finding | Defect shape | Predicted other instances | Structural correction | Action disposition |
|---|---|---|---|---|
| A type/effect result loses its premises when transferred | Proof scope silently widened | Handler multiplicity, callback escape, alias growth, cancellation | Explicit calculus-to-Jet relation and negative examples | New [#2925](index.md#card-2925) |
| Optimizer legality is confused with profitability | One decision answers two different questions | Fusion, vectorization, parallelism, layout, specialization | Shared semantic facts plus separate measured cost selection | Reuse #2895, #2899, #2922; protected D-ACCEL1 remains unchanged |
| Nominal equal-time schedules are not exactly equal | Representation omitted from the experimental premise | Timers, simulation, duration conversion, event replay | Exact input identity and representation-aware oracle | Retained model witness; reuse #822 and #825 for Jet game proof |
| A source or paper count appears to prove novelty | Evidence volume substituted for a comparative result | New type systems, teaching tools, proof pipelines | Prior-art comparison plus explicit disproof and reproduction | Research only; no novelty or superiority claim |
| A kernel result stands in for domain readiness | Workload substitution | Web, games, ML, services, GUI, embedded | Matched complete programs in every mission area | Reuse #2858 and #2919 |
