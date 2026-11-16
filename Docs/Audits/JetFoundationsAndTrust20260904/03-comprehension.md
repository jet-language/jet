# Comprehension: show the rule, the state, and the reason

[Master report](index.md) · [Mixed-language reasoning](06-interop.md) · [Capability ledger](capability-ledger.json)

Jet should let a person answer four questions without changing tools: What does this mean? What happens next? Why did that happen? What change will fix it? The same compiler-owned account should answer a beginner, an expert, and an automated client. A visual studio may make the answers easier to see; it must not own a different answer.

This report answers questions 9–12 and 14–18. Question 13's full cross-level and foreign-language answer is in [mixed-language reasoning](06-interop.md#q13). The assessment covers the first-party inventory at family level and retains the collected item-level trace in the capability ledger. It is not a claim that every function has been executed or every screen observed.

## Evidence and vocabulary

“Source present” means a declaration, handler, reference page, or fixture was found. “Ratified” means the owner chose the contract. “Historically observed” means a dated run or card recorded behavior. “Fresh model result” means the retained JavaScript experiment ran. None means the current Jet tree works. The fresh compiler build failed; no current Jet learner session, editor installation, or all-domain runtime qualification was possible in this investigation.

The source inventory includes a shared [CLI registry](../../../crates/jet-cli/src/CLI.rs), [LSP server](../../../Source/LSP/Server.rs), [editor adapter](../../../Source/LSP/Adapter.rs), [editor host](../../../crates/jet-devserver/src/EditorHost.rs), [resident session](../../../crates/jet-devserver/src/Session.rs), [learn runner](../../../Source/CmdLearn.rs), [first-hour guide](../../spec/guides/first-hour.md), and [diagnostic recovery guide](../../spec/guides/diagnostic-recovery.md). Existing source is a substantial base. It is not an excuse to infer current success.

<a id="q9"></a>
## Q9. The sharpest gaps are false confidence, divergent meaning, and broken first steps

**Direct answer.** The non-cyber red-team result is not “Jet has no tooling.” It is that an extensive toolchain can still tell a convincing but unsupported story. The principal failure shapes are one command proving less than the user thinks, an engine changing an observation, a stale result appearing current, and an easy route failing while an obscure route works.

