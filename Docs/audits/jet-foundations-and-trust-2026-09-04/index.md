# Jet foundations and trust

**Superseded design report:** [Jet refounding — concrete language, Core, and systems proposals](../jet-refounding-2026-09-05/index.md) is the current recommendation and owner slate. Read its [visual report](../jet-refounding-2026-09-05/index.html) or [decisions and dispositions](../jet-refounding-2026-09-05/06-decisions-and-finding-dispositions.md). This earlier report remains evidence and decision history; its retained findings and adopted rulings are not withdrawn.

## Executive summary

**Jet's strongest next step is to make one behavior contract survive every stage of the developer's work.** Checking establishes facts. Shared operations preserve meaning. Runtime and target adapters implement that meaning. Tools explain the same facts. Qualification records what was actually established for one candidate.

This is the common cause behind the most important findings: duplicated meaning, missing obligations, stale results, hidden copies, incomplete foreign contracts, and explanations that require an expert to fill the gaps. Fix the relationship, not each symptom with another registry or special case.

The owner asked for fundamental advancement first, trust and comprehension next, and stability after that. The recommendation keeps all three priorities. It does not trade away beginner ease, expert control, enterprise auditability, or correctness to reach a date. Implementation effort is not a reason to reject the better combined design.

### What this investigation establishes

| Result | Meaning |
|---|---|
| **30 direct answers** | Every original numbered question has a detailed answer, a master takeaway, evidence limits, and a card/decision disposition in [coverage.json](coverage.json). |
| **7 area reports plus a new-design chapter** | The research answers the original questions; the added chapter specifies twelve concrete capabilities, APIs, surfaces, architectures, and evaluations. |
| **18 packets, 2,717 records** | The integrated factual collection is broad. Record count is provenance, not proof, representative sampling, or a feature-completeness percentage. |
| **323 first-party inventory identities mapped** | The [capability ledger](capability-ledger.json) joins collected surfaces to authored contracts, oracles, reports, and owners. It is not a second production registry or a verified complete denominator. |
| **3 original planning cards plus 22 delivery cards** | #2925–#2927 retain their research/census/claim scopes. #2934–#2955 add explicit implementation contracts, prerequisites, and rejecting acceptance criteria without replacing existing owners. |
| **1 ratified proof policy plus 6 new owner ballots** | D-COMPILER-PROOF1 is ratified A: prove every Jet-controlled stage with explicit foreign-tool assumptions. Six bounded choices cover the proof toolchain, reason records, editor views, learning sequence, Shared revisions, and test comparison. |
| **Retained executable model evidence** | Stale publication, floating-point reassociation, reused entity identity, and clock representation were demonstrated outside Jet. Source, full output, and receipt are linked below. |
| **No fresh Jet runtime proof** | The authorized fresh compiler build failed. This report does not claim current parity, performance, full functionality, or release readiness. |

### The decisions and actions that matter most

1. **Complete one semantic route.** Reuse the ratified shared-lowering and fact laws. A shared representation is necessary, but it does not prove that every execution mode preserves failure, effect order, input consumption, or lifecycle.
2. **Make omissions count.** Derive the advertised capability denominator from canonical registries and actual routes. Join it to a consumed outcome and current evidence. A declared member or a test filename is not a pass.
3. **Pursue a real compiler assurance contract.** Proved algorithms and sound result checkers can coexist. Keep the specification, checker, trusted tools, foreign behavior, and physical environment as explicit boundaries. Independent tests remain necessary.
4. **Make comprehension an observable obligation.** A newcomer should explain, predict, modify, and derive the ordinary path. Use the same compiler facts in CLI, editor, debugger, and optional visual studio. No AI dependency or optional AI mode belongs in the proposed experience.
5. **Encode time and identity where they matter.** Memory safety does not stop a valid old handle from referring to a new entity unless the identity contract prevents it. Fixed-step code does not erase clock representation or overload policy.
6. **Bind broadly, convert narrowly, replace with evidence.** Keep foreign source authoritative until the adopter accepts a replacement. C++ is a supported-boundary problem, not a whole-language transpilation promise.
7. **Use prerelease freedom honestly.** Correct current release claims, preserve historical law, and make justified breaking changes cleanly. Do not build current Jet-version migration machinery. After 1.0, preserve the declared external contracts.

## Read the complete argument by subject

