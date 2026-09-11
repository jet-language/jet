# Trust: a checked claim with a visible boundary

[Master report](index.md) · [Capability ledger](capability-ledger.json) · [Frontier mechanisms](02-frontier.md) · [Stable-readiness plan](07-readiness.md)

Jet should aim for a mechanized account of what compilation preserves, then use tests to challenge the specification, implementation, and external assumptions independently. A green suite cannot establish universal correctness. A proof with an unstated boundary cannot establish it either.

This report answers questions 19–25. It starts with the strongest assurance architecture and names every reduction in scope. No compiler proof, external paper artifact, Jet suite, or project gate was executed by the author. The current-tree baseline build failed. Existing source and historical receipts are not promoted to fresh execution evidence.

<a id="q19"></a>
## Q19. Functionality is a relation over intended behavior, implementation, and evidence

**Direct answer.** Derive a complete capability denominator from canonical registries, then join each capability to a behavioral contract, actual implementation route, observable result, and candidate identity. A declared member, a test file, a “done” card, or an example that only compiles is not sufficient.

The [capability ledger](capability-ledger.json) preserves the collected first-party inventory without pretending its rows are independent proof. The language packet contains normalized identities with multiple original observations. Both are retained where they differ; this report does not choose the first arbitrarily. Source-reference metadata corrections identify locators, not working capabilities.

### Required relation

For each capability, record:

1. **Identity and intent.** Canonical public name, kind, applicable target/mode, ratified rule, input domain, normal observations, failure behavior, and resource/lifecycle obligations.
2. **Implementation route.** Parser/checker/fact producer, shared lowering, Core implementation, adapter, and external dependency where relevant. A wildcard route or registration bit does not prove the behavior exists.
3. **Observable proof obligation.** A program that consumes the returned value or observes the effect. Also include a boundary case that would fail if the rule were absent or wrong.
4. **Evidence method.** Source inspection, historical execution, fresh execution, differential agreement, property result, checked certificate, or theorem. Keep these categories distinct.
5. **Identity and validity.** Source revision, compiler binary, target, relevant inputs, oracle, and normalization. A change to a relevant input makes the old evidence stale.
6. **Disposition.** Passed for this claim, failed, missing, unavailable, stale, or explicitly not applicable. Every negative state has an owner; none silently leaves the denominator.

Existing [hardening-rig work](../../../scripts/agent/hardening-rig.mjs), #2335, #2285, and #2286 already establish much of the intended inventory and result model. #2898 covers construct/Core execution-mode census; #2902 covers formatter/editor projections; #2903 covers executable examples. Reuse these sources instead of writing a competing registry.

### Capability-to-obligation map

| Capability family | Intended contract source | Necessary observable evidence | Present campaign status and owner |
|---|---|---|---|
| Grammar, operators, literals, declarations | `Syntax.rs`, syntax decisions, parser/checker | Parse/check/format round trip; accepted and rejected programs; equivalent spellings preserve meaning | Source inventory only; #2898, #2902, #2903, #2919 |
| Values, types, facts, ownership | Living spec, AST/checker facts, memory decisions | Alias/move/copy boundaries, mutation, rejection, escape, layout, failure behavior | Source and dated evidence; no fresh qualification; #217, #2898, #2919 |
| Core modules, receiver methods, fields, nominal types | Core source, exports, signature and route registries | Value-consuming calls, field access, constructor/use pairs, failure/edge cases on every applicable mode | #2335 and #2286 denominator exists by recorded work; current matrix unexecuted |
| Compile-time evaluation and metaprogramming | Stage/fact law, comptime implementation | Same relevant inputs yield same facts; runtime inputs cannot leak into compile-time claims; rejection is accurate | Source inventory; #2920/#2921 and #2919 retain current proof |
| Shared lowering and target adapters | I3/I9, D-TIER-ONEIR1 | Same trace, result, failure, and external interaction where the target applies | Ratified architecture and implementation-only work; #2898/#2919 |
| CLI and project checking | CLI registry, project-check and verdict decisions | Actual command pass/fail/partial/unavailable outcomes, scope, application argv, streaming, JSON consistency | Source present; #2389/#2505/#2900/#2919 |
| Editor, formatter, debugger, live tooling | LSP/editor host, source identity, debugger contracts | Real editing, in-flight cancellation, rename, stop/resume, stale values, accessible fallback | Source and historical fixtures; current browser/editor/native qualification not run |
| Tests, proof artifacts, replay, budgets | [Proof-artifact producer](../../../Source/CmdProve.rs): `.jetproof` files and `jet.jproof` schema; replay decisions and suite manifests | Invalid artifact rejection, nonvacuous property detection, replay divergence, real tier execution | Existing #1127/#1131/#1905/#2644; current runtime still #2919 |
| Build, packages, target tools | Package/output/target law and driver contracts | Cold/warm/offline build, dependency closure, target selection, reproducible inputs, failure truth | Source inventory and dated evidence only; existing measurement/build owners |
| Foreign declarations and conversion | FFI decisions, binder and bridge contracts | ABI/layout, ownership, errors, callbacks, tool identity, manual remainder, mixed-project execution | #1120/#1123/#1125/#1156/#1346/#1347; no near-native or full-conversion claim |
| Domain libraries and complete applications | Full eight-area mission, Core/domain contracts | Real complete workloads, ordinary errors, lifecycle, target behavior, performance, comprehension | Inventory is not domain qualification; #2858/#2919 plus domain owners |
| Release claims | Owner prerelease ruling, future release policy, candidate evidence | Every advertised promise joins to current same-candidate evidence | Current text contradiction; new #2927; release remains blocked |