| Counterexample or concern | Evidence class | What it defeats | Accurate disposition |
|---|---|---|---|
| Fresh compiler build failed with 161 reported `jet-comptime` errors; an observed example was unresolved `jet_std` in `WebTable.rs` | Fresh baseline failure on the pre-existing dirty tree | Any claim that this report used a fresh current-tree Jet executable | [#2919](index.md#card-2919) owns integrated restoration and proof. No feature attribution is inferred from the broad failure. |
| Historical project checks missed entry, module, Core closure, or lowering failures | Dated finding and source/criteria on #2389 | “Check passed, therefore run cannot have a compiler failure” without an exact check scope | Reuse #2389's ratified project-check contract and #2919's fresh proof. Do not call it currently broken merely because its older body describes the bug. |
| Historical stdin route materialized all lines before the body; AOT pulled one line at a time | Existing #2923 source finding; not rerun here | Equal final output as a complete streaming oracle | Reuse [#2923](index.md#card-2923). Observe first output, unread remainder after `break`, and memory behavior. Do not rewrite D-LOOPREAD1. |
| Layout fact documented for a plain compile-time binding failed on the recorded ordinary spelling | Existing #2921; broader build attribution corrected separately in #2920 | A feature working only inside an obscure context as proof of the ordinary documented route | Reuse #2920 and #2921. Their current execution is still owed. |
| Historical mutation, closure capture, numeric conversion, failure, and display outcomes differed between modes | Dated bug records, many now implementation-complete | A shared parser or common API name as proof of shared meaning | Reuse #2898's census and #2919. Closed implementation records are not fresh universal green evidence. |
| A fact read before an edit attaches after the edit | Executed small event model; not a Jet defect reproduction | An answer without revision identity | Reuse resident identity/verdict/record work. Include the shape in [#2926](index.md#card-2926). |
| Current versioning says 1.0 has shipped, while the owner says prerelease | Exact source contradiction | Version strings as release evidence | New [#2927](index.md#card-2927). Preserve historical ratification. |
| Ada/Pascal import commands prepare binder/manual output, as the binder-only map states | Consistent source/map boundary, not an observed defect | “Supported import command” mistaken for semantic conversion success | Use this as a classification case in the claim census, not a second bug. #1156 and #1346 retain conversion/map obligations. |
| Teaching features have no campaign human outcome measurement | Evidence absence, not a runtime bug | “Readable,” “learnable,” or “intuitive” as established results | New #2926. Source-based RLI5 remains a model assessment. |

This is aggressive because it challenges the meaning of success, not because it manufactures more defects. The complete negative program inventory and fresh all-tier reruns remain blocked by the compiler baseline. No unexecuted probe is reported as a pass or a fresh failure.

<a id="q10"></a>
## Q10. Explicit assumptions need narrower, testable statements

**Direct answer.** Jet's goals are useful, but several common interpretations are too strong. Keep the goals and test the interpretation rather than weakening the mission.

| Explicit assumption | Supporting reason | Counterexample or contrary evidence | Corrected claim and test |
|---|---|---|---|
| One implementation gives identical meaning | Eliminates independent semantic copies | Adapter ABI, representation, host I/O, numeric lowering, and optimization can still diverge | One implementation is a structural prerequisite. Differential traces and preservation arguments qualify each adapter. |
| Safety by default makes programs correct | Excludes classes of memory/type mistakes | Wrong formulas, stale domain identities, time-step errors, and incorrect requirements remain | Name each excluded defect family and its boundary. Use domain contracts for the rest. |
| A formal proof makes tests redundant | A theorem covers all cases in its model | Parser, theorem statement, external functions, hardware model, proof tool, and integration can be wrong or outside scope | Keep proofs and independent tests. [Q20](04-trust.md#q20) gives the exact architecture. |
| One canonical mechanism means one spelling or layout | Fewer meanings reduce surprise | Different useful organizations may share one semantic operation | Preserve I8's semantic unity without forcing ceremonial uniformity. Test equivalent spellings for meaning and cost. |
| Implicit failure is beginner-friendly | Removes repeated propagation syntax | A call can look effect-free while failure exits the current computation | Show the failure route contextually and let experts inspect the full contract. Do not restore retired syntax. |
| Compile-time facts have zero runtime cost | Erased facts can guide compilation | Proving, storing, invalidating, or validating a fact has compiler cost; false facts can license wrong code | Separate runtime erasure from compilation cost and proof validity. |
| A faster edit loop is a better loop | Waiting disrupts work | A stale or incomplete answer arrives quickly and causes a wrong edit | Measure correctness before latency; include invalidation and cancellation. |
| All domains can share one language | Values, state, abstraction, and effects recur | GPU kernels, GUI events, embedded interrupts, and services have different external constraints | Share semantics; retain typed domain contracts, target restrictions, and measured platform obligations. |
| Self-hosting increases trust | Exercises the language and enables dogfood | A compiler can reproduce its own error | Compare independent implementations and bootstrap stages. Evaluate before making it a release gate. |

These are not newly discovered violations of ratified law. They are falsifiable interpretations to use in review and acceptance.

<a id="q11"></a>
## Q11. Hidden assumptions cluster around identity, scope, and completion

**Direct answer.** The implicit assumptions most worth reconsidering are that a name denotes the same thing across time, a result applies to the context currently visible, and an inventory item means usable behavior. They are easy to miss because each local component can be internally correct.

| Hidden premise | Where it appears | Failure shape | Required challenge |
|---|---|---|---|
| A source span still names the analyzed program | Diagnostics, hover, code actions, graphs | An old result points into a new document | Edit while a request is in flight; reject publication for the old revision. |
| A cache's positive dependencies are enough | Entry selection, imports, output profiles | A newly created higher-priority file should invalidate an old answer | Add a previously absent candidate and compare warm/cold results. |
| A slot or name is stable identity | Entities, tasks, debugger references, foreign handles | Reuse gives an old reference access to a new object | Release and reuse the slot; require generation validation or a statically enforced lifetime. |
| A registered method is callable everywhere | Core tables, completion lists, generated docs | Help advertises a route whose lowering or observable result is absent | Join registration to actual dispatch and value-consuming probes. |
| A result code describes the whole operation | CLI wrappers, nested checks, JSON output | Outer success wraps inner failure | Exercise compiler, tool, environment, and partial-result failures through the real command. |
| A warning is harmless advice | Diagnostics and build policy | A routine warning makes the common path fail, or a serious refusal appears cosmetic | Test exact exit and repair semantics, not wording alone. |
| A default is free | Parallelism, copies, capture, materialization, polling | A convenient call changes resource use or ordering | Inspect the decision and compare against the plain operation. |
| A language label describes conversion depth | Import commands and migration maps | Binder generation looks like source translation | Classify output by preserved semantics and manual remainder. |
| A current version string proves a release happened | Docs, banners, package metadata | Historical policy becomes a present claim | Resolve against owner authority and same-candidate evidence. |
| A learner who produces output understands it | Examples, guided exercises | Copying succeeds while prediction and transfer fail | Separate explain, predict, modify, and derive tasks. |

The retained event and handle models are small executable demonstrations of two premises. They do not prove every Jet consumer has been audited. The structural censuses are explicit follow-up obligations, not silent gaps.

<a id="q12"></a>
## Q12. RLI5 must cover the whole first-party experience, with honest evidence labels

**Direct answer.** Treat the language, libraries, commands, editors, runtime tools, build system, and foreign workflow as one learning path. The family census below is the detailed source-based assessment. The [capability ledger](capability-ledger.json) retains each collected inventory identity, exact name or member family, source locator, evidence limits, detailed-report home, and action owner. It does not claim every member was behaviorally requalified.

The beginner has no hidden knowledge of Rust, compiler phases, ownership jargon, a repository layout, or Tower. The expert still deserves precise technical explanations. The product should let one reader move between those depths without switching semantic accounts.

### Explain, predict, modify, derive census

| First-party family | Explain task | Predict task | Modify task | Derive task and likely friction |
|---|---|---|---|---|
| Install, environment, package identity | Say which executable and package are active | Predict behavior offline or with a missing tool | Select an explicit tool or repair a missing prerequisite | Infer why identical source can differ with target inputs. Hidden environment state is the risk. |
| Scaffold and entry selection | Identify what runs first | Predict which entry wins when another file appears | Add a second module without changing behavior | Derive package scope from one example. A successful single file is not enough. |
| Values and bindings | Explain value, name, assignment, and copy | Predict whether a later change affects an earlier value | Make an independent copy intentionally | Derive identity versus equality. Do not teach the outdated “variable as a box” model without its limits. |
| Types, inference, unions, optional values | Explain what values are allowed | Predict a rejected branch or absent value | Handle the missing case | Derive a useful domain type rather than memorize annotations. |
| Functions, generics, traits, modules | Explain a call contract and visibility | Predict which implementation is selected | Extract a helper without changing ownership or failure | Derive abstraction boundaries. Hidden generic obligations need contextual explanation. |
| Branches, patterns, loops, iterators | Explain the repeated step and termination condition | Predict zero/one/many iterations and early exit | Change traversal without losing remaining input | Derive an invariant. Streaming/materialization differences are high-consequence misconceptions. |
| Numeric operators and units | Explain exact versus approximate values and dimensions | Predict overflow, rounding, or a unit mismatch | Change units or reduction order intentionally | Derive why algebraic rewrites have premises. Mathematical notation alone is not the machine contract. |
| Collections, strings, bytes, tables | Explain elements, keys, order, encoding, and shape | Predict aliasing, missing keys, unequal lengths, or Unicode length | Change a nested value and observe the result | Derive data invariants and complexity. A value-consuming probe is required. |
| Mutation, borrowing, resource lifetime | Explain who can change or close a resource | Predict a rejected alias or stale use | Shorten a borrow or transfer ownership | Derive why lifetime rules preserve a useful guarantee. `&` is exclusive write under current law, not read-only lending. |
| Failure, contracts, diagnostics | Explain what failed, why, and the next action | Predict which call can exit and what gets cleaned up | Apply a safe edit and rerun | Derive the difference between a wrong program, unavailable tool, and compiler defect. Implicit failure must remain visible. |
| Tasks, channels, cancellation | Explain who owns work and its result | Predict shutdown, blocked work, ordering, and cancellation | Bound a queue or change the owner | Derive safety versus liveness. “No data race” does not mean the protocol is right. |
| Compile-time evaluation and facts | Explain when a value is computed and where a fact applies | Predict whether a runtime input can influence it | Move the computation to the correct stage | Derive phase separation. A compile-time marker that only works in a special context fails the lesson. |
| Layout, SIMD, kernels, allocation | Explain representation and legal optimization | Predict whether an override changes meaning or only cost | Choose an expert layout or reject parallel execution | Derive legality versus profitability. Inspect actual decisions rather than promise “zero cost.” |
| Core I/O, filesystem, terminal, process, time | Explain the external effect and failure route | Predict blocking, partial output, and resource cleanup | Stream instead of materialize or handle a missing file | Derive environmental assumptions. These are common tasks, so recovery must be direct. |
| Data, math, plots, tensors | Explain shapes, units, missing values, and approximation | Predict aggregation, broadcast, or conversion behavior | Repair a shape mismatch | Derive numerical and data-model principles. A library label is not an oracle. |
| Web, services, persistence | Explain request/state/lifecycle boundaries | Predict stale updates, restart, and partial completion | Add a field or failure path coherently | Derive contracts between components. Generated boilerplate must not hide behavior. |
| Games, graphics, audio, input | Explain simulation versus rendering and object lifetime | Predict frame-step and reuse cases | Change speed without making it frame-dependent | Derive time, state machines, and identity. Visual success can hide wrong timing. |
| GUI/TUI and accessibility | Explain state, event routing, focus, and layout | Predict keyboard navigation and an invalid state | Add an accessible control | Derive state-driven presentation. The visual editor cannot be the only authoring route. |
| Embedded and target control | Explain target facts, layout, and hardware assumptions | Predict unsupported operations or bounded resources | Select a target without changing language meaning | Derive the boundary between language safety and hardware behavior. Host success is not target proof. |
| `check/test/prove/fix/explain/inspect` | Explain precisely what each verdict covers | Predict a stale, unsupported, or unknown result | Repair and rerun until the obligation is satisfied | Derive evidence kinds. A proof artifact and a compiler theorem are different. |
| `dev/debug/repl/notebook` | Explain live state, a stop, and a recorded run | Predict what becomes stale after edit or resume | Replay or inspect without hidden side effects | Derive observation versus execution. Do not invent optimized-away or unavailable values. |
| LSP, formatting, refactoring, docs | Explain names and relationships in context | Predict whether a rename changes semantics | Apply a source-aware edit | Derive syntactic equivalence versus semantic equivalence. Every projection must match the compiler. |
| Build, package, foreign compilers, conversion | Explain dependency, ABI, source authority, and generated remainder | Predict a foreign exception or unavailable binding | Adopt one Jet module into an existing project | Derive mixed-language contracts. Accepted commands must not imply full conversion. |
| Release and evidence records | Explain what was actually qualified | Predict invalidation after a source/tool change | Reproduce the same candidate | Derive a release claim from evidence, never a version banner. |

Cyber-related capability content is excluded from analysis. Inventory exclusions are explicit; excluded rows are not counted as working, missing, or audited.

### RLI5 friction judgments

| Friction | Why a newcomer stalls | Proposed correction | Evidence status |
|---|---|---|---|
| Too many command names before one successful task | A catalog supplies choices but no order | First show `new → run → check → test → fix → explain`; reveal specialized commands when the task needs them | Command path exists in source/docs; fresh first-hour execution remains #2900 |
| Implicit behavior without an explanation nearby | Short code hides failure, copy, or scheduling rules | Contextual “what may happen” facts at calls and bindings | Design assessment; current complete coverage unmeasured |
| Technical implementation terms in recovery | The reader cannot act on a backend or evaluator instruction | Name the command they ran, the invalid rule, and an exact edit or next command | Existing diagnostic census #2906; current reruns owed |
| A green status does not state scope | The learner assumes more was checked than actually was | Name target, scope, engine obligations, and unknowns in the same verdict | Ratified verdict/project-check work exists; no current runtime claim |
| Values shown without time or revision | A correct old answer teaches a wrong current model | Display observed-at identity; mark stale or unavailable explicitly | Source structures plus model witness, not a Jet-wide execution audit |
| “Exercise passed” rewards imitation | Copying a solution can bypass understanding | Require a prediction and a transfer task on a different example | Supported research direction; Jet human outcome not established |

<a id="q14"></a>
## Q14. The next-generation experience is one deterministic explanation loop

**Direct answer.** Jet should be a development system that carries the same meaning through source, check, execution, observation, repair, and release evidence. It does not need an AI narrator or a mandatory studio. It needs reliable facts and good projections.

The common path remains ordinary Jet and ordinary commands. After a failure, the tool should expose the smallest useful explanation first: the rule, the relevant source, the observation, and the next action. An expert can expand the derivation, target assumptions, cost decision, and raw evidence. An enterprise reviewer can reproduce the same record without recreating an editor session.

| Stage | Existing carrier | Required answer |
|---|---|---|
| Write | Source, formatter, completion, hover | What values and operations are available here? |
| Check | `jet check`, project graph, structured reports | Which obligations were checked, and which are not established? |
| Run | `jet run`, `jet dev`, build outputs | Which program identity ran, with which relevant external inputs? |
| Observe | `jet inspect`, debugger, recorded results | What happened, where, and under which assumptions? |
| Repair | `jet fix`, code actions, diagnostic explanation | Which edit is safe, which needs judgment, and why? |
| Recheck | The same command and identity relation | Did the intended obligation disappear without creating a new one? |
| Qualify | Existing proof/receipt and handoff records | Which claims remain current for this candidate? |

Do not add a second semantic service for the visual studio. Its graph, table, timeline, and source view must be projections of the same records used by CLI and LSP. A view may summarize; it must not decide meaning or claim an unobserved value.

<a id="q15"></a>
## Q15. Teach through contrasts that expose the wrong model

**Direct answer.** The useful creative unit is a small behavior contrast, not a long explanation generated after an error. Ask for a prediction, reveal the actual or formally derived behavior, and let the learner change one cause. Reuse real compiler facts and recorded execution.

### Design A: prediction before reveal

The learner sees two short programs that differ in one semantic choice: independent value versus shared resource, exact time versus approximate time, or sequential versus reordered work. They select the expected result and explain it in one sentence. The tool then reveals the answer, the relevant state change, and the rule.

The answer oracle is either a checked static derivation or an actual identified execution. The UI labels which. It never silently runs arbitrary effectful code for a hover. A wrong prediction is evidence about a mental model, not a reason to label the learner wrong in general.

### Design B: the smallest counterexample

When a rule rejects a program, show the smallest path that would violate it. For a stale handle, display allocation, release, reuse, and the rejected old generation. For a resource close, display the deferred callback that would use it afterward. The expert view exposes the underlying lifetime or transition relation.

If the checker cannot produce a valid counterexample, it says the proof is insufficient and names the required condition. Do not invent a runtime failure merely to make the diagnostic persuasive.

### Design C: “change one thing” comparisons

A learner changes frame rate while simulated time stays constant, changes a unit while the represented quantity stays constant, or renames a local variable while behavior stays constant. The tool shows invariants and the first changed observation. This teaches abstraction, units, testing, and causality.

The retained binary64 clock counterexample is especially useful. The learner's claim “one second is one second” needs a representation premise. The exact-tick comparison repairs the experiment without pretending that a production clock has no jitter, overflow, or quantization.

### Design D: a proof-boundary card

Next to a result, display four rows: established fact, method, assumptions, and unknowns. “This branch cannot receive an absent value” is a precise statement. “This program is correct” is not. A beginner sees the consequence; an expert can inspect the derivation; a reviewer can reproduce the identity.

This should reuse the existing report/ledger/proof vocabulary rather than mint a new public command. The new work is the assessment corpus under #2926. Any uncovered public protocol or UI policy returns as a separate owner decision.

### What the research actually supports

[Prediction-versus-production](https://doi.org/10.1016/j.learninstruc.2023.101871) reports benefits in a bounded novice study. [Reading/tracing research](https://doi.org/10.1145/1041624.1041673) and [misconception work](https://doi.org/10.1145/3649823) show that apparently working code can coexist with wrong behavior models. [Ask-Elle](https://doi.org/10.1007/s40593-015-0080-x) demonstrates deterministic feedback using program models and properties, but also records undecided cases. [Victor's Learnable Programming](https://worrydream.com/LearnableProgramming/) offers a design argument for visible state and flow, not a controlled learning effect.

The synthesis justifies testing these designs. It does not establish their effectiveness across Jet, experts, accessibility needs, or long-term transfer.

<a id="q16"></a>
## Q16. Teach transferable principles through real work

**Direct answer.** Start with a useful task, introduce the concept needed to complete it, then test the same concept in a different setting. Do not build a course whose success criterion is remembering Jet syntax.

| Real task | Programming principle | Transfer check |
|---|---|---|
| Transform a file and report a bad line | Data representation, iteration, partial failure | Process a stream without losing unread input after an early exit |
| Build a small score tracker | State, invariants, identity, transitions | Replace several contradictory booleans with a valid state model |
| Plot a measurement series | Units, approximation, missing data | Explain why changing units differs from changing values |
| Add a helper and module | Abstraction, contracts, scope | Change the implementation while preserving callers' observations |
| Load a resource and close it | Ownership, lifetime, cleanup | Reason about a callback that runs after the original scope |
| Run two tasks and stop them | Concurrency, cancellation, liveness | Identify a protocol that is race-free but can still wait forever |
| Call an existing C library | Representation and foreign assumptions | Explain which guarantee ends at the boundary and how to validate it |
| Reproduce a failed build or run | Evidence, determinism, dependency identity | Show why a changed tool or input invalidates the old result |

The progression should be optional and available offline through the toolchain's existing learning path. It should accept more than one correct program where the task permits it. Tests should check the intended relation, not force one sample solution's syntax. An expert can skip guided steps without losing access to explanations.

Measure four outcomes separately: successful task completion, correct prediction, correct explanation, and transfer. Record time and abandonment, but do not reward speed at the expense of understanding. No current human study is claimed here.

<a id="q17"></a>
## Q17. Visuals and LSP features should expose relationships already known by the system

**Direct answer.** Prefer source-linked values, state transitions, ownership/lifetime relationships, and dependency explanations over decorative graphs. Use standard LSP capabilities where they fit. A new protocol extension is justified only when the required relation cannot travel through the existing editor host and record model.

The [LSP specification](https://microsoft.github.io/language-server-protocol/) supplies transport and editor operations, not a universal execution model. Jet already has source handlers for hover, navigation, rename, semantic tokens, inlay hints, completion, call/type hierarchy, and shared fixes. Current availability still requires the deferred editor run.

| Interaction | Exact information needed | Portable path | Failure and honesty rule |
|---|---|---|---|
| Hover: “what this call may do” | Selected symbol, instantiated type, copy/borrow behavior, failure/effect facts, source revision | Hover plus textual `inspect`/diagnostic detail | Show incomplete or unavailable facts instead of a guessed contract |
| Inline value with a time label | Runtime value, program/build identity, stop or event identity, liveness | Debugger variables or an existing recorded inspection result | Clear or mark stale after edit/resume; never synthesize optimized-away values |
| Ownership strip | Owner, active borrows, exclusive operations, scope end, rejected escape edge | Inlay hints, related locations, an ordered text explanation | Do not draw a live borrow graph from type names alone |
| State-transition diagram | Current state, permitted transitions, guard, triggering event, failure/cancel path | Source links and a table of transitions | An omitted transition is “not modeled,” not impossible |
| Event timeline | Ordered observations, causal links, clock model, dropped/unavailable events | Recorded log and keyboard-step list | Do not imply total order where only a partial order is known |
| Change-impact view | Dependency edges, negative lookup facts, invalidation reason, old/new identities | `inspect build`, related information, file links | A cached graph must become stale when its inputs change |
| Optimization explanation | Candidate transform, legality facts, profitability estimate/measurement, expert override | Existing inspect/acceleration work | Distinguish “legal,” “chosen,” and “measured faster”; none implies the others |
| Counterexample pair | Same task, one changed premise, output/state difference, source relation | Readable text first, optional visualization | A model example is labeled as a model, not the user's execution |

### Presentation rules

A timeline needs an ordered text alternative. A graph needs named nodes, edge meaning, and a keyboard path. A color must have a label. Animation should reveal a real transition, then stop. Reduced motion must preserve the information without movement. Large data must be summarized with an explicit bound and a route to the complete record.

The visual studio can compose these views. It cannot require a project to adopt a new architecture merely to be inspected. Exact implementation and acceptance belong to the existing editor/devtools cards; #2926 supplies the coverage and learning evidence that are missing from this investigation.

<a id="q18"></a>
## Q18. Prioritize friction by repeated work and consequence, not by novelty

**Direct answer.** The common path should make package selection, running, understanding a failure, and applying a safe repair nearly automatic. Rare expert operations may ask for more information when that information changes meaning. Friction should be removed at its source, not hidden behind a second convenience wrapper.

No representative frequency study of Jet usage ran here. The ordering below is a task-based priority judgment, not a numerical usage claim.

| Priority | Friction | Beginner impact | Expert impact | Enterprise impact | Owner |
|---|---|---|---|---|---|
| First | Failed fresh baseline and unproved command promises | Cannot start reliably | Cannot trust current behavior | Cannot qualify a candidate | #2919 |
| First | First-hour entry, dependency, streaming, and generic-helper failures | The obvious route fails | Workarounds contaminate examples | Onboarding cost and undocumented support | #2900, #2920–#2923 |
| First | Scope-free or inconsistent verdicts | Misreads success | Rechecks with several commands | Evidence cannot be audited | #2389, #2505 |
| High | Fix text names the wrong tool or a nonworking edit | No actionable next step | Manual source investigation | Repair is not reproducible | #2906 |
| High | Hidden copy, materialization, or schedule change | Learns a misleading model | Unexpected cost or ordering | Resource and latency promises fail | #2898, #2899, #2922, #2923 |
| High | Source/editor/runtime identity drift | Sees contradictory answers | Debugging wastes time | Cannot attach evidence to a release | #2506, #2902; #2926 learning cases |
| High | Release wording overstates current status; foreign commands need explicit capability classes | Chooses an unavailable path if the class is misunderstood | Plans against the wrong boundary | Procurement/release misunderstanding | #2927 owns the confirmed release contradiction and claim census; Ada/Pascal is a consistent classification case |
| Next | Expert facts exist but are hard to discover | Too many concepts at once | Repeated manual inspection | Audit trail assembled by hand | #2505, #2899, #2926 |

The best combined outcome is progressive detail over one mechanism. It removes common ceremony without removing the expert contract. When an operation cannot be made frictionless, explain the information it genuinely needs. Do not make the user pay because internal layers disagree.

## Generalize every finding

| Finding | Defect shape | Predicted other instances | Structural correction | Card disposition |
|---|---|---|---|---|
| A passing outer command hides unproved work | Silence where a verdict is owed | Check, test, prove, nested tool failures | One scoped typed status and explicit unknowns | Reuse #2389, #2505 |
| An explanation or value outlives its source | Temporal provenance omitted | Hover, fixes, debugger, replay, build graph | Revision-bound records and publication checks | Reuse #2506; new #2926 assessment |
| Common route fails while special route works | First-hour path not in the denominator | Scaffold, dependencies, streams, generic helpers, layout facts | Full task sequence plus ordinary-spelling probes | Reuse #2900, #2920, #2921, #2923 |
| Teaching rewards output rather than a mental model | Assessment oracle misses comprehension | Loops, copies, time, FFI, errors | Explain/predict/modify/derive census and transfer evidence | New [#2926](index.md#card-2926) |
| Versioning wording claims an achieved release | Policy or presence treated as current evidence | Version banners, importer labels, target support | Current claim census with evidence-backed labels; consistent binder-only paths remain classification cases, not defects | New [#2927](index.md#card-2927) |
| Error recovery names an implementation detail | Diagnostic names the wrong thing | Tier, compiler, platform, package failures | Same user operation, stable error class, concrete next action | Reuse #2906 |

The report intentionally does not close any implementation, learning, visual, or release gate. It supplies the design, direct answers, and missing proof obligations.

## Machine census contract (#2926)

`scripts/agent/learning-census.mjs` derives the assessment denominator from the
owner-controlled language, Core, CLI, editor, debugger, build, package, and FFI
registries. It joins Core rows to the existing
`scripts/agent/example-core-census.mjs` records and joins fixture names and
deliberate exclusions from `tests/conformance/corpus/` and
`tests/conformance/exclusions.tsv`. It does not maintain a second capability
name list.

```text
scripts/agent/jet-env node scripts/agent/learning-census.mjs --json
```

The machine envelope is `jet-learning-census-v1`. Every derived row carries
four obligations: `explain`, `predict`, `modify`, and `derive`. Each obligation
has a deterministic source-and-revision oracle, a plausible wrong model, a
source identity, a runtime identity, and separate modeled, executed, and human
evidence. The source-revision rule is explicit: an edit invalidates a prior
observation until its dependent projection is regenerated and the task is
re-run.

CLI and ordinary EditorHost paths share task IDs, source identities, oracles,
and revision rules. The census records both paths without AI or Studio. A
missing compiler or editor run remains `unavailable`; an unknown runtime value
is never replaced with a guessed value. Human transfer requires an explicit
observation record with participant, completion, prediction, transfer,
recovery, and latency fields. Modeled friction is not promoted to a defect,
and any observed defect must carry one deduplicated live owner card before it
enters the defect list.
