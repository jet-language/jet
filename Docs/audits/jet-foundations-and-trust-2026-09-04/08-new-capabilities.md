# New capabilities, interfaces, and delivery designs

**Addendum: 2026-09-05. Author: Main/Astra.** This section turns the research into concrete proposals and implementation contracts. It is not another inventory of gaps. The [delivery index](design-and-delivery-index.md) maps all 30 original questions to decisions, cards, dependencies, and observable acceptance.

[Read the visual design chapter](08-new-capabilities.html) · [Return to the master report](index.md).

**Published:** 22 delivery cards, **#2934–#2955**, and six complete owner ballots. No production implementation or fresh compiler execution is claimed. D-COMPILER-PROOF1 is already **ratified A**, not an open recommendation. It requires proof across Jet-controlled stages before 1.0; it does not establish that such proof exists today.

## The slate adds capabilities without adding another language

| Proposal | New contribution | Surface or mechanism | Delivery | Owner choice |
|---|---|---|---|---|
| [N01](#n01) | Implementation-bound compiler assurance | Formal models, checked stage arguments, replay, composition | #2925; #2934–#2941; #2944 | D-COMPILER-PROOF-TOOLS1 on #2934 |
| [N02](#n02) | One inspectable reason behind a claim | Shared derivation references in existing facts and records | #2945 | D-EXPLANATION-RECORD1 |
| [N03](#n03) | Source-linked reasoning views | Ordinary-editor hints, relationship views, complete text access | #2946 | D-EXPLAIN-VIEW1 |
| [N04](#n04) | Learning that exposes and repairs a mental model | Existing `jet learn`, prediction, counterexample, change, transfer | #2926; #2947 | D-LEARN-FEEDBACK1 |
| [N05](#n05) | Controlled semantic experiments | Existing `jet review`, contracts and comparable observations | #2948 | Existing law; no new command |
| [N06](#n06) | A disciplined route from optimizer theory to production | Machine-semantic legality, falsifiers, whole-job cost | #2939; #2951 | Existing optimization law |
| [N07](#n07) | Reusable stale-result rejection | Proposed `Shared.capture` and `try_replace` | #2949 | D-SHARED-REVISION1 |
| [N08](#n08) | Reasoning through foreign boundaries and replacements | Existing binders, overlays, debugging, adoption workflow | #2953 | Existing foreign/adoption law |
| [N09](#n09) | First-party two-implementation comparison | Proposed `core.testing.compare` | #2950 | D-TEST-COMPARE1 |
| [N10](#n10) | A consumed capability/evidence relation and useful test selection | Existing manifests, hardening records, independent oracles | #2942; #2952 | Existing qualification law |
| [N11](#n11) | Falsifiable advancement and self-hosting evaluation | Four controlled studies; eight compiler-shaped workloads | #2954; #2955 | Existing bootstrap owner gate, not a new release gate |
| [N12](#n12) | One candidate whose claims invalidate together | Existing records, CI, release status, truthful support claims | #2927; #2943; #2944 | Existing assurance and handoff law |

The new public choices are ordinary APIs, tool architecture, and presentation or teaching defaults. **No new keyword or sigil is recommended.** The alternatives are fully specified in their ballots. A published ballot is not permission to implement its recommendation.

### What was considered and rejected

| Tempting addition | Decision | Reason and retained capability |
|---|---|---|
| A new proof annotation or `#Proof` language | Reject | Existing facts, `jet prove`, and proof records already own program claims. Compiler assurance needs implementation correspondence, not another spelling. |
| A `#Temporal` marker or runtime “fact” object | Reject | Revision, resource generation, lifetime, and clock time are different relations. Use ordinary state and existing static fact laws. |
| A new comparison annotation or `#Compare` test language | Reject | `#Test` remains the test syntax. An ordinary Core API can express the proposed comparison. |
| A separate `jet compare` command | Reject | `jet review` already owns semantic, authority, and receipt changes. Extend that route rather than duplicate it. |
| An always-running AI tutor or optional AI mode | Reject | The requested experience must work deterministically, offline, and without AI. |
| A mandatory visual studio | Reject | Ordinary CLI and editor paths must retain complete reasoning and learning access. Canvas remains an optional projection. |
| A universal cross-language transpiler | Reject | Binding, explicitly supported conversion, and accepted replacement are distinct contracts. C++ binding is not whole-C++ translation. |
| A new permanent capability registry | Reject | Derive and join the current authorities. Do not create another list that can disagree with the compiler. |
| Self-hosting as an automatic 1.0 gate | Reject | It is a useful workload and trust-boundary experiment, not a correctness certificate. Existing owner approval remains required. |

These are design decisions, not implementation-effort excuses. No safety rule, expert control, required execution mode, or accepted product capability is removed to make the slate easier.

<a id="n01"></a>
## N01 — Prove the implementation, not a nearby algorithm

**Cards:** #2925, #2934–#2941, #2944. **New ballot:** D-COMPILER-PROOF-TOOLS1 on #2934. **Authority:** D-COMPILER-PROOF1=A, I1–I9, the ratified source semantics, and the existing shared-lowering direction.

The formal claim is conditional preservation. For a supported source program, configuration, and stated environment/resource premises, every completed target observation must be allowed by the source model. The observations include results, typed failure, mutation, input consumption, externally visible event order, cleanup, and promised scheduling/progress behavior. A mode may choose an allowed schedule; it may not invent a deadlock or erase a defined failure.

The model must state source bytes and source graph, values and types, stores and ownership, tasks, effects, foreign interactions, compile-time execution, resource premises, and observations. Models of emitted Rust, Cranelift operations, and web output are required where correspondence reaches those representations. A precise but wrong model is still wrong: disagreement with ratified law, executable examples, or the canonical conformance corpus blocks the affected claim.

### The proof chain has named implementation owners

| Stage | Required argument | Card |
|---|---|---|
| Tool construction and replay | Exact proposition, model, checker, implementation, artifact, and candidate binding | #2934 |
| Executable semantics | Complete observation contract and construct/boundary obligation relation | #2935 |
| Source processing | Bytes, lexical structure, parsing, source graph, names, and correct rejection | #2936 |
| Checking | Types, ownership, effects, fact propagation, and diagnostic obligations | #2937 |
| Compile-time work | Evaluation, fuel/resources, effects, generics, and dependency/cache validity | #2938 |
| Lowering and optimization | Each actual shared transformation preserves the allowed observations | #2939 |
| Core and runtime | Jet-owned Prelude behavior satisfies its contracts, not merely the right symbol name | #2940 |
| Execution adapters | AOT, JIT/dev, interpreter, and applicable web marshalling/translation correspondence | #2941 |
| Composition | Every required stage matches the same candidate, with no uncovered Jet-controlled step | #2944 |

Each accepted stage uses one of the three bindings already permitted by D-COMPILER-PROOF1:

1. A proved executable construction or extracted implementation, with extraction and build assumptions named.
2. Direct verification of the real implementation source under a stated language model, with source-to-binary trust named.
3. A sound, implementation-bound result checker and an accepted certificate for the actual covered translation.

An abstract algorithm theorem beside unrelated Rust code fails this contract. A source hash establishes identity, not correspondence. An unverified Rust-to-proof translator is a generator, not a shortcut around the binding requirement.

The proposed replay entry point is `scripts/agent/compiler-proof.mjs`. It is **planned, not implemented**. It consumes existing candidate/record identities and records the claim, model, correspondence method, implementation/checker/certificate, assumptions, and raw replay result. Wrong goals, admitted obligations, undeclared axioms, changed inputs, substituted artifacts, corrupt objects, missing evidence, and exhausted budgets cannot pass.

### The tool choice does not weaken the proof boundary

| Ballot option | Exact choice | Basis and limit |
|---|---|---|
| **A — Lean 4, recommended** | One canonical checker and composition language in the development environment | Aeneas documents a Rust-oriented Lean route. Its supported subset and translation trust do not establish full Jet coverage. |
| B — Rocq | The same complete obligation relation, checked in Rocq | CompCert supplies concrete compiler-proof practice, not correctness of Jet's Rust implementation. |
| C — HOL4 | The same complete obligation relation, checked in HOL4 | CakeML demonstrates checked compiler construction. Its verified binary is not a verified Jet binary. |

The recommendation is a tool-fit judgment, **not a measured Jet proof result**. Aeneas explicitly documents safe-subset limits and ongoing unsafe/concurrency work. No full Jet/Rust/Cranelift proof model was captured here. Those remain required implementation obligations, not exclusions from the gate.

Ordinary Jet installation needs no proof assistant. Root compiler and seam dependencies remain path-only. Prefer universal stage arguments checked when constructing the compiler. Where a translation needs a per-compilation certificate, check that instance. Unknown or rejection uses the already-ratified deterministic proved fallback, or stops as an internal compiler failure when no such path exists. It never accepts unchecked output or blames valid source.

**Remaining boundary:** the model and checking mechanism are named assumptions. rustc/LLVM, Cranelift, browser engines, assemblers, linkers, operating systems, hardware, and foreign contracts remain explicit where not separately checked. This is not an unconditional physical-machine theorem. Independent conformance, platform, domain, performance, and handoff evidence remain necessary. A compiler that rejects everything cannot qualify.

**Acceptance:** replay the complete chain, reject deliberately false or wrongly attached claims, consume every required capability/mode obligation, and compose only exact matching identities. #2925 retains the same-slice comparison of all three binding methods; its bounded experiment is not replaced by the larger implementation cards.

Sources: [Aeneas](https://github.com/AeneasVerif/aeneas), [CompCert's exact preservation boundary](https://compcert.org/man/manual001.html), [CakeML's verified components](https://cakeml.org/), and [the original trust analysis](04-trust.md#q20).

<a id="n02"></a>
## N02 — Every explanation should point to the reason that established it

**Card:** #2945. **Ballot:** D-EXPLANATION-RECORD1. This is a shared data contract, not a new evaluator, fact plane, or proof language.

A useful explanation answers four questions: **What is claimed? What established it? Which premises does it use? Does it still apply?** The producer owns the answer. A renderer can shorten it but cannot invent a simpler semantic story.

The proposed relation extends existing typed evidence with references to:

- subject and claim;
- producer and method: static derivation, mathematical proof, recorded execution, sampled agreement, or external assumption;
- rule and premise identities;
- source origin and source/run/target identity;
- assumptions and current disposition;
- an optional observed event, counterexample, or proof artifact.

Facts remain enums under D-FACTMODEL1. Ownership remains a prover under D-FACT-OWN1, not plane algebra. Runtime revisions are ordinary state, not a new static-fact mechanism. Proof records, semantic operations, and recorded events keep their existing owners.

**Recommended A:** retain compact reason links with existing evidence. Intern references, share nodes, retain the configured evidence slice, and expand on demand. **B** recomputes reasons from exact historical inputs when requested; those inputs or tools may be unavailable. **C** retains complete derivation snapshots for requested captures; privacy and retention still limit what can be included.

The irreducible cost is retaining information about the past. Deduplication and bounded capture reduce storage, but cannot make retained reasons occupy no space. The design removes full-tree copies per consumer and effectful re-execution on hover.

A changed premise or source/run identity invalidates dependent claims. Stale, expired, redacted, unavailable, unsupported, and budget-exhausted are explicit states. Missing runtime values are not guessed. Sampled agreement is not relabeled proof.

**Acceptance:** the same bounds, ownership, mutation, failure, and optimization witnesses expose identical claims and premises through CLI, editor, review, and learner. Wrong-source attachments, false reasons, removed premises, and evidence-class promotion must be rejected.

Prior practice: [Why3 stores transformations and proof attempts, marks changed goals obsolete, and replays them](https://www.why3.org/doc/starting.html). That is a useful mechanism, not a claim that its data model already covers Jet.

<a id="n03"></a>
## N03 — Keep the source readable and make deeper relationships reachable

**Card:** #2946. **Ballot:** D-EXPLAIN-VIEW1. **Scope:** ordinary editors. Canvas's separately ratified proof rail is unchanged.

Recommended **A** shows a concise source-local hint and an expandable, pinnable evidence view. **B** opens a detailed panel by default. **C** leaves source undecorated and exposes details only through a command. All three use N02's records and retain complete text and keyboard paths.

| View | What the reader can determine | Evidence boundary |
|---|---|---|
| Value and change | Observed value, previous value, responsible operation | Named run and frame; never a fabricated current value |
| Ownership and lifetime | Owner, checked read/write window, invalidation boundary | Existing ownership prover and source provenance |
| State transition | Previous state, transition, resulting state, rejected path | Static contract or identified execution, labeled separately |
| Event timeline | Input, effect, callback, task, and cleanup order | Recorded events and explicit missing/redacted spans |
| Dependency and change impact | Which claim or artifact depends on the changed subject | Source-bound semantic identities, not text similarity |
| Optimization reason | Accepted rule, rejected alternative, premises, copy/cost evidence | Legality and measured profit remain separate |
| Counterexample pair | The smallest retained case that distinguishes two claims | An actual valid witness, not persuasive prose |

The initial view must be bounded. Large data has a route to complete evidence rather than silent truncation. Keyboard navigation, stable focus, labels independent of color, reduced motion, narrow/wide layouts, and text alternatives are required. Current, stale, redacted, unavailable, running, failed, and empty states are distinct.

An ordinary editor must not run user effects on hover. No AI dependency, optional AI mode, or mandatory Studio is introduced. Experts can pin details, inspect premises, follow source maps, and obtain raw records.

**Acceptance:** launch the real editor and CLI surfaces and exercise the complete state matrix. The implementation owner supplies exact prerequisites, launch command, fixture or printed URL, ordered interactions, expected states, and Pass/Bounce cues. Owner acceptance judges hierarchy and interaction, not compiler correctness. A selected screenshot is insufficient.

Comparison: [VS Code's debugger](https://code.visualstudio.com/docs/debugtest/debugging) exposes variables, frames, hover values, and watch expressions for a selected execution. Jet should keep that explicit observation boundary while connecting static reasons and change identity.

<a id="n04"></a>
## N04 — Teach a rule by making the learner use it on a different program

**Cards:** #2926 and #2947. **Ballot:** D-LEARN-FEEDBACK1. D-LEARN1 already owns `jet learn` and its dependency-aware watch session. #2926 owns the complete curriculum/task census; #2947 implements the learning interaction.

The current source contains three repair exercises. That does not implement the proposed explain/predict/modify/derive cycle.

Recommended **A** uses this order:

1. Give the minimum prerequisite definitions and a concrete program.
2. Ask for a prediction, or an honest **I do not know**.
3. Reveal identified execution evidence and a focused explanation or valid distinguishing counterexample.
4. Change one relevant condition and ask what changes.
5. Present a structurally different transfer task that requires the same principle.

Unknown reveals help immediately. There is no imposed delay or penalty. Experts may skip a checkpoint, but the record then says **unmeasured**, not completed. Ordinary editing never interrupts the user with quizzes.

**B** shows the worked example before prediction. That measures post-instruction understanding, not the initial model. **C** lets the learner choose reveal/prediction order and records what actually happened. The recommendation is a design judgment; no comparative human learning result was measured in this campaign.

Every task has a capability identity, prerequisites, prompt, plausible wrong model, source/input identity, deterministic oracle, reveal policy, controlled edit, transfer task, and accessibility alternative. The curriculum derives its denominator from canonical language/Core/tool inventories rather than a second manually maintained product list.

Progress binds to curriculum, source, and oracle identities. User edits survive interruption and resume. Changed lessons invalidate affected completion evidence. Copying a solution or repeating a canned explanation cannot satisfy all four task kinds. Unavailable execution or unknown proof cannot complete a task.

**Acceptance:** real loop, state/lifetime, effect-boundary, and foreign-boundary interactions work through CLI and ordinary editor paths, with watch/one-shot/JSON behavior, interruption, stale evidence, keyboard access, and an owner-reviewed runbook. Human transfer claims require actual participant evidence and consent; simulated RLI5 is not that evidence.

Useful prior mechanism: [Python Tutor's stepwise values and frames](https://pythontutor.com/), including asking students to predict a step. Jet does **not** adopt that site's AI features or infer learning effectiveness from its usage claims.

<a id="n05"></a>
## N05 — Compare a controlled change without pretending to prove equivalence

**Card:** #2948. **Existing owner:** #2114, semantic/authority/receipt review. **No new command or ballot.**

The current `jet review` already reports semantic operations, authority changes, and gained/lost/changed claims. The new work connects those categories to preserved contracts, changed observations, and explicit unknown relationships.

```sh
jet review base.jet head.jet \
  --base-receipt base.jetproof --receipt head.jetproof
```

This is an existing interface. The richer behavior below is planned:

| Result | Required meaning |
|---|---|
| Preserved checked contract | A current argument covers the exact before/after subjects and stated observations. |
| Changed static contract | Types, authority, effects, failure, ownership, or other checked terms changed. |
| First differing observation | Comparable identified inputs produced a reproducible differing result or event. |
| Sampled agreement | The executed observations agree under the declared relation; universal equivalence remains unproved. |
| Unknown relationship | Inputs, environment, identities, alignment, or evidence are insufficient. |

Align subjects with compiler semantic identities and recorded rename/source operations. Ambiguous or unmatched subjects remain explicit. Do not guess from text similarity. Require compatible declared inputs, environment, numerical/scheduling premises, and observation contracts before comparing traces. Do not execute arbitrary effects to fill a missing record.

**Acceptance:** distinguish a rename, authority widening without annotation text change, lost proof, changed failure/input-consumption order, and same-output-but-unproved change. Reproduce the first differing observation and link it to the responsible source operation. The same record supports repair review and the learner's change-one-thing task.

<a id="n06"></a>
## N06 — An optimization must win twice: first on meaning, then on cost

**Cards:** #2939 and #2951. Existing #2892/#2895/#2899 and performance owners retain their production scopes. Finalized D-ACCEL1, D-PLACE1, and D-LOOPREAD1 are not reopened or edited.

| Executable experiment family | Exact question | Required falsifier |
|---|---|---|
| Arithmetic and floating rewrites | Under which machine-number premises is the rewrite valid? | Overflow, precision, NaN, signed-zero, or defined failure changes outside the allowed contract |
| Range/alias facts, fusion, vectorization | Do facts justify the transformed access and order? | Aliased mutation, boundary error, early-exit, input consumption, or failure-order change |
| Specialization and compile-time staging | Does moving work preserve effects, resources, and dependency validity? | Stale specialization, changed effect, wrong generic instance, or invalid compile-time assumption |
| Layout and copies | Does representation change preserve identity and avoid unnecessary semantic copies? | Different meaning, hidden clone, lifetime change, or cost shifted outside the measurement |
| Conditional parallel scheduling | Is parallel work legal and profitable for this actual job? | Ordering/cancellation change, startup/collection cost hidden, or a slower selected plan |

Each candidate has a stated domain, preconditions, observation relation, proof/checker boundary, counterexample, unchanged valid baseline, and same-job cost record. Use the current shared operation route, not an experimental second compiler. Equality search is a candidate generator; it does not make machine arithmetic obey real-number identities.

Legality and profitability are independent. A legal slow transform loses the cost comparison. An invalid fast transform is ineligible. Inspect output must expose the selected plan, rejected alternatives, copies, scheduling, assumptions, and measured cost where available.

Measure compile latency, cold/warm run behavior, memory, and applicable modes against the same required peers and workload. A research result is not release qualification. No averaged-away loss, omitted cost, easier peer, or AOT-only result qualifies.

**Acceptance:** all five families execute controls distinguishing invalid, legal-but-slow, and legal-measured-improvement cases. Every retained production candidate has exact implementation ownership. New semantic changes return to the owner. Unknown or bounded evidence remains labeled as such.

The retained JavaScript floating-point witness establishes only its bounded model result. It is not fresh Jet optimizer evidence; see [ordinary correctness](05-correctness.md#model-results) and [optimization research](04-trust.md#q21).

<a id="n07"></a>
## N07 — Reject stale publication without asking every writer to remember a counter

**Card:** #2949. **Ballot:** D-SHARED-REVISION1. This is the concrete API proposal arising from the temporal-identity hypothesis.

Memory safety alone does not prevent a delayed result from overwriting newer valid state. Value equality alone misses **A → B → A**. The recommended interface extends existing `Shared<T>` rather than adding another shared-state mechanism.

```jet
// Proposed API; not implemented or executed in this campaign.
struct State { value: String }
state :: shared State{value: "old"}
seen :: state.capture()
state.value = "new"
print(state.try_replace(seen, State{value: "late"}))
// Required: false; state.value stays "new".
```

| Interface | Contract |
|---|---|
| `capture()` | Atomically capture the value and opaque owner/revision ticket; return `SharedSnapshot<T,T>`. |
| `capture(pure_projection)` | Capture an ordinary owned copy of projected `U` from the same atomic read; return `SharedSnapshot<T,U>`. |
| `try_replace(snapshot, value)` | Atomically check owner and committed revision, then publish a new `T`; return `Bool !SharedRevisionError`. |
| `false` | The ticket is stale; neither value nor revision changes. |
| Typed failure | Wrong owner or exhausted generation; no wrap, owner confusion, or unchecked publication. |

Snapshots obey existing semantic-copy eligibility. Projection avoids copying unrelated data and permits an eligible result from a larger non-copyable value. The ticket is opaque and non-decodable as ordinary data. It grants no mutation authority. The method is named **capture**, not **snapshot**, so `Rollback.snapshot` keeps its existing transaction meaning.

Every committed write advances the revision, including equal-value writes. Two publications against one revision have at most one winner. Reusing the accepted ticket is stale. No effectful computation is retried automatically.

### Transactions must compose with the revision contract

- Ordinary Shared statement writes use the same revision mechanism.
- A successful transaction publishes one committed revision; abort restores value and revision.
- A transaction-local capture remembers its local write sequence.
- Only captures of the final committed state can become current. A capture made before a later local write remains stale.
- Abort invalidates transaction-local tickets. Nested rollback follows the same rule.

Reuse the existing shared lock and activate revision bookkeeping when captures are requested. Ordinary writes must not copy another value merely to maintain a stamp. An independent historical value still requires retained data; pure projection removes unrelated retention, not that information requirement.

**Alternatives:** B adds a distinct replacement-only `Versioned<T>` API, with the same safety and transaction contract. C keeps a complete explicit revision/owner recipe in Shared transactions. A avoids both a second interface and forgotten writer updates.

**Keep the distinctions:** `Pool<T>/Id<T>` owns removed/reused resource identity. Ownership owns valid access. A Shared revision owns committed-value freshness. Clock time owns temporal measurement and domain scheduling. They can share evidence relations without becoming one misleading type or a new runtime fact plane.

**Acceptance:** deterministic stale/ABA, competing winner, wrong-owner, repeated-ticket, exhaustion, commit/abort/nested-transaction, and intermediate-capture controls. One semantic implementation must serve all applicable execution modes. The previous stale-publication model did not exercise this rejecting API.

Prior practice: [Java's AtomicStampedReference](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/util/concurrent/atomic/AtomicStampedReference.html) atomically reads and compares a reference/stamp pair. Its caller-managed stamp policy is precisely the discipline the recommended Shared integration should own.

<a id="n08"></a>
## N08 — A foreign call should reveal its complete boundary contract

**Card:** #2953. Existing binder, adoption, export, and build-host cards retain their implementation ownership. **No new C++ ruling or universal converter is proposed.** D-FFI-CPP1=A is archived ratified law on #501, not an unresolved question.

The same source-linked boundary record must cover ABI, width/alignment/layout, ownership and close rules, borrow duration, encoding, nullability, error/exception mapping, callbacks, task/thread crossing, target availability, and copy/conversion costs. Bind it to the header, overlay, generator, foreign implementation, toolchain, and target identities.

For C++, preserve the ratified clang-based binder, cached C shims, opaque owned handles with consuming cleanup, ordinary methods, caught exceptions, on-demand templates, labeled overloads, named operators, and checked overlays. A generated namespace is not evidence that these obligations work.

| Family | Required classification and reasoning route |
|---|---|
| C/C++ | Existing binding, native export, mixed debugging, and build-host paths; C/C++ source import remains unavailable. |
| Python, Java, C#, JavaScript/TypeScript, Go | Existing adapter plus explicitly supported narrow conversion subset; unsupported constructs remain explicit. |
| COM/VBA, Ada, Pascal | Their stated adapter/binder contracts; do not call emitted stubs semantic translation. |
| CMake, Gradle, Bazel, MSBuild | Existing host invocation binds to the same source/tool/target contract. |

The adoption sequence stays **bind in place → convert an explicit supported subset → compare a replacement → accept the replacement**. Foreign source remains authoritative until acceptance. A mismatch, unmodeled effect, unsupported construct, or unknown equivalence does not silently replace it. Sampled agreement is not a theorem about native semantics.

**Acceptance:** consume a supported executable path or exact non-support classification for every named family. Exercise real cleanup, exceptions/errors, callbacks/thread crossing, layout/encoding, stale artifacts, and copy cost. Change a header, overlay, toolchain, or target and verify invalidation. Ordinary inspection and debugging must expose the same assumptions.

This is foreign adoption, not current Jet-version migration support. See [the foreign-contract analysis](06-interop.md#foreign-contract), [autocxx](https://google.github.io/autocxx/), and [pybind11's class binding model](https://pybind11.readthedocs.io/en/stable/classes.html).

<a id="n09"></a>
## N09 — Make differential comparison an ordinary test, with explicit limits

**Card:** #2950. **Ballot:** D-TEST-COMPARE1. `#Test` remains the only test syntax under D-TESTKIT1.

```jet
// Proposed library API; not implemented in this campaign.
use core.testing as testing
testing.compare(cases, reference, candidate).assert_equal()
```

Recommended A adds `compare(cases, reference, candidate, relation?)`, returning a comparison record. It reuses existing corpora, fixtures, test verdicts, reduction, and record identities. B exposes only a `jet test` comparison mode, requiring entry-point adapters for direct function work. C retains ordinary loops and repeated accounting.

The default relation is existing typed equality. An explicit relation may permit numerical or scheduling variation; the receipt must name it and cannot claim stronger equivalence. Typed failure, mutation, input consumption, event order, and cleanup are included only when the declared observation contract includes them.

Pure callables require no environment setup. Effectful scenarios need explicit isolated recorded fixtures and reset boundaries. Do not silently repeat external effects, share contaminated inputs, or capture secrets.

| Outcome | Meaning |
|---|---|
| Matched | Nonempty valid executed cases agree under the declared relation. |
| Mismatch | A reproducible difference exists; retain the first differing observation and a reduced witness. |
| Empty | No valid cases, including all-discarded input; cannot pass. |
| Unsupported or unavailable | A required scenario or side cannot be evaluated; cannot pass. |
| Timeout or cancellation | Comparison is incomplete; cannot pass. |
| Invalid oracle/record contract | A declared oracle check or evidence contract failed; cannot pass. |

The API cannot infer the intended answer or detect every bad reference implementation. Agreement with the same wrong implementation is still only agreement. Independent expected outcomes and oracle controls remain necessary for correctness claims. `assert_equal()` asserts the declared comparison, not universal program correctness.

Record case/input identity, seed where applicable, source/tool/target identity, relation, raw observations, and reduced reproducer. Reduction must preserve the mismatch and relation. Use the existing gauntlet, not this comparison call, for performance timing.

**Acceptance:** compare real pure and explicitly isolated stateful/foreign scenarios, replay a reduced failure, reject false-green empty/inconclusive/invalid-contract cases, and preserve the same behavior through all applicable modes. No external dependency or second test runner.

Prior practice: [Hypothesis](https://hypothesis.readthedocs.io/en/latest/quickstart.html) generates inputs and reduces failures inside ordinary tests. The useful transfer is composable generation and replay, not a claim that a passing sample proves all inputs.

<a id="n10"></a>
## N10 — Count every obligation and retain tests for the defects they detect

**Cards:** #2942 and #2952. Existing Core routing, conformance, hardening, property, grammar, mutation, and gauntlet owners remain authoritative.

The central concept is a consumed relation:

`capability → intended contract → actual route → observable oracle → evidence class → exact candidate`

This is a join of authorities, not a new product registry. Derive language constructs, public Core types/members/fields/receivers, CLI/editor/debugger/build/package/FFI surfaces, and applicable modes from their canonical sources. The research's 323 collected identities are not the complete production denominator.

Each row is counted and has an explicit disposition: planned, implemented-unqualified, passed, failed, stale, unavailable, unsupported, or owner-ratified not-applicable. A declaration, command name, test filename, or bound-and-discard result cannot set passed. Shared implementation agreement needs an independent expected outcome; all modes can share the same bug.

The same row identities must feed inspect, hardening status, learner census, and release claims. New untested members and missing modes must become visible gaps rather than disappear from the denominator.

### Test value is distinct detection, not test count

For each retained, consolidated, or removed test, record its obligation, plausible wrong implementation, detected defect shape, measured time/memory/failure latency, and replacement evidence. Mandatory obligations and unique detectors come before cost. Deterministic selection may prefer a cheaper duplicate, but cannot remove the last detector or an applicable mode.

Remove wording-only, wiring-only, mock-echo, and genuinely redundant tests only when retained consumer evidence covers the required behavior. Keep useful historical minimal regressions when their failure shape has no equivalent detector. A selector miss reopens its selection rule and owning obligation.

**Acceptance:** reject zero-valid, all-excluded, discarded-output, broken-oracle, wrong-candidate, and harness-error false greens. Compare retained and omitted detection on the same candidate under the existing closeout process. No new runner, smaller hidden denominator, or weakened hardening quota.

<a id="n11"></a>
## N11 — Test the proposed advance against a result that could refute it

**Cards:** #2954 and #2955. These are executable evaluation contracts, not generic “investigate later” cards or claims of a breakthrough.

| Study | Intervention | Refutation |
|---|---|---|
| One semantic contract | The same meaning drives routes and evidence | A legitimate advertised operation needs an independent semantic rule. |
| Temporal identity | Current revision, generation, lifetime, and publication relations are explicit | A modeled invalidation still permits stale acceptance, or distinct domain meanings are erased. |
| Conditional optimization | Checked legality and actual cost select a plan | Defined observation changes, or hidden whole-job cost reverses the claimed gain. |
| Evidence-backed explanation | One reason supports beginner explanation and expert inspection | The explanation invents meaning or fails an unfamiliar prediction/repair/transfer task. |

Each study needs an executed baseline/intervention pair, exact identities, raw results, explicit disposition, and independent replay. Cover the eight mission areas through existing workload and gauntlet owners. Do not substitute easier jobs or promote bounded models into runtime evidence, simulated readers into human studies, or agreement into proof.

Report **rejected**, **supported within scope**, or **unresolved** for each hypothesis. Compare named prior work before claiming novelty. Every discovered defect gets exact deduplicated ownership. A useful synthesis need not be advertised as a scientific breakthrough.

### Self-hosting is an evaluation workload before it is a port

#2955 evaluates eight compiler-shaped families: source scanning/parsing; symbol/type relations; diagnostics/source maps; dependency invalidation; generic/compile-time work; concurrent scheduling; foreign interaction; and multi-package builds.

Each needs expected results/failures, ownership/effect boundaries, invalidation rules, resource limits, supported modes, a same-job oracle, cost measurements, and reasoning tasks. Report implementation gaps, repair/readability friction, measured cost, and retained-host trust separately.

The evidence informs existing #217/#218 and their port chain. It does not depend on prior approval of the port, start that port, or make self-hosting a new 1.0 gate. Equal bootstrap binaries are not semantic correctness. Human readability claims need human evidence; machine measurements remain agent-owned.

<a id="n12"></a>
## N12 — A release claim must belong to the candidate that earned it

**Cards:** #2927, #2943, #2944. **Preserved authority:** D-COMPILER-PROOF1=A and D-HARDENING-GATE1. Frozen #2420 is reference-only.

Compose existing identities for source commit/content, relevant configuration, compiler and generated artifacts, target tools, proof/model/checker inputs, consumed test/oracle results, performance, platform qualification, and owner acceptance. The existing dashboard and release path consume that one relation. Do not create another release registry.

Changing a dependency invalidates its dependent green claims. Historical receipts remain immutable and visibly historical. Missing, stale, unknown, unavailable, failed, zero-valid, or incorrectly excluded evidence cannot combine into ready.

#2927 corrects current prerelease/support claims while preserving ratified decision history. Future external-contract policy is not evidence that 1.0 has shipped. Ada/Pascal binder classification and unavailable C/C++ source conversion remain honest. No current Jet-version migration machinery is added.

The stable path remains:

1. Truthful current status and a complete capability/contract relation.
2. Complete semantic foundations and the real implementation/evidence loop.
3. The selected full Jet-controlled compiler assurance contract.
4. Understandable end-to-end work in every mission area.
5. One frozen candidate with the required proof, conformance, performance, platform, and visual evidence.
6. The existing sustained handoff window and owner-approved 1.0.

The existing handoff law retains 14 clean days, 10,000,000 valid differential cases, at least 100 valid mutations per eligible callable, and the required fresh-context challenge schedule. These are requirements, not results achieved here. No source fix, new current compiler build, broad suite, or release qualification was performed in this campaign.

**Acceptance:** substitute source, generator, model, checker, target tool, binary, and release-claim identity in isolated records and observe the exact fail-closed reason. A correct refusal proves the qualification tool's behavior, not that the present candidate is qualified.

## The remaining choices are explicit and bounded

| Ballot | Card | Recommended A | B | C |
|---|---|---|---|---|
| D-COMPILER-PROOF-TOOLS1 | #2934 | Lean 4 canonical checker | Rocq | HOL4 |
| D-EXPLANATION-RECORD1 | #2945 | Compact shared reason links | Recompute on request | Complete requested derivation snapshots |
| D-EXPLAIN-VIEW1 | #2946 | Short hints and expandable/pinnable evidence | Always-open detailed panel | Command-only evidence |
| D-LEARN-FEEDBACK1 | #2947 | Predict before reveal, with immediate unknown/help | Worked example first | Learner-directed order |
| D-SHARED-REVISION1 | #2949 | Checked capture/publication on Shared | Separate Versioned type | Explicit revision recipe |
| D-TEST-COMPARE1 | #2950 | Core Testing comparison API | Test-runner-only mode | Ordinary test loops |

Each is a complete short ballot within one mechanism, with three worked alternatives, current and primary-source comparisons, recommendation, reasons against alternatives, and addressed or justified losses. No new syntax or invariant carve-out is proposed. No independent review pass is claimed for these short ballots. The earlier full compiler-policy ballot keeps its actual review history unchanged.

## Generalize every finding

| Finding | Defect shape | Predicted unprobed instances | Structural correction | Card |
|---|---|---|---|---|
| A theorem is detached from production | Identity is mistaken for correspondence | Parser helpers, generated code, runtime bodies, proof generators | Implementation-bound stage arguments and replay | #2934–#2941; #2944 |
| Tools explain different reasons | A consumer re-decides producer meaning | Hovers, diagnostics, review, learner, optimizer reports | Shared derivation references; consumers only project | #2945; #2946; #2948 |
| A solved exercise is called understanding | Completion is mistaken for transfer | Every capability family and first-hour path | Prediction, controlled change, unfamiliar transfer, honest evidence | #2926; #2947; #2954 |
| A delayed result overwrites newer state | Valid memory is mistaken for current identity | Editor work, search results, caches, background game work | Owner-bound committed revisions and atomic conditional publication | #2949 |
| A fast rewrite changes the program | Profit is mistaken for legality | Floating rewrites, fusion, layout, specialization, parallel scheduling | Machine-semantic law, rejecting controls, separate whole-job cost | #2939; #2951 |
| Two implementations share the same wrong answer | Agreement is mistaken for correctness | Shared Prelude, generated adapters, foreign replacements | Explicit relation and independent oracle controls | #2950; #2952; #2953 |
| A declaration has no consumed behavior | Registration is mistaken for support | Types, fields, receivers, CLI/editor/build/FFI routes | Complete counted contract/route/oracle/candidate join | #2942 |
| A binding hides its assumptions | Signature is mistaken for a complete boundary | Exceptions, callbacks, close, encoding, widths, target tools | One inspectable foreign contract through accepted replacement | #2953 |
| Green fragments describe different builds | Historical evidence is mistaken for current qualification | Proof, generated artifacts, tools, CI, release copy | Dependency-bound candidate invalidation | #2927; #2943; #2944 |
| A research synthesis is called a breakthrough | A plausible hypothesis is mistaken for replicated advancement | Temporal facts, semantic reuse, optimizer search, teaching visuals | Executable falsifiers, same-job baselines, independent replay | #2954 |
| Self-compilation is called readiness | A workload is mistaken for a correctness certificate | Bootstrap stages, compiler-shaped libraries, retained foreign tools | Bounded evaluation before the existing port decision | #2955; existing #217/#218 |

The [delivery index](design-and-delivery-index.md) is the exact card/dependency/acceptance map. The [machine-readable publication](design-delivery.json) records the authored contracts and ballot payloads. Neither substitutes for future implementation evidence.