A row is not complete because another row tests a similar implementation detail. Conversely, one strong behavioral case may satisfy several explicitly named obligations. The relation should make that reuse visible rather than multiply test files.

<a id="q20"></a>
## Q20. Absolute bug freedom is not an honest promise; conditional semantic preservation is a serious one

**Direct answer.** Jet cannot guarantee that no functionality is ever missing or that every application always does what its author intended. Requirements can be wrong, environments can violate assumptions, and unrestricted program properties are not generally decidable. Jet can make much stronger, precise promises than “we ran many tests”: defined semantics, checked compilation preservation, excluded defect classes, explicit external contracts, and a denominator that makes omissions visible.

### Start with the strongest architecture

The proposed architecture has one chain of meaning:

- A formal source semantics defines values, mutation, failures, effects, tasks, and external observations.
- Parsing, checking, and compile-time evaluation have explicit relations to that semantics. A checked program is not merely an AST with flags.
- Canonical facts justify one lowering into shared operations.
- Optimizations preserve the specified observations, including defined failures and ordering.
- Every execution adapter implements the same operations under a stated target model.
- Jet-owned Core/Prelude runtime implementations satisfy their contracts, beyond merely preserving call/return plumbing.
- Foreign code, operating systems, hardware, and explicitly unsafe contracts are named boundaries rather than hidden assumptions.
- Independently replayable evidence binds the proved object to the actual implementation or accepted translation, with compiler, model, checker, and target identities.

The desired theorem is conditional: successful Jet-controlled translation preserves source-permitted observations under stated models and premises. Its target is the named per-mode output or interpreter behavior, not an implicitly proved physical-machine executable. Define termination, failure, evaluation order, I/O, and relevant resource observations explicitly. Do not borrow C's undefined-behavior freedom for Jet's defined errors.