| Area | Direct questions | Main result |
|---|---|---|
| [1. Foundations and language lessons](01-foundations.md) | 1–5 | Transfer a mechanism with its assumptions, not a slogan, syntax fragment, or creator regret. |
| [2. Frontier research and competitive design](02-frontier.md) | 6–8 | Four falsifiable hypotheses connect existing research to Jet's facts, compilation, explanation, and whole-job performance. No breakthrough is claimed. |
| [3. Comprehension, assumptions, and developer experience](03-comprehension.md) | 9–12, 14–18 | Treat knowledge, freshness, and repair as explicit contracts. Measure understanding with tasks rather than polished prose. |
| [4. Proof, testing, optimization, and CI](04-trust.md) | 19–25 | Use a complete capability/obligation relation and distinguish proof, checking, bounded search, tests, and current qualification. |
| [5. Ordinary correctness and games](05-correctness.md) | 26–29 | Exclude modeled invalid states and preserve time/identity. Domain intent and physical correctness still require domain evidence. |
| [6. Whole-program and mixed-language reasoning](06-interop.md) | 13; foreign-adoption additions | Keep one reasoning chain through local code, modules, runtime, build, and foreign boundaries. |
| [7. Stable release and self-hosting](07-readiness.md) | 30; compatibility and bootstrap additions | Foundations first, trust/comprehension next, same-candidate stability last. Self-hosting is an evaluation workload, not an automatic gate. |
| [8. New capabilities, APIs, architecture, and tools](08-new-capabilities.md) | All 30 questions and interview additions | Twelve proposals, six complete owner ballots, and 22 concrete delivery cards; no new syntax or implementation claim. |

[Complete design and delivery index](design-and-delivery-index.md) · [Exact published contracts and ballots](design-delivery.json) · [Publication verification](design-verification.json).

<a id="takeaway-foundations"></a>
## 1. Language history favors explicit boundaries and coherent mechanisms

**Take from the Turing lectures:** disciplined semantic structure, explicit assumptions, compositional reasoning, usable abstractions, observation of real workloads, and skepticism about claims broader than their model. Dijkstra's control-flow argument, Backus's alternative organization of computation, Hoare's proof/engineering perspective, and Lamport/Sifakis's system-modeling work address different problems. They do not reduce to one universal language recipe.

**Take from language postmortems:** coupling matters. Rust's ownership benefits coexist with learning and compiler-feedback costs. C++ compatibility and native reach coexist with accumulated complexity and unchecked boundaries. Java/Python/JavaScript ecosystem decisions are inseparable from deployment, libraries, and historical users. Zig's implementation transitions do not show that ambitious features can ship without a complete semantic design.

| Candidate idea | Take it with | Reject the false shortcut |
|---|---|---|
| Strong types, ownership, and effects | Inference, escape rules, diagnostics, libraries, and lowering | A marker with no preserved relation |
| Formal verification | Precise semantics, theorem, checker, replay, and explicit trusted boundary | A proof file called universal correctness |
| Optimization search | A valid machine-semantic equality theory and profitability evidence | Mathematical identities used as unrestricted machine rewrites |
| Live tools and visual explanations | Compiler-owned facts, revision identity, and deterministic projections | A second interpreter that invents a simpler meaning |
| Foreign generation | ABI, lifetime, error, encoding, source ownership, and target contracts | Namespace presence called safe or complete interoperation |

The silhouette is **lost relationships**, not a missing fashionable keyword. Jet already has many relevant laws. The task is to carry those laws through every user-visible path and count the paths that lack evidence.

