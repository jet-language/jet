# Ordinary correctness: remove invalid states, preserve time and identity

[Master report](index.md) · [Optimization assurance](04-trust.md#q21) · [Mixed-language boundaries](06-interop.md)

Jet can improve ordinary software quality by excluding classes of mistakes from safe programs, preserving information at boundaries, and making domain rules explicit. It cannot infer an unstated requirement. A game can be memory safe and still have bad collision logic. A service can be race-free and still lose an update at the wrong protocol boundary.

This report answers questions 26–29. It uses documented failure mechanisms, current Jet law/source inventory, historical Jet findings, and a retained JavaScript model experiment. It contains no claim that a current Jet game, service, numerical kernel, or embedded program has been qualified.

<a id="q26"></a>
## Q26. Adoption can reduce bug classes when the guarantee covers the actual cause

**Direct answer.** Make invalid memory, type, ownership, and explicitly modeled state transitions unrepresentable or rejected on the safe path. Preserve units, identity, bounds, failure, and lifecycle facts through libraries and compilation. Detect the remaining mistakes with contracts and observable evidence. Do not market every ordinary glitch as a type-system problem.

| Defect family | Cause | Language/library response | Exact benefit and remaining limit |
|---|---|---|---|
| Dangling references, double release, invalid alias mutation | Resource lifetime or ownership violated | Ratified value/borrow rules, checked ownership, explicit unsafe boundary | Excludable from the safe model if checker, compiler, runtime, and foreign contracts uphold it. Current Jet implementation proof is still owed. |
| Out-of-bounds access and invalid conversion | Missing or discarded size/range information | Bounds, range/refinement facts, checked conversions, typed failure | Reject or report the operation rather than corrupt state. A correct bound does not prove the index means the intended business object. |
| Absent values and uninitialized state | Representation permits a missing case without handling | Optional values, exhaustive branches, complete constructors | Exclude unchecked absence where the contract requires handling. A present but wrong value remains possible. |
| Contradictory flags and illegal lifecycle transitions | A product of independent booleans represents impossible combinations | Sum/state types and transition contracts | Prevent the encoded invalid combinations. A badly chosen state model can still omit a legitimate transition. |
| Stale handle after object reuse | Physical slot or name is mistaken for persistent identity | Generation-tagged handles or a statically bounded lifetime | Reject old identity after reuse. Generation overflow and foreign aliases need an explicit policy. |
| Units, coordinate spaces, and time domains mixed | Same numeric representation hides different meanings | Existing dimension/measure facts; distinct domain types; checked conversion | Catch incompatible quantities. The correct unit attached to a wrong number still needs domain validation. |
| Precision loss, NaNs, overflow, invalid numeric assumptions | Mathematical laws differ from machine arithmetic | Exact/approximate distinctions, defined numeric rules, constrained optimization | Preserve the promised semantics and reject illegal assumptions. Approximation remains an explicit numerical choice. |
| Data races and one-shot resource reuse | Concurrent access or repeated consumption violates ownership | Structured tasks, exclusive mutation, affine/one-shot contracts | Exclude modeled interference or repeated use. Deadlock and incorrect ordering may remain. |
| Leaked tasks, callbacks after shutdown, missed cleanup | Work outlives its owner or exit bypasses disposal | Task ownership, cancellation and cleanup contracts, lifecycle identity | Contain or reject specified lifetime violations. External operations already performed cannot always be undone. |
| Lost updates, duplicate events, stale reads | Protocol lacks an order/identity/consistency rule | Typed state machines, versions, transaction boundaries, idempotent operations where meaningful | Makes the protocol testable. A local type cannot guarantee a remote system honored its contract. |
| Resource exhaustion, long pauses, blocked UI | Unbounded allocation, queue growth, synchronous I/O, or missed deadline | Bounded structures, explicit resource policies, async/task boundaries, inspectable cost | Bound or expose the cause. Work cannot meet a deadline when required computation exceeds available capacity. |
| Wrong business rule, wrong asset, bad algorithm | Intent/specification is wrong or absent | Domain contracts, examples, property oracles, review | Detect when an explicit rule is violated. No universal language rule can recover an unstated requirement. |

The causal claim must be conditional. If developers actually use the safe path, the modeled guarantee is sound, and foreign boundaries honor their contracts, those excluded defects no longer depend on each developer remembering a rule. That is the “rising tide” mechanism. A measured reduction in overall defect rate across deployed Jet projects has not been established here.

### Real incidents show the limit of local safety

The [Mars Climate Orbiter investigation](https://llis.nasa.gov/llis_lib/pdf/1009464main1_0641-mr.pdf) connects a unit mismatch with organizational and interface failures. Typed units could reject the specific incompatible exchange if both sides preserve the unit contract. They would not, alone, fix the review and operational process.

The [Ariane 501 inquiry](https://esamultimedia.esa.int/docs/esa-x-1819eng.pdf) describes conversion and exception behavior in reused software under changed flight assumptions. A range proof must include the new input domain. “This code was reliable elsewhere” is not a valid premise for a different environment.

The ordinary reliability portions of the [GitLab database postmortem](https://about.gitlab.com/blog/postmortem-of-database-outage-of-january-31/) and [GitHub incident analysis](https://github.blog/news-insights/company-news/oct21-post-incident-analysis/) show that backups, recovery, replication, and delayed events require operational tests and clear ownership. A successful local call or a backup file's existence does not prove restoration works. No cyber material from those sources is analyzed here.

<a id="q27"></a>
## Q27. A Jet game should lose memory hazards, not magically acquire correct physics

**Direct answer.** Compared with an otherwise equivalent C++ implementation that relies on unchecked pointers and manual lifetime discipline, a correctly implemented safe Jet game should exclude those memory and ownership failures. Other glitches become less likely only when Jet's game APIs encode time, identity, state, and lifecycle contracts. Physics accuracy, level design, performance, and player intent still require domain evidence.

Modern C++ engines already offer handles, smart pointers, lifecycle hooks, state machines, and checked tools. The fair comparison is the same scene and contract using the strongest practical C++ path, not deliberately careless C++. Jet's opportunity is to make the safe, coherent path the ordinary one and preserve it across tools and execution modes.

| Same game scenario | Strong C++/engine practice | Required Jet behavior | What becomes impossible or less likely | What remains |
|---|---|---|---|---|
| An entity is destroyed, then its slot is reused | Generation handles or engine-owned weak object references | Ordinary handles validate identity or cannot outlive the owner | Old handle mutating the replacement is rejected under that contract | Wrong entity selected with a valid current handle; generation wrap; foreign engine violations |
| A callback fires after the object closes | Object-bound timers, cancellation, lifetime-aware callbacks | Callback ownership and cancellation follow the entity/task lifetime | Modeled use-after-close or duplicate one-shot use is excluded | The callback can still perform the wrong valid action |
| Movement depends on rendered frames | Fixed simulation step, accumulator, render interpolation | Typed simulation time separated from rendering time | Accidental per-frame speed dependence is reduced or rejected by the API | Clock quantization, overload policy, numeric nondeterminism, wrong integration method |
| A fast projectile passes through a wall | Continuous collision detection or smaller steps | Explicit collision contract and units; ordinary safe defaults where appropriate | Unit/space mismatch can be rejected; invalid API use can be detected | Discrete collision inherently misses events between samples without stronger modeling |
| A character is both jumping and crouching in an illegal combination | State machine instead of unrelated booleans | A domain state type permits only the intended states and transitions | The encoded contradictory combination becomes unrepresentable | An incorrect state graph or animation binding |
| Removing entities during iteration skips another update | Deferred commands or stable iteration contract | Mutation/iteration contract forbids invalidating the traversal | Iterator invalidation and the specified skipped-update class are excluded | A valid but unintended update order |
| Two systems observe half-updated state | Double buffering or explicit dependency scheduling | Snapshot/phase boundaries are visible in the state model | Unspecified intermediate observations can be excluded | Extra memory, intentional one-step latency, or an incorrect phase graph |
| Audio loads synchronously and freezes a frame | Preload, streaming, bounded queues, worker ownership | Blocking/resource effects are visible; game loop avoids forbidden work | Some accidental blocking can be rejected or exposed before release | Insufficient I/O capacity or an asset that is too large |
| A pool retains old fields or releases twice | Full reset, debug checks, typed pool ownership | Complete construction and ownership-aware release | Duplicate safe release and uninitialized fields can be excluded | Incorrect but fully initialized reused state; inadequate capacity policy |
| Parallel physics changes the answer | Deterministic schedules/reduction rules or explicit tolerance | Same defined order/precision on every applicable mode | Compiler-induced meaning changes are forbidden | Physical platform variation outside the contract and inherently chaotic sensitivity |

### Primary game evidence

[Game Programming Patterns](https://gameprogrammingpatterns.com/) describes specific mechanisms: [update order and collection mutation](https://gameprogrammingpatterns.com/update-method.html), [pool reuse and capacity](https://gameprogrammingpatterns.com/object-pool.html), [delayed events and stale state](https://gameprogrammingpatterns.com/event-queue.html), [double buffering](https://gameprogrammingpatterns.com/double-buffer.html), and [state machines](https://gameprogrammingpatterns.com/state.html). These are causal examples, not measured defect rates for shipped games.

[Fiedler's fixed-timestep explanation](https://gafferongames.com/post/fix_your_timestep/) separates simulation steps, accumulated elapsed time, and render interpolation. [Unity's timing manual](https://docs.unity3d.com/2022.3/Documentation/Manual/TimeFrameManagement.html) documents fixed updates, catch-up, and maximum-step behavior. Its [collision documentation](https://docs.unity3d.com/2022.3/Documentation/Manual/physics-optimization-cpu-rigidbody-collision-modes.html) explains discrete tunneling and the cost of stronger detection. These are versioned engine contracts, not universal game-engine laws.

Unity's [Awaitable documentation](https://docs.unity3d.com/6000.1/Documentation/Manual/async-awaitable-introduction.html) gives a particularly useful one-shot example: pooled awaitables must not be awaited repeatedly. [Unreal timers](https://dev.epicgames.com/documentation/unreal-engine/gameplay-timers-in-unreal-engine?application_version=5.8) provide object-bound cancellation, while its [actor lifecycle](https://dev.epicgames.com/documentation/unreal-engine/unreal-engine-actor-lifecycle?lang=en-US) distinguishes ending play from final deallocation. Jet should expose those distinctions instead of using one vague “alive” bit.

### Jet disposition

The source inventory contains `core.game` names `Backend`, `Replay`, `Scene`, and `run`, plus the raylib package and input/drawing/audio functions. Registration is not game readiness. Existing [#822](index.md#card-822) owns input, fixed-step clock, replay, and rollback; [#825](index.md#card-825) owns the complete playable slice and failure proof. This report adds concrete scenarios to their interpretation without claiming their current execution passed.

<a id="model-results"></a>
## Executed model results: four small premises that matter

The retained [JavaScript source](../../../tools/agent-eval/foundations-trust/research-demonstrators.mjs), [full result](demonstrator-results.json), and [execution receipt](demonstrator-receipt.json) make the experiments reproducible. The program is a research model. It does not invoke Jet, a real game engine, a real clock, a concurrent scheduler, or a foreign allocator.

| Case | Inputs and relation | Observed result | What it establishes | What it does not |
|---|---|---|---|---|
| Revision-bound publication | Enumerate all six permutations of read, write, publish; a legal publication requires a prior read | `read → write → publish` attaches revision 1 while state is revision 2; the model flags it stale | Legal event order alone permits an old fact to attach to new state | A rejecting revision guard, a particular Jet publisher defect, or a complete concurrency model |
| Binary64 reassociation | `(1e16 + -1e16) + 1` versus `1e16 + (-1e16 + 1)` | `1` versus `0` | Ordinary real-number associativity is not a valid unrestricted binary64 rewrite | Jet's current floating-point mode or optimizer policy |
| Entity slot reuse | Allocate a slot, retain its handle, release, reuse, write through the old handle | Slot-only resolution accepts the stale write; slot-plus-generation rejects it | Physical storage location is not stable logical identity | A full allocator, concurrent lifetime proof, or generation-overflow policy |
| Nominal one-second binary64 clock | 30/60/144 nominal frame schedules, fixed step `1/60`, no dropped time | 60, 60, and 59 steps; accumulated elapsed sums are not exactly equal | The representation premise matters even in a small accumulator | Exact-equal-time invariance, real timer quality, or production overload behavior |
| Exact integer-tick clock | 720 ticks/second; 12-tick simulation step; frame increments 24/12/5 ticks | 60 simulation steps, 720 elapsed and simulated ticks, zero remainder at every rate | These exact equal-time partitions produce the same count in the bounded model | Jitter, nonintegral rates, clock drift, overflow, collision accuracy, interpolation, or production scheduling |

Publish-before-read orders remain in the enumeration but are invalid publications with no attachment. They are not stale counterexamples. In the legal stale witness, publication is marked valid because its read occurred first; that does not mean its revision is current. The proposed correction is a rejecting revision guard. No guarded-publication variant was executed.

The binary64 result was retained rather than replaced by a “passing” exact-tick demonstration. The correction adds a different, explicitly stated representation and establishes a different bounded relation.

This is the right experimental habit for Jet: keep the counterexample, repair the premise, and state precisely what the new result establishes.

<a id="q28"></a>
## Q28. Push the trade space by separating decisions that need not be coupled

**Direct answer.** Several apparent tradeoffs disappear when semantics, representation, scheduling, evidence, and presentation are separated. Others are real only under a particular resource or physical constraint. Implementation effort is not a negative in this analysis. The remaining losses must be tied to a concrete premise.

| Apparent tradeoff | Design-away move | What stays jointly achievable | Irreducible boundary or open measurement |
|---|---|---|---|
| Safety versus speed | Prove bounds/ownership, then erase redundant checks; keep checked fallback when proof is unavailable | Same safe meaning with optimized common paths | A necessary dynamic check has cost unless the premise can be established earlier. Measure it rather than promise zero. |
| Beginner simplicity versus expert control | One default semantic path, contextual explanation, explicit expert constraint/override | Short common code and auditable control | An expert must supply information the system cannot infer. The burden belongs only where that information changes the outcome. |
| Fast edit loop versus strong checks | Incremental dependency reasoning, reusable proof facts, precise invalidation | Strong checks without recomputing unrelated work | A changed dependency requires new evidence. Reusing stale evidence is not an optimization. |
| Determinism versus parallel performance | Defined partitioning/reduction order; separate rendering from simulation; deterministic scheduling where promised | Parallel work with specified observable results | Some reorderings are not semantics-preserving. A different numerical contract needs an explicit choice, not a hidden mode. |
| Rich diagnostics versus context economy | Small primary explanation with linked derivation and raw evidence | Beginner clarity, expert detail, machine-readable completeness | The full proof can be large. It may be summarized, but its scope and unknowns cannot be omitted. |
| Low-level interoperation versus safe application code | Narrow typed boundary, layout/ownership/error contracts, generated repetitive adapters | Existing native libraries with a safe ordinary Jet API | Foreign code can violate its contract. A wrapper cannot prove arbitrary native behavior merely by existing. |
| Live reload versus external effects | Preserve identity and supported state; invalidate unsupported changes; separate pure replay from external actions | Fast iteration with honest state continuity | An already performed external action may be irreversible. Replay cannot fabricate an undo. |
| Fixed-step stability versus rendering smoothness | Render interpolation over previous/current simulation states | Stable simulation steps and smooth display | Interpolation adds a presentation-time relation; it does not improve the underlying physics or remove overload. |
| Bounded resources versus accepting unlimited work | Backpressure, explicit rejection, priority, or a capacity contract | Predictable resource bounds and an honest outcome | A finite resource cannot hold an unbounded backlog. One of delay, refusal, loss, or more capacity must be specified. |
| Proof strength versus release date | Mix proved algorithms and sound result checking; reuse one semantic model | Strong proof without needlessly constraining compiler implementation style | If required proof is incomplete at a date, the release cannot both meet that date and satisfy the proof gate. Owner ballot remains pending. |

A failed optimization does not automatically require a new language feature. First ask whether information was lost, a library operation hides a copy, or the cost decision uses the wrong workload. Conversely, do not blame a compiler heuristic for a semantic contract that never permitted the desired transformation.

<a id="q29"></a>
## Q29. Put prevention at the earliest layer that has the necessary information

**Direct answer.** Language shape should express meaningful distinctions. Checking should establish their consequences. Shared lowering should preserve them. Optimization should exploit them without changing meaning. Runtime libraries should enforce the dynamic remainder. Tools should explain which of those steps supplied the guarantee.

| Layer | Creative opportunity | Concrete case | Guard against overreach |
|---|---|---|---|
| Language/domain types | Replace ambiguous representation with a named distinction | Simulation time versus render time; entity identity versus index; local versus world position | Use existing facts/types where possible. A new public type needs a real use case and owner approval. |
| Static facts | Carry relations, not isolated labels | A handle refers to generation G; a callback cannot outlive owner O; a bound holds until mutation M | Invalidation and joins are part of the fact's semantics. A stale proof is not a proof. |
| State/transition model | Make illegal sequences rejectable | Consume once, close last, publish only for the analyzed revision | Model cancellation and failure exits, not only the happy path. |
| Shared lowering | Resolve behavior once | One lazy line pull, one copy rule, one failure propagation operation | Every adapter must implement the same observation contract. A common enum does not prove that. |
| Optimization | Remove work justified by facts | Fuse pure collection operations; eliminate proved bounds; preserve fixed reductions | Preserve failure order, effects, exactness, and identity. Never infer legality from speed. |
| Runtime/domain library | Check what static reasoning cannot know | Generation validation, bounded queue, dynamic unit/shape decode, supported target operation | Checks must report a useful outcome and remain consistent across modes. |
| Tooling | Show the causal path and repair | Explain why an entity handle is stale or a loop retained input | Show facts and observations with identity; do not run an invented semantic interpreter. |
| Assurance | Turn one example into a shape census | Enumerate every single-use API, deferred callback, stale-result publisher, or numeric rewrite | Reuse the canonical denominator and existing owners. Do not make a separate registry per report. |

The strongest design is not “add a special rule for game entities.” It is to identify temporal identity and operation order as reusable relations, then apply them to entities, editor results, callbacks, handles, and replay. The mechanism is shared; each domain supplies its own lifecycle meaning.

### Concrete acceptance scenarios

- A source edit occurs after analysis starts but before diagnostics publish. No old fact is presented as current.
- A handle survives release and slot reuse. A safe operation cannot mutate the replacement object through that handle.
- A one-shot asynchronous value is consumed twice. The system rejects the second consumption or reports a defined failure before corrupting state.
- A streaming loop exits early. Every applicable mode leaves the same unread input and produces the first observation at the same semantic point.
- A reduction runs with different scheduling. Results follow the same defined numerical contract, not a convenient real-number identity.
- A fixed-time simulation is rendered at different frame schedules. The test fixes elapsed input, clock representation, overload policy, and numerical contract before comparing trajectories.
- A foreign callback arrives after shutdown. The boundary contract determines cancellation, rejection, or valid remaining ownership explicitly.

These are proposed proof obligations attached to existing mechanisms. The model experiment exercises only the first two simplified identity cases and the numeric/clock premises. It is not a current Jet conformance run.

## Generalize every finding

| Finding | Defect shape | Predicted other instances | Structural correction | Card disposition |
|---|---|---|---|---|
| Stale identity acts on new state | Identity reused without lifetime/generation relation | Entities, debugger handles, callbacks, resident results | Canonical temporal fact and validated publication/use | #822/#825/#2506; research #2925 and learning #2926 |
| Mathematical rewrite changes machine result | Numeric law lacks its domain premise | Constant folds, reductions, division, conversion, vectorization | Defined numeric relation and checked transform guards | Reuse #2891/#2895/#2899; no protected ballot changes |
| Equal final output hides different interaction | Oracle ignores time/order/consumption | Streams, tasks, event queues, cleanup | Trace-level behavioral oracle | Reuse #2898/#2923/#2919 |
| Domain type exists but its real rule is absent | Representation mistaken for specification | Time, units, coordinates, money, states | Domain contracts and matched negative examples | Existing domain owners; #2926 census |
| Apparent performance gain changes the workload | Cost comparison weakens semantics | Fast math, dropped frames, truncated input, skipped validation | Same-program comparator and explicit expert contract | Reuse #2858/#2859/#2922 |
| Local safety is advertised as global correctness | Guarantee widened past information boundary | Physics, business logic, external recovery | Claim-specific proof and external assumptions | New #2925/#2927; reuse #2339 |

No production fixes or domain API additions are made by this report. The difference between an intended guarantee and a current implementation result remains visible throughout.
