# Pursue a research result, not a research-shaped slogan

[Executive report](index.md) · [Application walkthroughs](04-domain-walkthroughs.md) · [Decision slate](06-decisions-and-finding-dispositions.md)

## The strongest research question is a precise composition

**Can one checked query produce a batch answer, a maintained answer, and an explanation of each change, while preserving Jet's value, error, order, ownership, and publication rules?**

Each ingredient has prior art. Typed queries, incremental view maintenance, effect systems, provenance, and proof-carrying transformations are not new discoveries. The possible research contribution is a useful, verified composition that retains the observations existing systems often separate. Novelty remains unestablished until a literature and implementation comparison survives independent review.

The practical product does not wait for a novelty claim. A type-preserving query, a controlled timeout test, or a checked geometry conversion is valuable if it removes real work and prevents a named mistake.

## What the primary sources actually contribute

| Source | Take | Do not copy or claim | Concrete Jet consequence |
|---|---|---|---|
| [Dijkstra, The Humble Programmer](https://www.cs.utexas.edu/~EWD/transcriptions/EWD03xx/EWD340.html) | Structure a program so reasoning fits human limits | Treat a historical argument as a benchmark or copy a syntax fashion | Reduce independent descriptions and make local obligations visible |
| [Iverson, Notation as a Tool of Thought](https://www.jsoftware.com/papers/tot1.htm) | Executable notation can reveal relationships and support composition | Assume terse glyphs are automatically teachable | Query/shape/space operations should make the relationship visible in ordinary code |
| [Codd, relational model](https://research.ibm.com/publications/a-relational-model-of-data-for-large-shared-data-banks) | Separate logical data meaning from internal representation | Hide every physical cost or treat all state as relational | One typed query over lists, files, and columnar buffers, with inspectable plans |
| [Go at Google](https://go.dev/talks/2012/splash.article) | Attack slow builds, uncontrolled dependencies, language subsets, and tooling friction together | Copy Go's feature set as Jet's ceiling | One-command project execution, complete checked summaries, a small ordinary path |
| [Go testing time](https://go.dev/blog/testing-time) | Fake time plus a stable-background-work boundary removes sleep-based guesses | Assume fake time alone makes external devices deterministic | F05 scoped world and explicit provider coverage |
| [Lamport, Turing lecture](https://lamport.azurewebsites.net/pubs/turing.pdf) | Describe behavior and state transitions precisely before implementation | Confuse a model with the running system or an unstated business requirement | F10 independent state models and hostile histories |
| [DBSP](https://link.springer.com/article/10.1007/s00778-025-00922-y) | A systematic algebra for incremental computation and view maintenance | Claim Jet already implements it, or transfer SQL assumptions unchanged | F03 derivation rules with Jet error/order/effect premises |
| [Differential Dataflow](https://timelydataflow.github.io/differential-dataflow/introduction.html) | Work over changing collections and retained arrangements | Make every application adopt a distributed dataflow runtime | Use its maintained-state lessons inside the bounded query job |
| [Adapton / demand-driven incremental computation](https://www.cs.tufts.edu/~jfoster/papers/cs-tr-5027.pdf) | Reuse work according to dependencies and demand | Treat cache invalidation and publication as the same problem | Separate computation validity from whether a result may become current |
| [Koka effect types](https://arxiv.org/abs/1406.2061) | Track effects in callable types and reason about effectful operations | Introduce a second effect system beside Jet's ratified one | Reuse Jet effects for controlled worlds and legal query rewrites |
| [Euclid](https://docs.rs/euclid/latest/euclid/) | Typed coordinate spaces prevent numeric values from being mixed incorrectly | Promise all dynamic scene identities erase for free | F04 nominal spaces plus runtime identity only where necessary |
| [Arrow C data interface](https://arrow.apache.org/docs/format/CDataInterface.html) | A small stable same-process interface with explicit release ownership | Confuse it with a Parquet reader, IPC transport, or proof of foreign memory behavior | F07 one checked typed buffer boundary |
| [Polars lazy API](https://docs.pola.rs/user-guide/concepts/lazy-api/) | Retain a query until collection and explain its plan | Assume projection/predicate pushdown preserves Jet's validation contract automatically | F02 common plan with explicit legal-rewrite premises |
| [Hypothesis stateful testing](https://hypothesis.readthedocs.io/en/latest/stateful.html) | Generate action sequences and reusable values; shrink to a small counterexample | Derive the oracle from the implementation being tested | F10 typed histories with independent meaning |
| [CompCert](https://compcert.org/man/manual001.html) | Compose machine-checked preservation arguments over compiler stages | Transfer C undefined-behavior latitude into Jet's defined errors | Implement the adopted complete Jet proof requirement with exact boundaries |
| [Alive2](https://github.com/AliveToolkit/alive2) | Validate transformations against a formal intermediate-language model | Treat bounded checks, unsupported transformations, or timeouts as universal proof | Use validation as research evidence unless it meets the adopted sound-checker contract |

These sources support mechanisms and limits. No download count or star count here is presented as evidence that one API is popular. No paper's speedup is reported as Jet's speedup.

## A candidate theorem for watched queries

Let `Q` be a checked query, `S` an input state, and `e` a valid input edit. Let `apply(S,e)` be the new input state. Let `batch(Q,S)` be the query's reference execution. Let `maintain(Q,M,e)` update the retained state `M`.

The desired relation is:

```text
observe(maintain(Q, retained(Q,S), e))
    = observe(batch(Q, apply(S,e)))
```

That equation is meaningful only after defining `observe`. For Jet it includes the result values and types, specified order, errors and their timing/order where specified, externally visible effects, and lifetime/publication obligations. It is not just a checksum of successful output rows.

For a sequence of edits, the proof must compose. Publication must then ensure the result becomes current only for the source revision used. A correct calculation published into the wrong document is still wrong behavior.

### The theorem's required premises

1. The source edit satisfies the changing-source contract, including unique keys and transaction boundaries.
2. Every transformed operation has a checked rule for its types, numerical mode, effects, and order.
3. Retained state represents the relevant source state and obeys its memory/lifetime limits.
4. Unsupported pure operations use the defined recomputation path; effectful operations are not silently replayed.
5. Errors and exhaustion have the same permitted observations as the reference contract.
6. Publication checks the same owner/revision identity used by all live consumers.

If a premise fails, the implementation either uses a valid reference path or reports a defined inability. It does not label an unchecked result “incremental and equivalent.”

## An experiment that actually ran

The [standalone model](incremental-query-experiment.mjs) was executed with Node through the repository environment. Its [result artifact](incremental-query-results.json) binds the script digest and runtime version.

```sh
scripts/agent/jet-env node \
  docs/audits/jet-refounding-2026-09-05/incremental-query-experiment.mjs \
  docs/audits/jet-refounding-2026-09-05/incremental-query-results.json
```

| Executed case | Observed result |
|---|---|
| Every bounded table over three IDs, two regions, and amounts -1/0/1 | 343 states enumerated |
| Every valid single insert, replace, or remove from those states | 7,056 transitions agreed between exact recomputation and maintained totals |
| Deliberately retain a group after its final row is removed | Rejected; expected no group, observed a zero-valued empty group |
| Deliberately omit removal from the old group when a row changes region | Rejected; observed a stale old-region contribution |
| Apply the same subtract-old/add-new idea to binary64 Float | Refuted by a concrete rounding counterexample |

The Float counterexample is small:

```text
Before: [10^16, 1, -10^16]       left-to-right sum = 0
Replace the middle 1 with 2
Recompute: [10^16, 2, -10^16]    left-to-right sum = 2
Maintain: old sum - 1 + 2       result = 1
```

**Conclusion:** exact-integer update algebra passed this bounded model. The same rewrite is not justified for ordinary left-to-right floating-point addition. The experiment therefore changed the proposal: numerical-mode and operation-law premises are mandatory, not optional optimizer metadata.

**Limits:** no Jet parser, checker, MIR, backend, runtime, join, failure stream, concurrency, or publication system ran. Group sorting normalizes comparison; it does not select Jet's public ordering rule. This is neither a performance result nor an unbounded proof.

## Research directions with an actual falsifier

| Hypothesis | Concrete experiment | What would refute the promised benefit |
|---|---|---|
| H1 · Typed incremental queries remove a second application algorithm | Same sales/leaderboard program under batch and edit histories; compare result/error/order and user-authored maintenance code | Required callbacks still need a second handwritten delta algorithm, or a valid history differs |
| H2 · Checked access information can derive useful resource schedules | Same game frame with source-order reference and derived schedule; inspect hazards, transfers, lifetimes, and measured frame/memory cost | Dependency declarations remain duplicated, a schedule changes meaning, or retained scheduling cost dominates the matched job |
| H3 · Scoped effects make asynchronous tests both simpler and more reliable | Port named timeout/cancellation tests without changing production signatures; inject scheduler and provider variations | Tests need global overrides, uncontrolled effects silently escape, or legitimate code cannot be exercised |
| H4 · A source-first explanation improves comprehension | Give newcomers the same code tasks with and without the view; score prediction, modification, and transfer using a predeclared rubric | More correct-sounding explanation produces no improvement, teaches a wrong rule, or makes the ordinary task slower |
| H5 · Typed foreign buffers remove copies without weakening safety | Import/export real Arrow producers, exercise release/alias/nullability cases, and measure actual copies and memory | A supposed shared path copies invisibly, retains invalid pointers, or loses semantic type information |
| H6 · One checked module summary improves edit/run latency | Same project, edits, diagnostics, toolchain, and cache state; count repeated front-end work and measure whole-command latency | Work is merely moved into a hidden stage, stale facts are reused, or the overall job does not improve |

A failed hypothesis is useful. Record the reason, retain the counterexample, and remove the unjustified optimization or product claim. Do not rename the experiment and report a win on an easier workload.

## Aggressive optimization needs a legal transformation before a cost model

A **cost model** predicts which equivalent implementation is cheaper. It cannot make two different meanings equivalent. The legal-rewrite question comes first.

| Candidate | Information required | Counterexample family |
|---|---|---|
| Fuse map/filter operations | Callback effects, failure order, ownership, and short-circuit rules | A callback prints, throws, or consumes a value at a different point |
| Push a projection into file decoding | Source validation obligations and supported physical layout | An invalid dropped column stops being rejected |
| Reassociate a sum | Exact arithmetic or an explicitly permitted numerical relation | The executed Float counterexample above |
| Eliminate a semantic copy | Escape, alias, lifetime, and later-write observations | A retained value changes when the original is edited |
| Reuse a temporary allocation | Non-overlapping actual lifetimes and compatible layout/device | A consumer still runs after apparent source-level last use |
| Specialize a generic call | Concrete type/effect facts and complete invalidation | A cached specialization survives a relevant contract change |
| Parallelize a reduction | Identity, associativity under the actual numeric mode, deterministic error/order contract | A merge changes a rounded answer or which failure appears first |
| Incrementalize a join | Key equality, multiplicity, update/retraction, and source revision law | Duplicate keys lose or duplicate output pairs |
| Reuse a compiled module | Complete input/toolchain/configuration/authority identity | A stale cache returns an old verdict or runs under changed rights |

Equality saturation and learned cost models can be research tools behind this boundary. They are not new public language mechanisms. A learned model may propose a cheaper already-legal choice; it never supplies the safety proof or becomes a required remote service.

The proper experiment compares compilation cost, runtime cost, memory, and artifact size for the complete job. A transformation that saves a nanosecond while multiplying edit latency can lose the workload. The performance gate evaluates each required metric rather than averaging the loss away.

## The full compiler proof remains adopted law

D-COMPILER-PROOF1=A requires every Jet-controlled stage: parsing, checking, compile-time evaluation, initial lowering, optimization, adapters, and Jet-owned runtime/Prelude behavior. D-COMPILER-PROOF-TOOLS1 selects its concrete tooling direction. This audit does not reduce that scope to an interpreter, a model, a subset, or a passing differential suite.

A **proof** establishes a statement for every case covered by its model and premises. A **test** runs selected cases. A **certificate checker** can accept a particular translation only if a soundness argument connects acceptance to the required relation. A hash identifies an artifact; it does not prove the artifact implements an algorithm.

The ratified boundary is precise. Jet's theorem reaches emitted Rust, Cranelift IR, or web output under a stated model, and covers Jet-owned interpretation. Foreign compilers, engines, operating systems, hardware, and explicit unsafe contracts remain named premises unless separately checked. That is not a physical-machine theorem for every possible environment.

### No false-green substitution

| Evidence offered | What it can count as | What it cannot replace |
|---|---|---|
| A mechanized theorem about an abstract optimizer | A model theorem | Binding the real optimizer implementation |
| Source/checker/build hashes | Identity evidence | Semantic correspondence |
| A passing bounded validator | Bounded validation evidence | A universal proof outside its bound or supported model |
| A timeout or unknown | Unresolved proof obligation | Success |
| A deterministic already-proved fallback | A valid implementation path under the adopted contract | The performance gate if it is too slow |
| Tests against all four engines | Differential evidence on those cases | The full compiler theorem |
| A proof that compilation preserves accepted programs | Preservation | Coverage and usability; a compiler that rejects everything is not acceptable |

A mismatch between the executable model and ratified Jet semantics blocks the proof claim. Correct the model or obtain a real owner decision for a semantic change. Do not redefine the language to fit the proof silently.

## Why “no bugs ever” needs a precise target

Jet can make specified invalid operations impossible within a sound model: invalid ownership use, wrong-space operations, unchecked stale publication, or a transformation that introduces forbidden behavior. It cannot infer an unwritten requirement or prove that an external device behaves as documented merely from source code.

The useful trust promise is therefore concrete:

- Every claimed language guarantee has a precise rule, implementation binding, and evidence.
- Every supported capability has a complete acceptance denominator.
- Unknown, unsupported, stale, or failed evidence cannot pass a gate.
- A discovered contradiction reopens its owning claim immediately.
- External assumptions remain visible and separately exercised where practical.

This is stronger than saying “we test a lot,” and more honest than saying “all software written in Jet is bug-free.” It preserves the owner's complete compiler-proof requirement without confusing compiler correctness with all possible application intent.

## The test suite should maximize distinct defect detection

A test earns its place by rejecting a plausible wrong behavior. The suite should not grow because a wrapper was added or a field moved.

| Keep | Remove or consolidate | Why |
|---|---|---|
| A regression that reproduces a real wrong result or state transition | A test that asserts a private field was copied | Only the former defends a consumer-visible contract |
| Boundary and precedence cases | Many rows that take the identical path without a distinct risk | Repetition is not coverage |
| Diagnostic snapshots for registered errors | Wording pins on incidental internal text | Diagnostics are a product; incidental strings are not |
| Tier differential examples with real outputs/errors | Mock adapters that echo supplied values | Echo agreement cannot find semantic divergence |
| Independent property/reference checks | Two routes sharing the same buggy oracle | Independence makes disagreement informative |
| History tests with shrinking | Unbounded sleep/poll tests for deterministic events | Controlled events are faster and more reliable |
| Target/device integration where the boundary matters | Host-only assertions claimed as physical proof | Evidence must reach the claimed surface |

### Measure the suite's usefulness

Build a mutation ledger over named defect shapes. For each deliberate plausible fault, record which tests reject it and their cost. Preserve a compact set that kills each meaningful mutant, then retain independent integration evidence where mutations do not model the risk.

Do not optimize only for mutation score. Easy mutants can inflate it. Group mutants by semantic shape, include known historical defects, and keep the required platform/tier/capability denominator. A test that uniquely detects a rare safety failure is not deleted because it is slower than a wrapper test.

The existing test-economics card owns this work. A new “test health framework” and a second coverage database would recreate the problem.

## What other projects' harnesses teach

| Project | Actual mechanism | Jet lesson | Limit |
|---|---|---|---|
| [Rust compiler](https://rustc-dev-guide.rust-lang.org/tests/intro.html) | Compiletest modes, library/unit/doc tests, ecosystem testing, performance infrastructure, distribution checks | Use different evidence for diagnostics, code generation, runtime, ecosystem, and packaging | Rust's allowance for some tool failures is not permission to waive Jet's required gates |
| [LLVM testing guide](https://llvm.org/docs/TestingGuide.html) | Regression tests, unit tests, whole-program test suite | Small rejecting cases and complete programs answer different questions | A text check of IR does not establish every runtime observation |
| [Go testing time](https://go.dev/blog/testing-time) | Stable asynchronous test scopes and fake time | Eliminate time guesses at the runtime boundary | External uncontrolled work still needs separate treatment |
| [Hypothesis](https://hypothesis.readthedocs.io/en/latest/stateful.html) | Generated actions, bundles of reusable values, preconditions, invariants, shrinking | Generate meaningful histories and print a minimal replay | The author still supplies the invariant and model |
| [Alive2](https://github.com/AliveToolkit/alive2) | Translation validation integrated with LLVM transformations and tests | Couple optimizer experiments to counterexamples and explicit support limits | Its repository explicitly warns about unsupported interprocedural transformations |
| [CompCert](https://compcert.org/man/manual001.html) | Composed mechanized stage proofs plus explicit trusted/unproved boundaries | Bind each Jet-controlled stage and compose the exact candidate theorem | Its C error/undefined-behavior model is not Jet's semantic authority |

These are methods to adapt, not build systems to copy wholesale. Jet should keep one existing command/gate owner and improve the evidence behind it.

## CI should qualify a candidate, not collect unrelated green badges

A **candidate** is the exact source, generated artifacts, toolchain, package closure, configuration, and proof/test artifacts being considered for release. Changing a relevant input invalidates the affected evidence. A passing result from an older candidate cannot silently qualify the new one.

```text
source + decisions + generated declarations + exact toolchain
                            |
                    frozen candidate identity
                            |
        /-------------------+---------------------\
  proof replay       focused behavior/tier     package/domain
  and coverage       and negative tests        and performance
        \-------------------+---------------------/
                            |
                one qualification decision
```

This composition reuses the adopted qualification/proof/evidence cards. It does not add a second CI product. Each job returns the same candidate identity, exact scope, outcome, and artifact references. A missing target is missing, not skipped-green.

Fast change-time checks run the affected criterion. Milestone closeout runs the composed targeted sweep and fresh integrated review. Release runs the required complete qualification. Repeating a broad suite after every source edit is not a substitute for choosing the right rejecting test.

## Five measurable development quantities

The compiler and tools should be good for people first. The same properties make automated editing more reliable.

| Quantity | Plain meaning | Measure on a fixed task set |
|---|---|---|
| Verdict fidelity | The answer is true for the stated candidate and scope | False success, false rejection, stale result, and unknown incorrectly labeled as success |
| Latency | Useful feedback arrives quickly | Cold/warm check, edit-to-diagnostic, run, first result, and explanation latency |
| Actionability | The next repair is clear and valid | Successful first repair, unnecessary changes, and source location accuracy |
| Context economy | The answer carries enough evidence without a dump | Bytes/lines needed to make the correct decision; omitted critical facts count as failure |
| Repair determinism | The same problem leads to the same safe change | Repeated repair outcome under identical inputs and explicit variation under changed inputs |

No baseline or improvement for these quantities was measured in this campaign. The hypothesis is that shared checked facts and source-first views improve them. The evaluation must include beginners, expert code, foreign boundaries, missing inputs, and stale state rather than only a successful arithmetic example.

## A release plan with real stop conditions

This is the dependency order for delivery, not permission to leave a feature half implemented.

| Gate | Complete outcome | Stop condition |
|---|---|---|
| R0 · Restore a buildable candidate through the existing implementation stream | Fresh compiler construction succeeds; the accepted failure has a real owner and fix | The current construction failure remains; this audit did not retry it |
| R1 · Finish the adopted semantic foundation | One MIR/Core definition path and full capability/adapter coverage, with current examples | A public behavior still has a private engine meaning or missing route |
| R2 · Finish the smallest high-value user jobs | Endpoint/client, typed query, controlled timeout, typed geometry, one-command project path | An advertised ordinary path still requires a manual duplicate or an unavailable stub |
| R3 · Finish changing and resource-bound jobs | Live query publication, resource schedule, columnar lifetime, model package, hostile histories | Wrong/stale result, unbounded retained state, hidden copy/transfer, or uncontrolled effect |
| R4 · Complete all eight application batteries | Real input, failure, artifact, deployment, and target evidence for each required area | A printed gate, source export, or headless transcript substitutes for the real application |
| R5 · Qualify the exact release candidate | Full adopted proof, capability denominator, diagnostics/examples, platform/domain, performance, and professional handoff gates | Any required claim is false, missing, stale, unsupported, unknown, or contradicted |

R0 and R1 are not excuses to stop design work, and later gates are not excuses to close incomplete implementation cards. Each coherent card closes on its complete observable criteria. The release remains blocked until all required gates pass.

### Performance cannot be traded away quietly

Required cells cover foundations and the eight application areas. A Jet/Rust ratio at or below 1.05 is permitted parity, not a win. Every required non-Rust comparison needs a ratio below 1.00. Apply this per cell and metric. Wrong, unavailable, mismatched, uncovered, or inconclusive evidence fails the gate.

No proposed feature in this report has earned a Jet/peer performance win. The expected savings are hypotheses: fewer copies, less repeated calculation, fewer front-end passes, shorter setup, and more precise tests. Their whole-job measurements remain required implementation evidence.

## What would make this audit fail

The report fails if it merely renames existing decisions, adds a facade without deleting duplicated work, hides an expert control, depends on AI to develop ordinary Jet, or claims runtime/proof/performance evidence it does not have.

The product direction fails if a beginner must learn all twelve proposals before doing one useful task. It also fails if the advanced path cannot be inspected or refused. The intended result is a small ordinary language whose checked relationships remain useful as the job becomes harder.