[Q1: lectures](01-foundations.md#q1) · [Q2: postmortems](01-foundations.md#q2) · [Q3: transfer dependencies](01-foundations.md#q3) · [Q4: gaps](01-foundations.md#q4) · [Q5: first principles](01-foundations.md#q5)

<a id="takeaway-frontier"></a>
## 2. The research opportunity is a unified contract, not a novelty claim

The frontier report evaluates types/facts, effect and control models, proof/translation validation, equality saturation, specialization, staging, and domain compiler work. It records prerequisites and counterclaims. A paper's result is not a Jet result, and a useful combination is not automatically a scientific breakthrough.

| Hypothesis | Proposed advance | Falsifying result |
|---|---|---|
| One semantic contract drives implementation and evidence | Facts, shared operations, adapters, tests, and explanations agree without re-deciding meaning | A legitimate advertised operation needs an independent semantic rule or cannot be expressed by the contract. |
| Temporal facts connect correctness and freshness | Revision, handle generation, lifetime, and publication checks share a reusable relation | The relation loses domain distinctions, or stale facts remain accepted after modeled invalidation. |
| Conditional optimization preserves expert intent | A checked legality relation and explicit cost evidence choose faster execution without weaker meaning | A transformation changes defined failure, order, precision, identity, or resource behavior. |
| Explanations are evidence-backed projections | A beginner derives a rule and an expert inspects its premises from the same facts | The explanation invents a semantic model or fails an unfamiliar prediction/repair task. |

Measure frontier and fundamental gains on the **same job** in all eight mission areas: scripting/automation, services/data, CLI/software tools, libraries, numerical/compute work, embedded/resource-constrained work, cross-platform/visual applications, and games/interactive simulation. Each row needs intended output, failure behavior, resource/latency limits, supported hosts, and a matched peer implementation.

Jet is behind wherever those obligations have only a registration, partial route, historical result, or missing current candidate. This report creates no new win/parity/loss scoreboard. Existing measurement owners remain responsible for the strict comparison; unrelated wins cannot average away a required loss.

For agent-assisted development, the proposed product remains deterministic and independent of AI. The five useful measures are verdict fidelity, verdict latency, repair actionability, context economy, and repair determinism. They also improve human work.

[Q6: frontier papers](02-frontier.md#q6) · [Q7: breakthrough hypotheses](02-frontier.md#q7) · [Q8: full-mission competition](02-frontier.md#q8)

<a id="takeaway-comprehension"></a>
## 3. Understanding must be tested through a task

A surface teaches itself when a newcomer can explain its meaning, predict an observation, make a small change, and derive a rule for an unfamiliar case. RLI5 detects gaps in that chain. A simulated reader is not a human usability study. Neither fluent prose nor generated help proves the task works.

The strongest red-team targets are assumptions beneath the interface: a registered route must run; checking must cover the selected project; a result must belong to the current revision; a repair must name the right tool; a version banner must not claim readiness; and a mode change must preserve interaction semantics.

| Concrete experience | Proposed behavior | Existing surface/owner |
|---|---|---|
| First hour | One ordered new → run → check → test → fix → explain task, including recovery | First-hour guide, CLI registry, #2900 and onboarding cards |
| Daily verdict | One source-bound status and deterministic repair loop, shared by CLI and editor | `jet check`, `jet fix`, `jet explain`, #2389/#2505/#2506 |
| Learn a rule | Same-program contrast, prediction, modification, unfamiliar transfer task | `jet learn`, #1918; new census #2926 |
| Inspect a decision | Show the fact, rejected alternative, reason, and cost evidence | `jet inspect`, #2899 |
| Explore relationships | Ownership/effect/state/data/call/build lenses anchored to source identity | LSP facts and portable projections, #2902 |
| Use a studio | Optional view of the same deterministic model, with keyboard and CLI paths | Existing visual owners; no AI or mandatory studio dependency |

Friction should track task frequency and consequence. Common work deserves the shortest sound path. Rare expert controls can require information when it changes behavior, but must remain discoverable and auditable. This priority ordering is reasoned from task structure, not a fabricated usage survey.

The capability ledger maps every collected first-party inventory identity. A mechanically complete production denominator and a real all-surface learner-task campaign are still owed. New #2926 owns that missing shape while consuming existing syntax, example, first-hour, and diagnostic censuses.

[Q9: red team](03-comprehension.md#q9) · [Q10: explicit assumptions](03-comprehension.md#q10) · [Q11: implicit assumptions](03-comprehension.md#q11) · [Q12: RLI5 inventory](03-comprehension.md#q12) · [Q14: experience](03-comprehension.md#q14) · [Q15: teaching designs](03-comprehension.md#q15) · [Q16: transferable principles](03-comprehension.md#q16) · [Q17: visual/LSP design](03-comprehension.md#q17) · [Q18: friction](03-comprehension.md#q18)

<a id="takeaway-trust"></a>
## 4. Trust needs a complete denominator and an honest proof boundary

The required relation is:

`capability → intended contract → implementation route → observable oracle → evidence class → candidate identity`

Every advertised form, receiver method, field, type, CLI route, editor feature, and applicable execution mode must appear or have a counted approved non-executable reason. A bound-and-discard result is not an oracle. A common implementation can make every mode agree on the same wrong answer, so independent oracles still matter.

| Evidence | What it establishes | What it cannot establish alone |
|---|---|---|
| Source inventory | A declaration, route, contract, or test exists in captured source | Current execution, total support, correctness |
| Model experiment | A premise fails or holds in the bounded model | Jet behavior, real environment, novelty |
| Test/differential result | Selected observations agree for executed inputs and candidate | Every program, intended business rule, all environments |
| Bounded validation | A relation holds within the checked bound/model | An unrestricted theorem |
| Checked mathematical proof | A stated property follows under explicit rules and assumptions | That the rules capture unstated intent or external premises always hold |
| Release qualification | The required portfolio passes for the identified candidate | Future changes or unqualified targets |

**Recommendation:** require checked preservation across every Jet-controlled stage before 1.0, including compile-time evaluation and Core/Prelude implementations. Bind the proof to extracted/in-logic code, directly verified source, or an accepted translation checked by a sound, artifact-bound checker. An abstract algorithm theorem plus unrelated Rust code is insufficient. A hash establishes identity, not correctness.

The theorem ends at explicit models of Jet-controlled output. Release still trusts rustc/LLVM and linking; development still trusts Cranelift; web still trusts the browser engine. An interpreter still has an implementation/build chain. Unless those foreign suffixes are separately checked, this is not an end-to-end physical-machine proof.

| Owner choice | Required formal boundary | Important remaining gap |
|---|---|---|
| **All Jet stages — recommended** | Source processing, compile-time work, shared operations, optimization, adapters, and Jet-owned Core/runtime behavior | Foreign tools and model assumptions; unmeasured artifact-bound proof obligation gates 1.0 |
| Source processing | Checking, compile-time work, and initial translation | Later Jet optimization/execution generally unproved |
| Back half | Valid shared operations through optimization and execution | Source processing generally unproved |
| Reference interpreter | One proved execution reference and its covered runtime | Other modes are compared, not thereby proved |
| Executable formal model | Precise executable rules | Production correspondence unproved |
| Existing evidence only | No additional required formal gate | General compiler preservation unproved |

Every option retains tests, platform/domain qualification, performance, and the ratified handoff window. The denominator remains the ratified modes and capabilities. No silent mode removal, unchecked fallback, new resource-exhaustion exemption, or valid-source blame can satisfy the gate. No option proves arbitrary application intent or all environments.

For optimization, separate legality from profit. For testing, purchase distinct detection rather than test count. For harnesses, retain corpus, exact run identity, oracle, raw outcomes, result states, and reducer. For CI, qualify one candidate rather than assembling unrelated green fragments.

[Q19: functionality relation](04-trust.md#q19) · [Q20: assurance](04-trust.md#q20) · [Q21: optimizations](04-trust.md#q21) · [Q22: efficient suite](04-trust.md#q22) · [Q23: peer assurance](04-trust.md#q23) · [Q24: harnesses](04-trust.md#q24) · [Q25: CI/CD](04-trust.md#q25)

<a id="takeaway-correctness"></a>
## 5. Safe programs still need identity, time, and domain intent

Jet can raise software quality when the ordinary safe path excludes a modeled defect class. Memory/type/ownership safety, exhaustive failure handling, domain quantities, valid state transitions, and lifecycle ownership move work from repeated human discipline into checked contracts. The benefit requires sound implementation and upheld foreign assumptions. No measured deployed Jet defect-rate reduction was established here.

A Jet game should exclude safe-path pointer/lifetime hazards of an equivalent unchecked C++ program. The fair opponent is strong modern C++ and engine practice, not deliberately careless code. Correct collision, physics, update order, resource capacity, and gameplay still need domain contracts and experiments.

| Retained model result | Consequence |
|---|---|
| Read → edit → publish attaches an old fact, which the model flags stale | A rejecting revision guard is proposed, not exercised. Publish-before-read orders are invalid, not stale witnesses. |
| Binary64 reassociation produces 1 versus 0 | A real-number identity is not an unrestricted machine-number optimization law. |
| A reused slot accepts an old handle without generation checking | Storage address/index is not persistent entity identity. |
| Nominal one-second binary64 schedules produce 60/60/59 steps | Clock representation matters; accumulated time sums are not exactly equal. |
| Exact 720-tick schedules produce 60/60/60 steps with zero remainder | Equal exact-time partitions satisfy this bounded count relation; real clocks and engines remain untested. |

Separate semantics from representation, simulation from rendering, legality from profitability, and concise presentation from full evidence before declaring a tradeoff. Some limits remain physical or informational: finite capacity cannot accept an unbounded backlog, and replay cannot undo an irreversible external effect.

[Q26: defect taxonomy](05-correctness.md#q26) · [Q27: game scenarios](05-correctness.md#q27) · [Q28: trade space](05-correctness.md#q28) · [Q29: prevention by layer](05-correctness.md#q29) · [Model source](../../../tools/agent-eval/foundations-trust/research-demonstrators.mjs) · [Full output](demonstrator-results.json) · [Receipt](demonstrator-receipt.json)

<a id="takeaway-interop"></a>
## 6. Foreign code belongs inside the reasoning workflow

The report traces expression, function, module, program, build, execution-mode, foreign, domain, and deployment reasoning. Each step says what information is preserved and which assumption enters. Tools must not erase boundaries; they should make them inspectable.

Existing Jet surfaces include unified FFI ownership/error/layout conversion, binder descriptors, checked overlays, native library exports, mixed-repository debugging, `jet cc`/`jet c++`, and CMake/Gradle/Bazel/MSBuild adapters. These are source/documentation observations here, not fresh execution proof.

Swift, CXX, autocxx, Java FFM/jextract, .NET source-generated marshalling, and Kotlin/Native show useful pieces. Their limits matter: containers may copy, pointer extent cannot be inferred from an address, widths differ by platform, callbacks have lifetime/thread/failure rules, and signature matching does not prove native semantics.

The existing adoption choice is sound: bind in place, convert explicit scalar/straight-line subsets, and replace modules under differential evidence. Python, Java, C#, TypeScript, JavaScript, and Go have narrow importer rows. Ada/Pascal commands and the map consistently describe binder stubs, not semantic translations. This is a capability-classification boundary, not a second observed contradiction. C/C++ source import is deliberately unavailable. Foreign source remains authoritative until replacement is accepted.

This is not current Jet-version migration support. No such machinery is proposed or required here.

[Q13: every-level reasoning](06-interop.md#q13) · [Boundary contract](06-interop.md#foreign-contract) · [Peer mechanisms](06-interop.md#peer-interop) · [Foreign conversion](06-interop.md#foreign-conversion) · [Portable workflow](06-interop.md#portable-tools)

<a id="takeaway-readiness"></a>
## 7. A release must describe the candidate that actually passed

The fresh build failed with exit 101 and 161 errors reported in `jet-comptime`. No new current Jet executable was qualified. Historical results and the JavaScript models remain useful, but cannot fill that gap.

The owner says Jet is prerelease. The versioning reference says 1.0 shipped and imposes post-1.0 migration obligations. New #2927 owns correcting current-state claims while preserving historical ratified policy. A string is not release evidence.

The stable path is:

**Current truth → semantic foundations → compiler assurance → comprehension/tools → complete domain work → frozen candidate qualification → sustained handoff → owner-approved 1.0.**

The ratified handoff requirement remains 14 clean days, 10,000,000 valid differential cases, at least 100 valid mutations per eligible callable, and eight fresh-context lanes in four waves of two, with current identity and counted exclusions. These are requirements, not results achieved here. #2420 remains owner-frozen and was not altered.

Self-hosting can stress compiler-shaped workloads and improve dogfooding. It does not prove compiler correctness or readiness. Evaluate the existing portfolio under #217 before deciding whether to port; do not add a new self-hosting 1.0 gate now.

[Q30: dependency plan](07-readiness.md#q30) · [Build boundary](07-readiness.md#current-evidence) · [Release truth](07-readiness.md#release-truth) · [Self-hosting](07-readiness.md#self-hosting) · [Owner gates](07-readiness.md#owner-gates)

<a id="deliverables"></a>
## Deliverables and provenance

- [Master Markdown](index.md), seven area reports, [30-question coverage](coverage.json), and [capability/obligation ledger](capability-ledger.json).
- [Linked HTML master](../jet-foundations-and-trust-2026-09-04.html) and its linked area pages. HTML tells the same story; it is not a separate technical authority.
- [Model source](../../../tools/agent-eval/foundations-trust/research-demonstrators.mjs), [full output](demonstrator-results.json), and [execution receipt](demonstrator-receipt.json). The receipt distinguishes the actual command from descriptive handoff metadata inside stdout.
- Cards #2925, #2926, #2927 retain their original planning scopes. D-COMPILER-PROOF1 is now ratified A. The original [research publication record](tower-publication.json) remains historical evidence of the full, non-draft, process-4 ballot and its sixteen authored fields.
- The [new-capabilities chapter](08-new-capabilities.md), [delivery index](design-and-delivery-index.md), and [publication snapshot](design-delivery.json) add #2934–#2955 and six owner ballots. The index links every original question and interview addition to concrete delivery and acceptance.

Each area supplies primary-source links, repository locators, reasoning, limits, and a final “generalize every finding” table. The table maps a finding to its mechanism, predicted sibling instances, structural correction, and owner. Fix the shape, not only the illustrated input.

### Evidence limits

- No current-tree Jet execution, parity proof, optimization benchmark, complete domain qualification, or release readiness is claimed.
- Collection is not a representative survey of programmers, languages, defect prevalence, or feature frequency. Missing lecture bodies and replication limits are named in the areas.
- The 323 inventory identities are collected records, with separate original observations retained. They are not the canonical production denominator or independent proof units.
- Source-reference repairs identify locators. They do not reconcile conflicting observations or prove behavior.
- Models are bounded sequential JavaScript demonstrations. They omit real concurrency, persistence, real clocks, production allocators, physics, Jet optimization, and foreign execution.
- Simulated RLI5 is not human usability research. Ballot challenge and rendered HTML design review have separate, bounded roles.
- No cyber work, production fixes, staging, commits, project gates, or current Jet-version migration machinery were performed by this authorship assignment.

<a id="authorship"></a>
### Authorship and supporting roles

AstraSynthesis authored the thinking, recommendations, reports, teaching/architecture designs, substantive card/ballot copy, and HTML story brief. Astra created the initial three cards and sixteen reuse logs. Main reviewed the card and ballot copy, refined the binding criteria and classification framing, then personally published the full ballot and verified its stored fields. This is Astra-model authorship/publication from both agents, as the owner permits.

Main/Astra authored the 2026-09-05 design addendum, all 22 new delivery cards, all six short ballots, and the complete question/dependency index. Read-only scouts supplied current code and archived-decision facts. The short ballots passed Tower's applicable checks; no separate full-ballot review is claimed for them.

Luna supplied factual packets and bounded source/probe work. Main owns orchestration, supporting execution, mechanical checks, and rendered verification. Sol implements presentation through OMP only. Fable is limited to one important-ballot challenge and the authorized frontend pass. Presentation review does not certify technical correctness.

<a id="decisions"></a>
## Decisions and issue disposition

**D-COMPILER-PROOF1, ratified A on 2026-09-05:** require checked preservation for every advertised Jet-controlled compilation/execution path before 1.0. The full, non-draft, process-4 ballot preserves all six historical alternatives, the complete contract, and design-away analysis. The [research publication record](tower-publication.json) records publication; the [design snapshot](design-delivery.json) records the later ratified state.

Methods may mix proved algorithms and sound result checkers. Tests and qualification remain necessary. Ratification establishes the required proof boundary, not its implementation or feasibility. The six new ballots make tool, API, and presentation choices explicit; no unratified option, compiler port, new syntax, or invariant exception is silently adopted.

The [new-design chapter](08-new-capabilities.md) now gives each retained proposal a concrete implementation or executable-evaluation contract. The [delivery index](design-and-delivery-index.md) lists all 22 cards, six ballots, dependencies, and rejecting criteria. Existing rulings are reused, not recast as new owner choices.

**Preserved law:** D-TIER-ONEIR1, D-FACT laws, D-JPROOF1, D-PROVE-SOLVER1, D-PROVE-LENS1, D-LEARN1, D-TEACH-FOREIGN1, D-ADOPT-TIER1, D-MIGRATE-SRC1, and D-HARDENING-GATE1 remain their own records. Finalized D-PLACE1, D-ACCEL1, and D-LOOPREAD1 received no writes. D-REL1–D-REL5 historical policy was not amended.

| New card | Deliverable | Disposition |
|---|---|---|
| <a id="card-2925"></a>**#2925** | Mechanized semantic contract and proof-boundary work | Planning; D-COMPILER-PROOF1=A is ratified. #2934–#2941 and #2944 supply the concrete proof chain. No compiler proof or implementation closure claimed. |
| <a id="card-2926"></a>**#2926** | Explain/predict/modify/derive census across first-party Jet | Planning; consumes existing censuses. No full human study claimed. |
| <a id="card-2927"></a>**#2927** | Current release and capability truth | Bug/planning for the confirmed versioning contradiction and claim census. Ada/Pascal is a consistent classification case, not another bug. No source fix performed. |

<a id="interop-cards"></a>
### Reused owner directory

“Reused” means an existing owner or evidence record, not current acceptance. Sixteen cards received report links through the CLI. #2420 refused a log because it is owner-frozen; no bypass was attempted. No implementation criterion or card phase changed.

| Existing cards | Responsibility | Disposition |
|---|---|---|
| <a id="card-211"></a>#211; <a id="card-806"></a>#806 | CI change/nightly inventories and qualification | Existing owner reference |
| <a id="card-217"></a>#217 | Bootstrap dogfood/readiness/sign-off | Research log; evaluation only, no port authorized |
| <a id="card-507"></a>#507; <a id="card-989"></a>#989; <a id="card-990"></a>#990 | COM/VBA, Ada, Pascal adapters | Existing binder owners |
| <a id="card-822"></a>#822; <a id="card-825"></a>#825 | Fixed-step/replay and playable game qualification | Research logs with model/domain limits |
| <a id="card-873"></a>#873; <a id="card-984"></a>#984 | Existing visual/debugging surfaces referenced by the capability map | Existing owner references, not current visual passes |
| <a id="card-1034"></a>#1034; <a id="card-1035"></a>#1035; <a id="card-1036"></a>#1036; <a id="card-1037"></a>#1037 | Beginner onboarding | Existing owners consumed by #2926 |
| <a id="card-1120"></a>#1120; <a id="card-1121"></a>#1121; <a id="card-1122"></a>#1122; <a id="card-1123"></a>#1123; <a id="card-1124"></a>#1124; <a id="card-1125"></a>#1125 | Unified FFI and conformance | #1120 research log; remaining existing references |
| <a id="card-1127"></a>#1127; <a id="card-1131"></a>#1131; <a id="card-2644"></a>#2644 | Proof artifacts, lenses, producer identity | Existing owners; not a whole-compiler theorem |
| <a id="card-1156"></a>#1156; <a id="card-1346"></a>#1346 | Source conversion and adoption map | #1156 research log; historical laws preserved |
| <a id="card-1344"></a>#1344; <a id="card-1347"></a>#1347 | Hermetic C/C++ driver and foreign build hosts | Existing owners |
| <a id="card-1905"></a>#1905 | Actual AOT test execution and generator evidence | Historical evidence reference |
| <a id="card-1918"></a>#1918 | Deterministic in-toolchain learning | Research log; no AI dependency |
| <a id="card-2285"></a>#2285; <a id="card-2286"></a>#2286 | Core routing totality and value-consuming conformance | Existing canonical census owners |
| <a id="card-2334"></a>#2334; <a id="card-2335"></a>#2335; <a id="card-2336"></a>#2336; <a id="card-2337"></a>#2337; <a id="card-2338"></a>#2338; <a id="card-2339"></a>#2339; <a id="card-2340"></a>#2340; <a id="card-2341"></a>#2341; <a id="card-2342"></a>#2342; <a id="card-2343"></a>#2343 | Existing hardening rig, denominator, runner, oracles, triage, dashboard, challenge, properties, grammar, mutation | #2335/#2339 research logs; no new qualification run |
| <a id="card-2389"></a>#2389; <a id="card-2505"></a>#2505; <a id="card-2506"></a>#2506 | Project checking, verdict loop, source-bound records | Existing owners, no replacement subsystem |
| <a id="card-2420"></a>#2420 | Sustained professional-handoff qualification | Owner-frozen; reference only; log refused |
| <a id="card-2482"></a>#2482 | Cold-start workflow evidence | Existing owner reference |
| <a id="card-2858"></a>#2858; <a id="card-2859"></a>#2859 | Same-workload gauntlet and compiler-speed gates | #2858 research log; no new benchmark |
| <a id="card-2888"></a>#2888; <a id="card-2898"></a>#2898 | One-lowering architecture and full tier/construct census | #2898 research log; D-TIER-ONEIR1 unchanged |
| <a id="card-2886"></a>#2886; <a id="card-2890"></a>#2890; <a id="card-2891"></a>#2891; <a id="card-2895"></a>#2895; <a id="card-2899"></a>#2899 | Loop, copy, numeric, vector, and decision-evidence owners | #2899 research log; protected decisions unchanged |
| <a id="card-2900"></a>#2900; <a id="card-2902"></a>#2902; <a id="card-2903"></a>#2903; <a id="card-2906"></a>#2906 | First hour, syntax projections, executable examples, actionable diagnostics | Research logs; #2926 consumes these censuses |
| <a id="card-2919"></a>#2919 | Integrated runtime, suite, and bug-fix proof | Research log records failed fresh build; no green claim |
| <a id="card-2920"></a>#2920; <a id="card-2921"></a>#2921; <a id="card-2922"></a>#2922; <a id="card-2923"></a>#2923 | Already-filed layout, diagnostic, parallel-cost, and stream-interaction findings | Reused; no duplicate cards or closures |

## Every original question has a direct answer

The [coverage file](coverage.json) preserves exact original wording, evidence, limits, actions, and master links. This table is the reading map.

| Question | Detailed answer | Master takeaway |
|---|---|---|
| 1. Turing lectures: take, avoid, lessons | [Q1](01-foundations.md#q1) | [Contextual mechanisms](#takeaway-foundations) |
| 2. Language postmortems | [Q2](01-foundations.md#q2) | [Causes and dependencies](#takeaway-foundations) |
| 3. Cherry-pick with required surroundings | [Q3](01-foundations.md#q3) | [Transfer the complete contract](#takeaway-foundations) |
| 4. Silhouettes and gaps | [Q4](01-foundations.md#q4) | [Lost relationships](#takeaway-foundations) |
| 5. Missing first-principles shapes | [Q5](01-foundations.md#q5) | [One behavior contract](#takeaway-foundations) |
| 6. Cutting-edge papers | [Q6](02-frontier.md#q6) | [Promising mechanisms and limits](#takeaway-frontier) |
| 7. Possible breakthrough | [Q7](02-frontier.md#q7) | [Falsifiable hypotheses, no novelty claim](#takeaway-frontier) |
| 8. Win advances and fundamentals | [Q8](02-frontier.md#q8) | [Same complete job](#takeaway-frontier) |
| 9. Non-cyber red team | [Q9](03-comprehension.md#q9) | [Attack the hidden premise](#takeaway-comprehension) |
| 10. Explicit assumptions | [Q10](03-comprehension.md#q10) | [Evidence boundaries](#takeaway-comprehension) |
| 11. Implicit assumptions | [Q11](03-comprehension.md#q11) | [Freshness, identity, time, knowledge](#takeaway-comprehension) |
| 12. RLI5 across first-party Jet | [Q12](03-comprehension.md#q12) | [Task census, not prose polish](#takeaway-comprehension) |
| 13. Reasoning at every scale and FFI | [Q13](06-interop.md#q13) | [One chain across boundaries](#takeaway-interop) |
| 14. Next-generation development experience | [Q14](03-comprehension.md#q14) | [One deterministic verdict loop](#takeaway-comprehension) |
| 15. Creative teachability | [Q15](03-comprehension.md#q15) | [Prediction, contrast, transfer](#takeaway-comprehension) |
| 16. Learn programming principles | [Q16](03-comprehension.md#q16) | [Derive an unfamiliar case](#takeaway-comprehension) |
| 17. Visual and LSP teaching | [Q17](03-comprehension.md#q17) | [Compiler-owned relationship lenses](#takeaway-comprehension) |
| 18. Friction and rough edges | [Q18](03-comprehension.md#q18) | [Frequency and recovery severity](#takeaway-comprehension) |
| 19. Complete functionality | [Q19](04-trust.md#q19) | [Capability/contract/oracle join](#takeaway-trust) |
| 20. Verifiable trust without absolute overclaim | [Q20](04-trust.md#q20) | [Strong proof, explicit scope](#takeaway-trust) |
| 21. Aggressive optimization research | [Q21](04-trust.md#q21) | [Legality before profitability](#takeaway-trust) |
| 22. Effective efficient suite | [Q22](04-trust.md#q22) | [Distinct detection, not test count](#takeaway-trust) |
| 23. Peer correctness methods | [Q23](04-trust.md#q23) | [Complementary evidence classes](#takeaway-trust) |
| 24. Peer harnesses | [Q24](04-trust.md#q24) | [Concrete runners and oracles](#takeaway-trust) |
| 25. CI/CD | [Q25](04-trust.md#q25) | [One qualified candidate](#takeaway-trust) |
| 26. Ordinary bug prevention | [Q26](05-correctness.md#q26) | [Exclude modeled invalid states](#takeaway-correctness) |
| 27. Game versus C++ | [Q27](05-correctness.md#q27) | [Memory safety plus domain contracts](#takeaway-correctness) |
| 28. Trade-space boundaries | [Q28](05-correctness.md#q28) | [Separate coupled decisions](#takeaway-correctness) |
| 29. Creative prevention by language/compiler | [Q29](05-correctness.md#q29) | [Preserve facts through every layer](#takeaway-correctness) |
| 30. Clear stable-release plan | [Q30](07-readiness.md#q30) | [Foundations, trust, then qualification](#takeaway-readiness) |

## Generalize every finding

| Underlying shape | Cross-area instances | Structural correction | Disposition |
|---|---|---|---|
| Meaning is decided more than once | Tiers, optimization, editor explanations | One checked semantic contract and qualified adapters | #2888/#2898/#2899; new #2925 |
| Presence is mistaken for evidence | API registration, test names, bindings, banners | Capability/contract/oracle/identity join | #2285/#2286/#2335; new #2927 |
| A fact outlives its premise | Editor results, entities, callbacks, proof caches | Identity, lifetime, invalidation relation | #2506/#822; new #2925/#2926 |
| A law lacks its domain premise | Floating rewrites, clocks, units, physics | State and preserve required premises | #2891/#2895/#825; new #2925 |
| The surface assumes an expert | First hour, repair, foreign contracts, proof ballots | Explain/predict/modify/derive on the real task | #2900/#2902/#2903/#2906; new #2926 |
| Evidence is widened beyond its boundary | Model to runtime, tests to theorem, rig to readiness | Exact evidence class/scope and candidate identity | #2339/#2420/#2919; new #2925/#2927 |