[CompCert's manual](https://compcert.org/man/manual001.html) is a useful example of precision. Its current pipeline includes a proved parser and a verified Clight-to-assembly phase, before external assembling/linking. The preservation theorem allows compilation failure, selection among permitted behaviors, and improvement of undefined behavior. Execution time and memory use are not observations in that theorem. Historical descriptions of its parser must not replace the current boundary.

### Prove algorithms, or prove a checker and check results

Two techniques can cooperate without lowering the goal:

1. Prove a transformation algorithm correct for all valid inputs in its model.
2. Prove an independent result checker sound, then require an accepted check for each transformation result covered by that route.

A tested checker is not automatically a sound checker. A bounded search that finds no counterexample is not a universal theorem. Solver timeout, unknown, unsupported operations, and missing certificates remain unproved. The checker and its execution chain belong in the trusted-computing account.

Use static algorithm proofs where they avoid repeated validation cost. Instance validation can help with complex optimizers, but its compilation latency and supported semantics must be measured. The goal does not require proof-only runtime instrumentation in ordinary programs.

### The ratified owner contract

[D-COMPILER-PROOF1](index.md#decisions), on [#2925](index.md#card-2925), is **ratified A as of 2026-09-05**. It requires checked preservation for all Jet-controlled stages before 1.0. The full, non-draft, process-4 ballot retains six historical boundaries; the owner selected all Jet stages. The [research publication record](tower-publication.json) preserves publication history, not the later verdict.

The contract combines proof techniques and retains independent tests. It does not select a proof assistant, approve a solver dependency, add syntax, or establish Full-Jet feasibility. The 1.0 date remains conditional on completing the obligation. The [new-design chapter](08-new-capabilities.md#n01) supplies #2934–#2941 and #2944, with the separate D-COMPILER-PROOF-TOOLS1 tool choice.

This ballot is not a claim that Jet is currently verified, and it does not rewrite I1–I9, D-TIER-ONEIR1, or the protected historical ballots.

### Bind the theorem to the implementation

A proof about an algorithm does not prove an unrelated Rust implementation. The acceptance record must name the proved object, relation, model, exact source/build/checker identities, and every trusted step connecting them. A hash proves identity, not correspondence.

Three binding shapes are accepted in principle: extracted or in-logic generated implementation; direct source verification under an explicit implementation-language model; or per-translation certificates accepted by a sound, artifact-bound checker. Extraction and source verification still need an honest build-chain boundary. The bounded experiment must compare those shapes. No compiler rewrite, proof tool, or dependency is approved; a concrete major architecture choice needs a later owner ballot.

The ratified contract includes parsing, checking, **compile-time evaluation**, initial lowering, optimization, adapters, and **Jet-owned Prelude/runtime implementation correctness** against Core contracts. Correctly calling a Prelude symbol is not proof of that symbol's behavior. The captured corpus supplies no direct Rust or Cranelift semantic verification model; these are explicit missing prerequisites, not literature-wide impossibility claims.

| Mode | Jet-controlled theorem boundary | Still-trusted foreign suffix unless separately checked |
|---|---|---|
| Release | Emitted Rust under a stated Rust model | rustc, LLVM, assembler, linker, and their execution environment |
| Development | Emitted Cranelift IR under a stated model | Cranelift and the toolchain that executes it |
| Web | Emitted web output under a stated model | Browser engine and host environment |
| Interpreter | Jet-owned interpretation and applicable runtime behavior | The Rust compiler/build chain producing the interpreter, unless a stronger binding covers it |

The contract is not an end-to-end physical-machine theorem for a released executable. It requires checked Jet-controlled transformations while making foreign-tool assumptions visible. The existing differential and performance gates still cover those tools. A later checked foreign suffix can strengthen the claim only with new evidence.

### State refinement, resource premises, and failed-check behavior

When compilation succeeds, target behavior must refine the source model: completed results, failures, and externally visible event traces are source-permitted under named premises. Target scheduling may select an allowed source schedule; it may not invent a deadlock or violate a promised progress condition. No generic equality of elapsed time or unbounded-memory guarantee is implied.

State stack, allocation, fuel, configuration, and fairness premises explicitly. A target-only resource-exhaustion outcome is admissible only where ratified semantics permits it and the stated premise fails. Otherwise it needs a separate owner semantic decision. This ballot creates no new I9 carve-out, timeout exemption, or undefined behavior.

An always-rejecting compiler can satisfy conditional preservation, so supported acceptance remains an independent conformance/example obligation. A checker that rejects or returns unknown must take a deterministic already-proved fallback, or stop as an internal compiler failure if no such path exists. It cannot accept unchecked output or blame valid source. A slower fallback still has to meet every required performance cell.

The denominator is the ratified I9 capability/mode set. Removing a mode or capability is not a way to satisfy the proof gate without a separate owner decision. The formal model is not a second authority: compare it with ratified law and I5 examples, repair model errors, and block unresolved semantic conflicts.

### The design-away pass preserves the alternatives as history

Proving the back half resembles an important CompCert boundary. Proving a reference interpreter gives differential testing a stronger oracle. Requiring executable semantics makes the contract precise even without production proofs. These remain credible historical alternatives, not permitted fallback scopes after ratification of A. Any narrower obligation needs an explicit later owner ruling.

More resources and modular checking can build the missing model and binding. They cannot replace the obligation to establish them before release. Dependency-bound reuse avoids needless reproof. The remaining A-specific loss is an unmeasured proof obligation that gates 1.0, plus renewed correspondence for changed covered semantics. No option guarantees a date under the existing handoff gates.

One fresh Luna beginner pass and one Fable challenge informed the ballot. The authors repaired terminology, artifact binding, foreign-tool and Prelude/comptime scope, refinement premises, alternatives, and design-away detail. No correction re-review occurred. [The full disposition](challenge-disposition.json) records each finding and response. Publication does not claim that Fable reviewed or approved the repaired version.

### Why proofs remain valuable when tests are necessary

A test asks whether selected inputs behaved correctly. A proof asks whether every execution allowed by a model satisfies a property. The proof removes an entire class of uncertainty inside that model. Tests then challenge the model-to-code boundary, the specification, omitted operations, external behavior, and the proof toolchain.

| Method | What it establishes | What it cannot establish alone |
|---|---|---|
| Type soundness | Well-typed programs do not reach the specified stuck states | Correct compilation, correct business rules, complete APIs, or physical device behavior |
| Semantic-preservation proof | Covered compilation does not introduce behavior outside the stated source relation | Correct source requirements or unmodeled loader, library, hardware, and runtime behavior |
| Translation validation | A particular transformation satisfies a checked relation within the validator's semantics | Every future transformation, unsupported features, or a soundness theorem for an unproved validator |
| Model checking | A finite model satisfies a property or exposes a counterexample | The complete implementation unless its relation to the model is established |
| Differential testing | Implementations agree or disagree on a generated/selected input | Correctness when all implementations share the same defect |
| Property testing | Sampled executions satisfy a stated law | The law is the right requirement, or all possible cases satisfy it |
| Mutation testing | The selected test portfolio notices deliberate plausible mistakes | Absence of all real mistakes or correctness of surviving mutants |
| End-to-end qualification | A named candidate performs the tested real workflows | Every environment, every input, or future releases |

[CompCert](https://compcert.org/man/manual001.html) and [CakeML](https://cakeml.org/) show why proof is practical evidence rather than a philosophical ornament. The [2011 Csmith paper, §3.1](https://users.cs.utah.edu/~regehr/papers/pldi11-preprint.pdf) reported no CompCert middle-end bug after about six CPU-years, while finding unverified frontend defects. This supports a real distinction between proved and tested transformations, not a universal bug-rate estimate.

The same section records a missing PowerPC immediate-width constraint in CompCert's target semantics. The assembler caught the out-of-range operand. This is precisely why model and tool-boundary checks remain useful alongside preservation proofs: a theorem cannot supply a premise its model omitted.

Independent tests remain useful for model validity, accepted-language coverage, omitted components, toolchains, and integration. [Alive2](https://github.com/AliveToolkit/alive2) supplies a different boundary: bounded LLVM translation validation. Its bounded checks are valuable without becoming a theorem for all Jet programs.

### What remains outside an honest promise

A safe, correctly compiled program can use the wrong tax rate, query stale data, select an inadequate collision model, or wait forever under a permitted schedule. FFI can violate a declared contract. A target model can omit a hardware behavior. A theorem can faithfully preserve an incorrect language rule.

The response is not to abandon proof. It is to state which property was proved, make external obligations visible, and use domain contracts and independent observations for the rest. “All compiler-owned paths covered” is a meaningful goal. “All software is always correct” is not.

<a id="q21"></a>
## Q21. Research optimization aggressively, but separate legality from profit

**Direct answer.** Yes. Use explicit semantic preconditions, independently checked before/after relations, and matched performance measurements. Begin with transformations that preserve information Jet already has, not speculative instruction tricks. This campaign executed a numeric counterexample model but no Jet optimization benchmark.

| Transformation | Semantic preconditions | Counterexample to include | Correctness oracle | Profit evidence |
|---|---|---|---|---|
| Constant folding and dead-result elimination | Exact numeric rules; no discarded effects; defined failure preservation; stage identity | An unused expression can still fail or perform I/O | Source and transformed traces, including failure order | Compile time, code size, runtime; no benefit inferred from fewer nodes |
| Map/filter/loop fusion | Callback effects, evaluation order, aliasing, bounds, laziness, early-exit behavior | A fused callback runs fewer times or consumes input past `break` | Values plus event count/order and unread input | Allocations, peak retention, first-result latency, total work |
| Copy elimination | Ownership, alias, lifetime, and mutation facts justify reuse | A later mutation changes an earlier independent value | Alias-sensitive observations and lifetime checks | Allocation/copy census before instruction tuning |
| Bounds-check elimination | A proved interval/shape relation remains valid across mutation | A collection changes after the bound fact was established | Negative boundary cases and checked relation | Removed checks without moving errors or changing default safety |
| SIMD and parallel reduction | Defined reduction order or an explicitly permitted numerical contract; independent writes | Binary64 reassociation produces 1 versus 0 | Exact bits where promised; documented tolerance only where already part of the task | Same input/output contract, host, compiler, and workload; include crossover |
| Layout selection | No externally fixed ABI or pointer-observation requirement; preserved identity and fields | A C consumer reads the old field offset | Layout relation and mixed-language call fixture | Cache behavior, copies, memory, and all affected operations |
| Specialization and partial evaluation | Stable inputs, correct invalidation, preserved effects and dynamic fallback | A changed environment reuses a specialized old answer | Warm/cold equivalence and stale-input mutation | Compile latency, code growth, runtime, retention |
| Equality saturation | Every equality valid under its guards; extraction respects effects and cost | A mathematical identity fails for overflow or floating point | Rule proof or sound checker; deliberately false rules rejected | Search/extraction cost and complete program results |

The relevant primary sources and limits are in [Q6](02-frontier.md#q6). Their reported speedups motivate experiments; they do not license a Jet forecast. AOT must retain Rust/LLVM quality under the ratified direction. Shared lowering is not a reason to give up a strong backend.

The optimization decision record should answer three separate questions: Is the transformation legal? Why was this version chosen? Was it actually faster on the relevant workload? Reuse #2899 and the protected D-ACCEL1 contract. Do not add a second inspection vocabulary.

<a id="q22"></a>
## Q22. An efficient suite purchases distinct detection, not a larger test count

**Direct answer.** Organize the suite by the wrong behavior each test can detect and the strength of its oracle. Keep small reproducible witnesses for uncertain contracts, end-to-end workflows for integration, and generated search for unexplored combinations. Delete redundant assertions about wiring or copied defaults rather than preserving them as “coverage.” No suite cleanup is performed in this research campaign.

### Portfolio

| Layer | Keep when it detects | Avoid |
|---|---|---|
| Small behavioral/unit cases | A real boundary, precedence rule, state transition, or error relation | Tests that only echo mocked arguments, copy fields, or assert nonempty output |
| Diagnostic UI snapshots | Wrong blame, missing information, unusable fix, wrong error class | Blessing a changed message without checking what/why/fix and exit behavior |
| Golden examples | Observable language and Core semantics across applicable modes | Bind-and-discard examples, compile-only “execution,” or accepted broken-mode categories |
| Law/property packs | Algebraic and domain relations over broad valid inputs | Properties too weak to distinguish the bug, or tautologies of the implementation |
| Differential programs | Adapter disagreement and lowering mistakes | Treating agreement as an independent oracle when engines share the same rule |
| Grammar/type-aware generation | Unexpected valid combinations and invalid-program diagnostics | Uncontrolled undefined behavior, invalid input swamping useful cases, unrecorded seeds |
| Mutation sampling | Silent-data and false-green detection sensitivity | Exhaustive trivial mutants or interpreting a score as proof of correctness |
| Full workflow/platform qualification | Environment, target, UI, FFI, lifecycle, release integration | Repeating the whole project suite per small edit or hiding missing hardware as a pass |
| Performance corpus | Regressions in complete matched workloads | Averaging away a losing cell or comparing different output semantics |

### A decision rule for keeping a test

Name a plausible wrong implementation that the test rejects. If that implementation still passes, strengthen the observable assertion or remove the test's claim. If another test detects the same wrong behavior more clearly and cheaply, keep the stronger case unless the second has a distinct integration boundary.

One case can discharge multiple named obligations, but the manifest must expose that relationship. Preserve a minimized regression when it is a durable boundary witness. Use temporary experiments for implementation plumbing. Quarantining a flaky or unavailable test may preserve workflow, but it cannot silently remove the corresponding release obligation.

Efficiency needs measurements: suite wall time, build reuse, memory, failure-detection latency, duplicate oracle coverage, and mutation sensitivity. None was measured here. Existing [suite partitions](../../../tests/suites.txt), [budgets](../../../tests/suite_budgets.txt), and #211/#806/#2334–#2343 are the implementation base. #2919 owns the composed campaign.
The implementation now consumes those records with [`test-economics.mjs`](../../../tools/agent-eval/test-economics/test-economics.mjs), exposed through [`scripts/agent/test-economics.mjs`](../../../scripts/agent/test-economics.mjs). It joins the hardening manifest, raw oracle bundles, historical defect corpus, suite budgets, and gauntlet measurements into a per-obligation report. Selection is deterministic: mandatory mode obligations, unique defect shapes, and independent oracle classes are satisfied before measured wall time or memory can break a tie. Every candidate record carries the frozen commit, binary, registry, and configuration identity plus raw stdout/stderr, exit, command, source hash, run ID, and oracle relation. Current records remain blocked because the manifest has 2,198 missing rows and no measured cost for the available hardening bundles; no test is removed or labeled redundant without same-candidate replacement evidence.


<a id="q23"></a>
## Q23. Other languages combine different kinds of evidence

**Direct answer.** Mature compiler projects use layered tests, generated programs, platform fleets, ecosystem checks, and focused performance tracking. Verified compilers add formal preservation. None of the named systems proves arbitrary application intent or every physical environment.

| Project | Primary evidence strategy | What Jet should take | What not to infer |
|---|---|---|---|
| [Rust compiler tests](https://rustc-dev-guide.rust-lang.org/tests/intro.html) | `compiletest` UI, codegen, MIR, debuginfo, incremental, run-make, docs, package tests; ecosystem and performance infrastructure | Choose a harness by the consumer-visible contract and keep bug witnesses reproducible | Rust's testing scope is not a whole-compiler theorem; ecosystem checks have platform and package exclusions |
| [LLVM testing guide](https://llvm.org/docs/TestingGuide.html) | Unit tests, `lit` regression tests, and whole-program test-suite measurements | Separate transformation checks from complete-program behavior; reduce failing inputs | FileCheck patterns alone do not prove semantics or runtime performance |
| [Go compiler/toolchain tests](https://go.dev/src/cmd/dist/test.go) and [testing package](https://pkg.go.dev/testing) | Named dist tests, package tests, examples, fuzz seeds, cross-target builders, explicit result states | One runner with reproducible selections and honest exclusions | A compile-only target is not an executed target; example without expected output may only compile |
| [CPython test guide](https://devguide.python.org/testing/run-write-tests/) | `regrtest`, isolated tests, randomized order, resource tagging, reference-leak tests, builders | Preserve order seeds, unavailable-resource states, and the first failing result | A rerun pass is not evidence the first failure never happened |
| [CompCert](https://compcert.org/man/manual001.html) | Composed mechanized semantic-preservation proofs plus independent testing | State the theorem, allowed failures, observations, and unverified boundaries precisely | Its theorem is not preprocessing-through-physical-machine correctness for arbitrary C environments |
| [CakeML](https://cakeml.org/) | Formal language semantics, verified compilation, supported backend proofs, bootstrap reasoning | Connect the language specification and implementation rather than verify an unrelated model | External loading, machine assumptions, and application requirements remain separate |
| [Lean reference](https://lean-lang.org/doc/reference/latest/) | Proof terms checked by a small logical kernel; executable tooling has a wider trust chain | Distinguish a checked logical theorem from trusting generated native execution | Native computation and extra axioms can enlarge the trust boundary |
| [Csmith](https://embed.cs.utah.edu/csmith/), [Alive2](https://github.com/AliveToolkit/alive2) | Valid-program differential testing and bounded translation validation | Generate precise counterexamples and keep seeds/reducers | A clean finite corpus or bounded search cannot establish general absence |

This comparison is not a league table. Rust/LLVM optimize for broad production use; CompCert/CakeML make different proof and scope commitments. Jet should retain strong optimization, broad applicability, and formal ambition by proving or checking the boundary between them, not by pretending their tradeoffs do not exist.

<a id="q24"></a>
## Q24. Their harnesses are concrete runners with explicit oracles and result states

**Direct answer.** The reusable architecture is a discovered corpus, one immutable run description, bounded workers, a suite-specific oracle, normalized but preserved raw output, and a reducer or rerun recipe. The details differ because the contracts differ.

### Rust

[Compiletest](https://rustc-dev-guide.rust-lang.org/tests/compiletest.html) uses source directives to select revisions, expectations, targets, and modes. UI tests compare normalized diagnostics and fixed output. Codegen and assembly suites inspect generated artifacts. Incremental tests compare revisions and reuse behavior. Run-make covers tool interactions. The [running guide](https://rustc-dev-guide.rust-lang.org/tests/running.html) exposes path selection, stage choice, cached results, deliberate `--bless`, and rerun controls.

Typical documented commands include `./x test tests/ui` and a single test path. They require a configured Rust checkout; they were not run here. Jet should copy precise selection and output identity, not the assumption that an expected-output update is self-justifying.

### LLVM

[`lit`](https://llvm.org/docs/CommandGuide/lit.html) discovers configured tests, executes `RUN` commands, supports target features, sharding, timeouts, and explicit outcomes such as pass, fail, unsupported, expected failure, unexpected pass, and flaky pass. [FileCheck](https://llvm.org/docs/CommandGuide/FileCheck.html) matches meaningful generated output. [llvm-reduce](https://llvm.org/docs/CommandGuide/llvm-reduce.html) shrinks a failure while preserving an interestingness condition.

The [whole-program test suite](https://llvm.org/docs/TestSuiteGuide.html) compiles, links, runs, compares reference output, and can record compile time, execution time, size, and statistics. This separation matters. A generated-code pattern is evidence about structure; a real output comparison is evidence about behavior.

### Go

`cmd/dist` registers named tests, validates selection, schedules bounded background work, and reports failure, no tests, or success with exclusions distinctly. The ordinary `go test` model compiles a separate test binary, supports examples with expected output, package result caching, fuzz seeds, and persistent failure inputs. `-count=1` disables ordinary package-test cache reuse when a fresh run is required.

Jet should retain the named immutable run description and explicit compile-only distinction. A missing test selection must fail or say no tests, not look like success. No new standalone runner is needed if the current suite/hardening runner already owns this contract.

### CPython

`regrtest` records selected tests, random seed, resources, timeouts, worker configuration, and failure policy. It isolates tests, detects changed environment state, distinguishes skipped/resource-denied/worker-failed/did-not-run/timeout results, and can rerun or bisect failures. The [guide](https://devguide.python.org/testing/run-write-tests/) documents copying the actual failing command and reproducing randomized order.

Jet should preserve the first failure and its environment even when a rerun succeeds. A test that requires unavailable resources needs a visible missing obligation. An exit code for “nothing ran” is not an ordinary pass.

### The Jet adaptation

Reuse one current manifest and result record. Each run should preserve source, compiler and target identity, exact command, environment inputs, seed, raw stdout/stderr, exit, oracle relation, normalization, and artifact hashes. The runner may summarize these facts but must not discard the raw evidence needed to reconstruct a failure.

Normalize only irrelevant variation. Do not normalize away failure class, Unicode semantics, stream timing, field order when order is promised, or a meaningful target distinction. Minimize failures by preserving the failing relation, not merely a crash. Reuse #2335–#2343 and #2919; the report does not authorize another competing harness.

<a id="q25"></a>
## Q25. CI should qualify one candidate, not assemble green fragments

**Direct answer.** Use fast focused feedback during development, then a complete same-candidate qualification at integration and release boundaries. Every supported target and obligation stays in the denominator even when a runner is unavailable. A release cannot be assembled from unrelated green commits, stale binaries, and skipped checks.

### What the primary pipelines do

| Project | Concrete pipeline design | Useful lesson and limit |
|---|---|---|
| [Rust CI guide/source](https://github.com/rust-lang/rust/blob/master/.github/workflows/ci.yml) | Pull-request, try, optional, and auto/merge jobs are selected by a job database and `citool`; merge jobs must be green | Fast PR feedback and stronger integration proof can coexist. A reduced PR set must not be presented as release coverage. |
| [Go builders](https://go.googlesource.com/build/+/refs/heads/master/dashboard/builders.go) | Builder/host configuration records platform, bootstrap, environment, owners, known issues, compile-only status, and test policy | Platform identity and ownership belong in the result. A host list is not proof every target executed. |
| [CPython buildbots](https://devguide.python.org/testing/buildbots/) and [PEP 11](https://peps.python.org/pep-0011/) | Stable builders span platforms and configurations; tier-one/tier-two persistent failures block release under the stated policy | Release support has explicit operational obligations. Jet cannot silently demote a promised platform to make a gate green. |
| [LLVM test suite](https://llvm.org/docs/TestSuiteGuide.html) | Separate runtime, compile-time, code-size, cross-target, and profile-guided configurations | Preserve configuration and workload identity. A performance baseline must match the candidate relation being claimed. |

### Proposed Jet qualification flow

1. **Edit-level evidence.** Run the exact changed behavior and its plausible failure boundary using the integrated tool. Do not repeat the broad suite for every small source edit.
2. **Integration boundary.** Freeze a candidate identity. Compose all affected semantic, CLI, example, diagnostic, target, and tooling obligations. A changed input invalidates only evidence whose dependency relation includes it.
3. **Independent challenge.** Generate valid programs, compare modes, exercise state models, and sample deliberate mutations. Retain seeds and reductions. These are the existing hardening mechanisms, not a new queue.
4. **Performance boundary.** Run the complete matched corpus. Preserve failures and every losing cell; do not average them into a pass.
5. **Domain and platform boundary.** Exercise real applications and supported environments, including offline and missing-tool recovery where promised. A simulator claim names what the simulator omits.
6. **Release qualification.** Bind all required evidence to the same candidate, canonical manifests, toolchain, and target identities. Check the owner-ratified proof policy and external contract list.
7. **Publication and recovery.** Verify the delivered artifacts match the qualified ones and that documented installation and rollback/recovery behavior is actually usable. No version banner substitutes for this evidence.

Existing #2339's professional-handoff gate includes ratified quantitative thresholds: 14 clean days, 10,000,000 valid cases, 100 valid mutations per eligible callable, and a prescribed fresh-context quota. Those are historical owner law, not measurements achieved by this campaign. This non-cyber investigation neither runs excluded review content nor silently redefines an existing broader gate. It records that applicable gate evidence remains owed.

The current tree cannot enter this flow as a qualified candidate because its fresh build failed. [#2919](index.md#card-2919) is the existing integration and validation owner. The [readiness plan](07-readiness.md#q30) puts foundational decisions before this final qualification rather than treating a suite run as a substitute for design.

## Generalize every finding

| Finding | Defect shape | Predicted other instances | Structural correction | Action disposition |
|---|---|---|---|---|
| A registered capability lacks observable proof | Presence counted as behavior | Receiver methods, fields, constructors, target routes | Derived capability/obligation join with value-consuming probes | Reuse #2285/#2286/#2335/#2898/#2903 |
| A proof record is overclaimed | Evidence promoted beyond its model | Solver success, replay, clean fuzz runs, self-host output | Explicit theorem, checker, assumptions, and scope | [#2925](index.md#card-2925); D-COMPILER-PROOF1=A; #2934–#2941/#2944 delivery |
| A suite passes without exercising the intended path | Oracle or execution identity is too weak | Release tests using the wrong mode, empty selections, cached results | Real-path observations and negative sensitivity checks | Reuse #1905/#2335/#2343/#2919 |
| A transformation changes a defined observation | Optimization precondition missing | Numeric reassociation, copies, streaming, failure order | Shared semantic law and checked transform relation | Reuse #2895/#2898/#2899/#2922/#2923 |
| Green results belong to different candidates | Evidence identity omitted | Build, generated artifacts, target tools, release docs | Same-candidate closure and invalidation | Reuse #2339/#2506/#2919; new #2927 claim census |
| Skips and unknowns disappear from totals | Denominator shrinks around failure | Unavailable hardware, refused modes, solver timeout, missing examples | Explicit failed/missing/unavailable/stale/not-applicable states | Reuse #2335/#2339/#2898 |

No tests, builds, gates, dependency installation, production fixes, or card closures were performed as part of this report authorship.
