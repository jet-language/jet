# Design and delivery index

**Published 2026-09-05 by Main/Astra.** [Master report](index.md) · [New capabilities and designs](08-new-capabilities.md) · [Exact contracts and ballot payloads](design-delivery.json).

This index closes the planning gap between the research and implementation. It maps all 30 original questions and the interview additions to concrete outcomes, owner decisions, delivery owners, dependencies, and rejecting acceptance criteria. Tower owns current state and execution order. This is a publication snapshot, not another planner.

## The publication is complete; implementation is not claimed

| Published result | Exact scope |
|---|---|
| Twelve proposals | N01–N12 cover proof architecture, records, editor views, learning, controlled review, optimizer experiments, Shared state, foreign reasoning, test APIs, test economics, research evaluation, and candidate qualification. |
| Twenty-two delivery cards | #2934–#2955; every card has owned paths, implementation steps, dependencies, observable criteria, and a focused future proof. No card is closed by this publication. |
| Six owner ballots | Complete three-way choices, examples, recommendation, consequences, design-away analysis, and primary comparisons. All six were accepted by Tower's short-ballot checks. |
| One existing proof ruling | D-COMPILER-PROOF1 is ratified A. It requires checked preservation across Jet-controlled stages before 1.0. The research does not supply that proof. |
| Preserved existing owners | Censuses, Core routing, shared lowering, hardening, platform and performance workloads, foreign binders, and sustained qualification are reused rather than recarded. |

**Evidence boundary:** no production implementation, fresh compiler build, project-wide test suite, current Jet runtime/performance qualification, or human learning study was performed for this design publication. The reported build failure remains ground truth. Planned paths and commands in the cards name future deliverables, not existing executables.

## The six decisions do not reopen the language

| Ballot | Card | Recommended A | What the owner decides |
|---|---|---|---|
| D-COMPILER-PROOF-TOOLS1 | #2934 | Lean 4 | One proof construction/replay toolchain; Rocq and HOL4 remain explicit alternatives. This does not weaken D-COMPILER-PROOF1 or put a proof assistant in ordinary Jet installations. |
| D-EXPLANATION-RECORD1 | #2945 | Compact shared reason links | Retained derivation links versus recomputation or complete snapshots. This extends existing records and facts; it does not create another evaluator. |
| D-EXPLAIN-VIEW1 | #2946 | Short hints with expandable and pinnable evidence | Ordinary-editor presentation and default density; no mandatory Studio and no AI mode. |
| D-LEARN-FEEDBACK1 | #2947 | Predict before reveal, with immediate unknown/help | Learning sequence; worked-example-first and learner-directed order remain complete alternatives. |
| D-SHARED-REVISION1 | #2949 | Shared.capture and conditional publication | An integrated Shared API versus a separate Versioned type or an explicit ordinary recipe. No temporal marker or new keyword. |
| D-TEST-COMPARE1 | #2950 | core.testing.compare | A library API versus runner-only support or ordinary loops; #Test remains the only test syntax. |

These are bounded short ballots, not claims of another independent full-ballot challenge. Their authored surface examples and all alternatives are retained in [design-delivery.json](design-delivery.json). No unratified choice may be implemented. If the owner chooses a different option, amend its dependent implementation contract before coding; do not preserve competing forms.

## Dependency order follows real prerequisites

The order below is one topological reading of the published graph, not a requirement to serialize independent work. The proof tool choice precedes the formal model. Parsing, checking, compile-time work, lowering, runtime, and adapters lead to composition. Source-bound records support views, learning, review, and foreign reasoning. Qualification consumes the complete obligation relation. The actual owner gates and existing-card dependencies remain in Tower.

| Card | Concrete delivery | Published phase | New owner gate | Direct prerequisites |
|---|---|---|---|---|
| [#2934](#card-2934) | Compiler proof: pinned checker toolchain and implementation-bound replay | planning | D-COMPILER-PROOF-TOOLS1 | None; owner gate applies if named |
| [#2935](#card-2935) | Compiler proof: executable Jet semantics and complete observation contract | ready | Existing law | #2934 |
| [#2936](#card-2936) | Compiler proof: source bytes, parsing, names, and rejection preservation | ready | Existing law | #2935 |
| [#2937](#card-2937) | Compiler proof: checked types, ownership, effects, and facts | ready | Existing law | #2935, #2936 |
| [#2938](#card-2938) | Compiler proof: compile-time evaluation and dependency validity | ready | Existing law | #2935, #2937 |
| [#2939](#card-2939) | Compiler proof: shared lowering and optimization preserve observations | ready | Existing law | #2935, #2937, #2938, #2919, #2898 |
| [#2940](#card-2940) | Compiler proof: Jet-owned Core and runtime behavior | ready | Existing law | #2935, #2937, #2898 |
| [#2941](#card-2941) | Compiler proof: AOT, JIT, interpreter, and web adapter correspondence | ready | Existing law | #2939, #2940, #2919 |
| [#2942](#card-2942) | Capability evidence: consume one complete contract-to-candidate relation | ready | Existing law | #2285, #2286, #2335, #2337, #2341, #2342, #2506, #2898, #2902, #2903 |
| [#2943](#card-2943) | Candidate qualification: invalidate stale proof, test, artifact, and release claims together | ready | Existing law | #2942, #2506, #2339, #2644, #2917, #2927 |
| [#2944](#card-2944) | Compiler proof: composed candidate theorem and fail-closed release evidence | ready | Existing law | #2936, #2937, #2938, #2939, #2940, #2941, #2943 |
| [#2945](#card-2945) | Explanations: one checked derivation record across facts, execution, and optimization | planning | D-EXPLANATION-RECORD1 | #2505, #2506, #2919 |
| [#2946](#card-2946) | Reasoning views: source-linked values, relationships, and proof limits | planning | D-EXPLAIN-VIEW1 | #2945, #2505, #2506 |
| [#2947](#card-2947) | Jet Learn: prediction, counterexample, change, and transfer interactions | planning | D-LEARN-FEEDBACK1 | #2945, #2926 |
| [#2948](#card-2948) | Semantic review: controlled changes and first differing observations | ready | Existing law | #2945, #2114, #2506 |
| [#2949](#card-2949) | Shared state: atomic snapshots and stale-result rejection | planning | D-SHARED-REVISION1 | None; owner gate applies if named |
| [#2950](#card-2950) | Core Testing: reproducible two-implementation comparison with honest outcomes | planning | D-TEST-COMPARE1 | None; owner gate applies if named |
| [#2951](#card-2951) | Optimization experiments: checked laws, rejected counterexamples, and whole-job cost | ready | Existing law | #2950, #2945, #2892, #2895, #2899, #2919 |
| [#2952](#card-2952) | Test economics: preserve unique defect detection while removing redundant cost | ready | Existing law | #2335, #2336, #2337, #2338, #2339, #2341, #2342, #2343 |
| [#2953](#card-2953) | Foreign reasoning: one boundary contract from binding through accepted replacement | ready | Existing law | #2945, #2948, #2950, #507, #989, #990, #1120, #1121, #1122, #1123, #1124, #1125, #1156, #1344, #1346, #1347, #2927 |
| [#2954](#card-2954) | Frontier hypotheses: falsifiable same-job experiments and independent replication | ready | Existing law | #2945, #2947, #2949, #2951, #2953, #2858, #2859, #822, #825, #2926 |
| [#2955](#card-2955) | Self-hosting evaluation: compiler-shaped workloads before a port decision | ready | Existing law | #2919, #2926 |

“Ready” is a planning state, not completed implementation or permission to ignore blockedBy. #2934 is a child of #2925. The other relationships above are explicit prerequisites. Existing #2420 and the finalized records below remain reference-only.

## Every original question has a decision and delivery disposition

The linked area report contains the substantive research answer and primary sources. The middle columns state the resulting design, rather than treating a broad umbrella card as an implementation. The last column names a result that would reject a plausible false completion.

| Original question and research answer | Design disposition | Concrete delivery and reused owners | Minimum rejecting acceptance |
|---|---|---|---|
| [Q1. Research Turing Award speeches: what to take, what to avoid, lessons learned.](01-foundations.md#q1) | Transfer the lecture mechanisms with their semantic and engineering premises, not isolated slogans. Proposals: N01, N02, N06, N11. | New: #2934, #2935, #2945, #2951, #2954. Existing: #2925. | Implementation-bound proof and falsifiable same-job experiments reject the specific false transfers named in the foundation report. |
| [Q2. Research postmortems of other programming languages.](01-foundations.md#q2) | Use language postmortems to reject accumulated parallel mechanisms and unearned compatibility obligations. Proposals: N11, N12. | New: #2954, #2955, #2943. Existing: #217, #2927. | The evaluation names retained-host and ecosystem boundaries; current prerelease truth does not claim historical policy is shipped capability. |
| [Q3. What should we cherry-pick, and what must be taken with surrounding dependencies to avoid false positives?](01-foundations.md#q3) | Adopt each useful mechanism with its checking, runtime, tooling, and evidence dependencies. Proposals: N01, N05, N08, N09. | New: #2944, #2948, #2953, #2950. Existing: #1120, #1156. | A controlled replacement or transformation retains the declared observations and exposes every unproved or foreign assumption. |
| [Q4. What silhouettes, shadows, and gaps emerge across the research?](01-foundations.md#q4) | Treat lost relationships between claims, routes, identities, and observations as the common missing shape. Proposals: N02, N07, N10. | New: #2945, #2949, #2942. Existing: #2898. | Missing routes, invalidated premises, and stale publications become counted non-passes rather than disconnected green claims. |
| [Q5. From first principles, what shapes might Jet be missing?](01-foundations.md#q5) | Deepen the existing semantic, identity, and evidence mechanisms instead of adding fashionable syntax. Proposals: N01, N02, N07, N10. | New: #2935, #2945, #2949, #2942. Existing: #2505, #2506. | One authority owns each meaning; views and adapters consume it, and every new public choice is explicit in a ballot. |
| [Q6. Which cutting-edge computer-science papers could help Jet get ahead?](02-frontier.md#q6) | Turn frontier papers into bounded hypotheses about proofs, effects, staging, temporal identity, and optimization. Proposals: N01, N06, N07, N11. | New: #2934, #2938, #2951, #2949, #2954. Existing: #2895. | Each proposed advance has an executable intervention, named premises, falsifier, raw result, and independent replay. |
| [Q7. Can we create a breakthrough in computer science?](02-frontier.md#q7) | Aim for a useful replicated advance without claiming novelty from a plausible synthesis. Proposals: N01, N11. | New: #2944, #2954, #2955. Existing: #2925. | The four studies distinguish rejected, supported-with-scope, and unresolved results against named prior work. |
| [Q8. How can Jet beat other languages to new advances and at fundamentals simultaneously?](02-frontier.md#q8) | Measure new mechanisms and fundamental costs on the same complete jobs in all eight mission areas. Proposals: N06, N08, N11, N12. | New: #2951, #2953, #2954, #2943. Existing: #2858, #2859. | Required peers, modes, output/failure, compile/run latency, and memory remain visible per cell; no easier substitute or averaged loss. |
| [Q9. Aggressive non-cyber red-team audit: where does Jet fall short and have gaps?](03-comprehension.md#q9) | Attack false confidence in checking scope, routes, freshness, repair, support claims, and qualification. Proposals: N02, N10, N12. | New: #2945, #2942, #2952, #2943. Existing: #2389, #2900, #2906. | Wrong-source, missing-mode, discarded-output, invalid-oracle, and stale-candidate controls reject a green result. |
| [Q10. Which explicit assumptions might be wrong?](03-comprehension.md#q10) | Make explicit assumptions executable or label them as assumptions with a precise boundary. Proposals: N01, N05, N10, N12. | New: #2935, #2948, #2942, #2943. Existing: #2925. | Changed or unsupported premises invalidate the claim; agreement, model validity, and implementation correspondence stay distinct. |
| [Q11. Which implicit assumptions were made and should be reconsidered?](03-comprehension.md#q11) | Expose hidden assumptions about identities, shared oracles, scope, and historical evidence. Proposals: N02, N05, N10, N12. | New: #2945, #2948, #2952, #2943. Existing: #2506. | A shared wrong oracle or mismatched historical identity cannot be promoted into current correctness. |
| [Q12. RLI5 audit of everything across all of Jet.](03-comprehension.md#q12) | Derive a complete first-party learner-task denominator and assess explain, predict, modify, and derive separately. Proposals: N03, N04, N10, N11. | New: #2946, #2947, #2942, #2954. Existing: #2926, #2900, #2902, #2903, #2906. | Every capability family has tasks or a reasoned non-applicable disposition; simulated friction is not a human study. |
| [Q13. How do we maximize reasoning about code at every level, in every domain and codebase, including FFI and other languages?](06-interop.md#q13) | Carry the same reasoning account from expressions through modules, builds, execution, and foreign boundaries. Proposals: N02, N03, N05, N08. | New: #2945, #2946, #2948, #2953. Existing: #2505, #2506, #2528, #1344, #1347. | Ordinary tools reveal source identity, ownership/effects, dependency and foreign assumptions, and comparable replacement evidence. |
| [Q14. How does Jet become a readable, comprehensible, learnable next-generation development experience rather than just a language?](03-comprehension.md#q14) | Make source, verdict, execution, explanation, repair, and recheck one deterministic development experience. Proposals: N02, N03, N04, N05, N08. | New: #2945, #2946, #2947, #2948, #2953. Existing: #2389, #2505, #2506. | The same facts and actions work through CLI and ordinary editor without AI or mandatory Studio. |
| [Q15. What creative solutions can improve teachability?](03-comprehension.md#q15) | Teach through prediction, a valid counterexample, one controlled change, and unfamiliar transfer. Proposals: N03, N04, N05. | New: #2946, #2947, #2948. Existing: #2926. | A copied solution or canned explanation cannot satisfy all learning tasks; unknown and unavailable remain honest. |
| [Q16. How can a newcomer learn programming principles, not merely Jet?](03-comprehension.md#q16) | Teach programming principles through structurally different tasks, not only Jet syntax repair. Proposals: N04, N11. | New: #2947, #2954. Existing: #2926. | Learners must derive the same principle in a different program; real transfer claims require real participant evidence. |
| [Q17. What visuals or LSP features can teach concepts and features and show relationships?](03-comprehension.md#q17) | Project values, ownership, state, effects, dependencies, and counterexamples from source-bound records. Proposals: N02, N03, N04, N05. | New: #2945, #2946, #2947, #2948. Existing: #2505, #2506. | Keyboard/text/no-color/reduced-motion and narrow/wide paths preserve complete evidence across current, stale, and unavailable states. |
| [Q18. Where are friction, rough edges, and other shortfalls?](03-comprehension.md#q18) | Put the shortest sound path on frequent work and keep rare expert controls discoverable and auditable. Proposals: N03, N04, N10, N11. | New: #2946, #2947, #2942, #2954, #2955. Existing: #2900, #2906, #2926. | First-hour and recurring reasoning tasks record concrete failure, repair, latency, and understanding limits rather than an invented usage survey. |
| [Q19. How do we ensure foundations, features, tooling, syntax, core libraries, and APIs are fully functional and work as intended?](04-trust.md#q19) | Join every advertised capability to its intended contract, actual route, consumed oracle, and candidate. Proposals: N01, N08, N09, N10, N12. | New: #2944, #2953, #2950, #2942, #2943. Existing: #2285, #2286, #2335, #2898, #2919. | Every applicable row is counted; a declaration, test name, missing mode, or discarded result cannot count as functional evidence. |
| [Q20. How do we ensure no missing or broken functionality ever and verifiably correct behavior always, to build trust?](04-trust.md#q20) | Replace an impossible unconditional guarantee with complete conditional proof and independent qualification obligations. Proposals: N01, N09, N10, N12. | New: #2935, #2944, #2950, #2952, #2943. Existing: #2925, #2339, #2420. | All Jet-controlled stages need bound checked arguments; model, foreign, resource, and application-intent boundaries remain explicit. |
| [Q21. Can we aggressively research and test theoretical compiler optimizations?](04-trust.md#q21) | Test theoretical optimizations under exact machine semantics before measuring whole-job profit. Proposals: N01, N05, N06, N09. | New: #2939, #2948, #2951, #2950. Existing: #2892, #2895, #2899, #2922, #2923. | Five experiment families reject illegal rewrites and legal slow plans separately, preserving failure, input, identity, and order. |
| [Q22. How do we obtain an effective, efficient test suite rather than a bloated one?](04-trust.md#q22) | Keep distinct behavioral detection and mandatory obligations, then minimize redundant cost. Proposals: N09, N10. | New: #2950, #2942, #2952. Existing: #2337, #2341, #2342, #2343. | Every removal has replacement evidence; unique historical/seeded detectors and required modes survive measured selection. |
| [Q23. How do other languages test and establish correctness?](04-trust.md#q23) | Use proof, conformance, differential generation, regressions, platform fleets, and workloads for their different claims. Proposals: N01, N09, N10, N12. | New: #2934, #2950, #2952, #2943. Existing: #2335, #2337, #2858. | Each evidence class retains its scope and candidate identity; no one class silently substitutes for another. |
| [Q24. What do their test harnesses look like?](04-trust.md#q24) | Make corpus, oracle, raw result, reducer, identity, and invalid outcomes first-class in existing harnesses. Proposals: N01, N09, N10, N12. | New: #2934, #2950, #2952, #2943. Existing: #2335, #2336, #2506. | Replay rejects stale inputs, wrong artifacts, zero-valid runs, invalid oracle contracts, and harness failure reported as success. |
| [Q25. What about their CI/CD pipelines?](04-trust.md#q25) | Qualify one candidate through existing CI rather than assemble unrelated green fragments. Proposals: N01, N10, N12. | New: #2944, #2942, #2952, #2943. Existing: #211, #806, #2339, #2420. | Source/tool/model/checker/artifact changes invalidate dependent claims while historical receipts and ratified thresholds remain intact. |
| [Q26. How can Jet prevent common software glitches, bugs, and errors at language level so adoption raises overall software quality?](05-correctness.md#q26) | Prevent modeled defect classes at the earliest sound layer and name the remaining domain assumptions. Proposals: N01, N07, N08, N09, N11. | New: #2937, #2949, #2953, #2950, #2954. Existing: #822, #825. | Ownership, stale publication, foreign lifecycle, and declared comparison controls reject the modeled bug without claiming all domain intent. |
| [Q27. If a game were written in Jet instead of C++, which common glitches should become impossible or less frequent?](05-correctness.md#q27) | Separate memory/lifetime safety, entity identity, scheduling, clocks, capacity, and game rules. Proposals: N07, N08, N09, N11. | New: #2949, #2953, #2950, #2954. Existing: #820, #822, #825. | Real game witnesses test stale/reused identity, time and overload rules, callbacks, and replay; bounded JavaScript models are not Jet proof. |
| [Q28. What is our trade space, and where can its bounds be pushed?](05-correctness.md#q28) | Design away representational and interface tradeoffs before accepting physical or informational limits. Proposals: N01, N03, N06, N07, N08, N11. | New: #2944, #2946, #2951, #2949, #2953, #2955. Existing: #2858. | Each remaining ballot loss has a concrete reason; safety, expert control, performance cells, and complete behavior are not traded away. |
| [Q29. How creatively can language shape, code generation, and optimization address ordinary bugs and errors?](05-correctness.md#q29) | Use existing types/facts, shared lowering, explicit state identity, and evidence-backed tools to prevent repeated mistakes. Proposals: N01, N02, N05, N06, N07, N09. | New: #2937, #2939, #2945, #2948, #2951, #2949, #2950. Existing: #2898, #2899. | The fix closes the defect shape across applicable modes and consumers instead of adding a spelling-specific special case. |
| [Q30. How do we get to stable release through a clear plan rather than minutiae?](07-readiness.md#q30) | Deliver foundations and complete evidence first, then one frozen candidate under the existing sustained handoff and owner gate. Proposals: N01, N10, N11, N12. | New: #2944, #2942, #2943, #2954, #2955. Existing: #217, #2339, #2420, #2858, #2859, #2919, #2926, #2927. | No stale green, missing required proof, averaged loss, synthetic handoff evidence, automatic self-hosting gate, or false 1.0 claim can qualify. |

## Interview additions remain acceptance constraints

| Owner requirement | Exact disposition |
|---|---|
| Beginner magic, full expert control, enterprise auditability; friction tracks frequency; effort is not a rejection reason. | [01-foundations.md#q5](01-foundations.md#q5); [03-comprehension.md#q18](03-comprehension.md#q18); [05-correctness.md#q28](05-correctness.md#q28) |
| Proof remains valuable alongside tests; evaluate self-hosting before gating. | [04-trust.md#q20](04-trust.md#q20); [07-readiness.md#self-hosting](07-readiness.md#self-hosting) |
| Near-native common-language work, C/C++ adoption, reviewable foreign conversion. | [06-interop.md#foreign-contract](06-interop.md#foreign-contract); [06-interop.md#foreign-conversion](06-interop.md#foreign-conversion) |
| No AI dependency or optional AI mode in the proposed experience. | [03-comprehension.md#q14](03-comprehension.md#q14); [06-interop.md#portable-tools](06-interop.md#portable-tools) |
| Prerelease breaking changes allowed; no current Jet-version migration support; preserve external contracts after 1.0. | [07-readiness.md#release-truth](07-readiness.md#release-truth) |
| Fresh Astra authorship; factual Luna collection; Main orchestration; permitted challenge and HTML mechanics only. | [index.md#authorship](index.md#authorship) |
| Reports, useful retained demonstrators, deduplicated cards, decisions, verified linked HTML; no production fixes. | [index.md#deliverables](index.md#deliverables); [index.md#decisions](index.md#decisions); [05-correctness.md#model-results](05-correctness.md#model-results) |
| Dedicated report section covering new features, APIs, surfaces, syntax, architecture, tools, ideas, and concepts, with related concrete cards and owner ballots. | [08-new-capabilities.md](08-new-capabilities.md); [design-and-delivery-index.md](design-and-delivery-index.md) |

The additions constrain every affected contract. Beginner defaults do not hide expert rejection, targets, ownership/effects, cost, or audit output. No optional AI mode is proposed. Foreign source remains authoritative until replacement is accepted. Prerelease freedom is not a justification for current version-migration machinery. Self-hosting stays an evaluation before any owner-approved port.

## Each delivery card has an observable completion boundary

Full steps, path ownership, focused future commands, and ballot text are in Tower and the [publication snapshot](design-delivery.json). The criteria below are the authored rejecting contracts, not results achieved in this campaign.

<a id="card-2934"></a>
### #2934 — Compiler proof: pinned checker toolchain and implementation-bound replay

**Delivers:** A reproducible offline proof construction and replay path binds each checked claim to the exact modeled implementation and produced compiler artifact. The owner selects one proof toolchain; ordinary Jet users need no proof assistant installed.

**Authority:** D-COMPILER-PROOF1,D-RECORD1,I6,I9 Owner choice: D-COMPILER-PROOF-TOOLS1.

1. A clean offline environment reconstructs and replays the pinned proof artifact with the same claim, model, source, checker, and compiler-artifact identities.
2. Deliberately false claims and valid proofs attached to the wrong proposition, source, implementation, or candidate are rejected; missing, timeout, and unsupported remain non-proofs.
3. Every accepted claim names a checked correspondence route to the actual implementation; a detached algorithm theorem or digest-only attachment is refused.
4. The root compiler and seam crates retain path-only dependencies, ordinary Jet installation needs no proof assistant, and raw replay evidence remains independently inspectable.

<a id="card-2935"></a>
### #2935 — Compiler proof: executable Jet semantics and complete observation contract

**Delivers:** One executable formal semantics covers the ratified language and Jet-owned Core behavior, including acceptance, rejection, effects, failures, resources, and permitted schedules. Its rules are checked against the published language rather than copied from one engine.

**Authority:** D-COMPILER-PROOF1,D-TIER-ONEIR1,I1,I2,I3,I4,I5,I9

1. Every current language construct, Core behavior family, and applicable execution boundary appears in the generated obligation relation with a rule and proof owner; omitted or orphaned rows fail the guard.
2. The executable model distinguishes result, failure, mutation, input consumption, effect order, resource premises, and allowed schedules for the retained nontrivial witnesses.
3. The model agrees with ratified examples and rejection contracts; a deliberately wrong rule is detected by an independent conformance witness.
4. No unproved application intention, foreign implementation, scheduler promise, or resource exemption is presented as established Jet semantics.

<a id="card-2936"></a>
### #2936 — Compiler proof: source bytes, parsing, names, and rejection preservation

**Delivers:** The actual source-processing implementation is bound to the formal source semantics, including all consumed input, source origins, and user rejection cases.

**Authority:** D-COMPILER-PROOF1,I2,I3,I4,I7

1. Every source-processing construct in the obligation relation has replayable implementation-bound proof. Missing proof prevents closure, and no successful result can omit a source suffix.
2. Invalid source is rejected for the modeled reason with a registered Jet report at the correct source; a parser or tool crash never becomes ordinary rejection evidence.
3. Precedence, scope, source origins, entry ordering, literal boundaries, and imported graph identity survive the retained positive and negative witnesses.
4. Changing the parser implementation, grammar authority, source graph, or construction inputs invalidates the corresponding proof binding.

<a id="card-2937"></a>
### #2937 — Compiler proof: checked types, ownership, effects, and facts

**Delivers:** Every accepted typed program and every emitted semantic rejection has a checked relation to current type, ownership, effect, contract, and state rules.

**Authority:** D-COMPILER-PROOF1,D-FACTMODEL1,D-FACT-LAW1,D-FACT-OWN1,I1,I2,I3

1. Every checking and fact-propagation obligation is covered by an implementation-bound proof under explicit premises; unknown obligations remain release-blocking.
2. Negative controls reject wrong type/ownership/effect/state facts, invalid joins, stale premises, and forged rejection certificates.
3. The retained alias, callback, generic, failure, and branch witnesses preserve the same checked meaning before lowering; backend acceptance cannot repair a rejected source program.
4. Current enum facts, tighten/loosen gates, memory safety, and diagnostic ownership remain canonical; no second checking implementation gains production authority.

<a id="card-2938"></a>
### #2938 — Compiler proof: compile-time evaluation and dependency validity

**Delivers:** Compile-time evaluation, failure, effects, termination limits, and cached results obey the same formal language rules and dependency identities as the resulting executable program.

**Authority:** D-COMPILER-PROOF1,D-TIER-ONEIR1,I3,I9

1. Every comptime construct and shared operation has implementation-bound preservation evidence, including failure and resource premises rather than results alone.
2. A changed dependency, evaluator artifact, or configuration cannot reuse a stale compile-time result; interrupted evaluation cannot publish a valid cache entry.
3. The retained mixed comptime/runtime witnesses have the same allowed observations on all applicable execution modes.
4. Unsupported, unknown, and exhausted proof attempts remain explicit blocking obligations; the existing build failure is not misrepresented as qualification.

<a id="card-2939"></a>
### #2939 — Compiler proof: shared lowering and optimization preserve observations

**Delivers:** Every Jet-controlled lowering and optimization pass has a checked preservation argument over the canonical operations, including failure, mutation, effects, input consumption, and allowed scheduling.

**Authority:** D-COMPILER-PROOF1,D-TIER-ONEIR1,D-ACCEL1,I3,I9

1. Every production lowering and optimization pass in the generated relation has an implementation-bound preservation argument; any missing pass blocks closure.
2. Negative controls reject reordered effects or failures, extra input consumption, invalid alias assumptions, unsound arithmetic reassociation, and a certificate attached to the wrong operations.
3. Scalar, vector, parallel, and layout choices preserve the same stated numerical and failure contract; cost selection cannot make an illegal transformation legal.
4. Pass composition retains source identity, observation order, and all premises through the final shared operations; no engine-specific semantic lowering is added.

<a id="card-2940"></a>
### #2940 — Compiler proof: Jet-owned Core and runtime behavior

**Delivers:** Jet-owned Core and runtime implementations satisfy the formal contracts that all execution adapters consume. Foreign implementations remain explicit assumptions rather than hidden Jet proofs.

**Authority:** D-COMPILER-PROOF1,D-TIER-ONEIR1,D-FFI-CAP1,I1,I6,I9

1. Every Jet-owned Core/runtime obligation has implementation-bound proof of its contract, including error policy and lifecycle; proving only dispatch or marshalling cannot close the card.
2. Negative witnesses reject wrong defaults, width/bounds conversions, cleanup count, callback lifetime, cancellation behavior, and changed error interpretation.
3. Foreign assumptions are named and bounded at their call edges; an external success status or source hash never proves the Jet-owned wrapper.
4. The same proved semantic bodies serve AOT, default run/dev, interpreter/comptime where reachable, and applicable web targets without host-side policy copies.

<a id="card-2941"></a>
### #2941 — Compiler proof: AOT, JIT, interpreter, and web adapter correspondence

**Delivers:** All applicable adapters preserve the shared operation contract, argument/result representation, failure behavior, event order, and resource lifetime without re-deciding semantics.

**Authority:** D-COMPILER-PROOF1,D-TIER-ONEIR1,I1,I2,I9

1. Every applicable adapter and operation has checked implementation correspondence; missing AOT, default run/dev, interpreter, or applicable web coverage prevents closure.
2. Wrong width/layout, event order, callback/handle lifetime, failure mapping, and source/target identity are rejected by retained negative controls.
3. All modes call the same proved semantic bodies; no adapter re-encodes validation, defaults, or error policy.
4. The artifact records exactly where Jet-owned proof ends and named foreign assumptions begin; no generic platform exemption or new I9 carve-out is introduced.

<a id="card-2942"></a>
### #2942 — Capability evidence: consume one complete contract-to-candidate relation

**Delivers:** The existing qualification and inspection tools answer what each advertised capability means, where it runs, what checked it, and whether that evidence applies now. Missing rows count as missing, not as a smaller product.

**Authority:** D-FACTMODEL1,D-RECORD1,D-HARDENING-GATE1,I4,I5,I7,I9

1. Every canonical first-party capability identity and applicable mode has one counted contract/route/oracle/candidate row, including public types, fields, receiver methods, tools, and foreign boundaries.
2. A newly declared but untested member, discarded output, missing mode, shared wrong oracle, stale candidate, and unsupported path receive non-pass dispositions without shrinking the denominator.
3. CLI inspection, hardening status, learner census, and release claims consume the same row identities and expose their exact evidence and owner.
4. Independent expected outcomes distinguish shared wrong behavior from correct cross-mode agreement, while only explicitly ratified non-applicable cases leave executable coverage.

<a id="card-2943"></a>
### #2943 — Candidate qualification: invalidate stale proof, test, artifact, and release claims together

**Delivers:** A release status describes one exact candidate and every required evidence class. Changing a covered source, generator, tool, target, or artifact invalidates dependent green claims without rewriting historical evidence.

**Authority:** D-COMPILER-PROOF1,D-HARDENING-GATE1,D-RECORD1,I2,I9

1. The existing dashboard and release path consume one candidate record whose required proof, behavior, performance, platform, artifact, and owner evidence all match exact identities.
2. Each covered input substitution invalidates the appropriate dependent green claim; unrelated historical evidence remains intact and cannot be reused as current.
3. Missing, stale, unknown, unavailable, failed, zero-valid, and incorrectly excluded evidence blocks readiness with an exact reason rather than an assembled green summary.
4. All existing assurance, performance, and sustained-handoff thresholds remain unchanged; prerelease truth is explicit, and self-hosting/current-version migration adds no new gate.

<a id="card-2944"></a>
### #2944 — Compiler proof: composed candidate theorem and fail-closed release evidence

**Delivers:** One candidate-bound checked argument composes source processing, checking, comptime, lowering, optimization, Core/runtime, and adapters. Any uncovered Jet-controlled stage blocks the compiler-proof gate.

**Authority:** D-COMPILER-PROOF1,D-HARDENING-GATE1,D-RECORD1,I1,I2,I4,I5,I9

1. The checked candidate theorem composes all Jet-controlled stages and names exact source, model, proof, checker, compiler-artifact, target, and configuration identities.
2. Removing one obligation or substituting any stage, intermediate representation, proof object, source input, or candidate binary makes qualification fail.
3. A reject-everything compiler, wrong diagnostic, unobserved required behavior, foreign-tool assumption disguised as proof, or missing execution mode cannot produce a complete compiler-proof verdict.
4. The existing candidate dashboard exposes the new ratified compiler-proof gate without weakening D-HARDENING-GATE1, performance policy, frozen #2420, or deferred self-hosting.

<a id="card-2945"></a>
### #2945 — Explanations: one checked derivation record across facts, execution, and optimization

**Delivers:** A consumer can ask what a claim means, what established it, which premises it uses, and whether it still applies. CLI, editors, review, teaching, and optimization consume the same compiler-owned records.

**Authority:** D-FACTMODEL1,D-FACT-LAW1,D-FACT-OWN1,D-PROVE-SEM1,D-JPROOF1,D-RECORD1,D-DEVR-CAUSE1 Owner choice: D-EXPLANATION-RECORD1.

1. The same bounds, mutation, failure, ownership, and optimization witnesses expose identical claims, premises, identity, and disposition through CLI, editor, review, and learner consumers.
2. A changed premise or revision invalidates every dependent explanation, and a missing or redacted observation is never displayed as a current value or proof.
3. A plausible but false explanation, wrong-source attachment, fabricated counterexample, and sampled agreement labeled universal proof are rejected by retained controls.
4. Consumers use the existing authoritative facts and records; no second evaluator, public fact model, artifact container, or AI dependency is introduced.

<a id="card-2946"></a>
### #2946 — Reasoning views: source-linked values, relationships, and proof limits

**Delivers:** A user can follow values, ownership, state, effects, dependencies, and change impact from ordinary source, with the same complete text route and no Studio or AI requirement.

**Authority:** D-CLI-ONE1,D-REF3,D-REPORT-EDITOR1,D-CANVAS-PROOFLENS1,D-DEVR-CAUSE1 Owner choice: D-EXPLAIN-VIEW1.

1. A newcomer and expert can trace the same source-backed value, owner, state transition, effect, dependency, and first changed observation through ordinary editor and complete text paths.
2. Current, stale, redacted, unavailable, running, failure, and empty states show the correct source/run identity and do not fabricate values or proofs.
3. Keyboard-only, no-color, reduced-motion, narrow/wide viewport, and large-data paths preserve every relationship and action; no AI or Studio dependency exists.
4. Owner visual acceptance uses the complete copy-paste runbook and confirms readable hierarchy, useful default detail, and reachable expert evidence.

<a id="card-2947"></a>
### #2947 — Jet Learn: prediction, counterexample, change, and transfer interactions

**Delivers:** The existing offline jet learn runner teaches a model through prediction, observed evidence, a minimal counterexample or explanation, a controlled change, and a structurally different transfer task.

**Authority:** D-LEARN1,D-TEACH-FOREIGN1,D-REPORT-EDITOR1,D-RECORD1 Owner choice: D-LEARN-FEEDBACK1.

1. The selected learning order works end to end for a loop, a state/lifetime error, an effect boundary, and a foreign-boundary task using real compiler or execution evidence.
2. The learner must make a prediction, explain the result, change one relevant condition, and solve a structurally different transfer task; copying a solution or matching a canned explanation cannot satisfy all four.
3. Wrong, unknown, unavailable, stale, cancelled, and completed states behave honestly across watch, one-shot, ordinary editor, and JSON paths; user edits survive interruption and resume.
4. The same curriculum works offline without AI or Studio, with keyboard/text alternatives and an owner-reviewed learning runbook.

<a id="card-2948"></a>
### #2948 — Semantic review: controlled changes and first differing observations

**Delivers:** The existing jet review path explains one controlled change using current semantic operations, claims, and comparable recorded observations. It distinguishes checked preservation from agreement on examples.

**Authority:** D-DEVR-REVIEW1,D-DEVR-TRY1,D-DEVR-CAUSE1,D-RECORD1,D-PROVE-SEM1

1. A rename, an authority widening without annotation text change, a lost proof, a changed failure/input-consumption order, and a same-output but unproved change receive distinct correct explanations.
2. A valid preservation argument is shown with its exact scope; sampled agreement, missing evidence, and incompatible input/environment identities cannot be labeled behaviorally equivalent.
3. The first differing observation is reproducible from the same recorded inputs and links to the responsible source operation; ambiguous alignment remains unknown.
4. All consumers use the existing jet review/semantic-op/record route, with no new command, text-diff authority, second evaluator, or AI dependency.

<a id="card-2949"></a>
### #2949 — Shared state: atomic snapshots and stale-result rejection

**Delivers:** A delayed computation can publish only against the same committed state it observed. The chosen interface preserves existing ownership and transactions, rejects ABA and wrong-owner tickets, and never retries effects silently.

**Authority:** D-CONC-SHARE1,D-SHARED-API1,D-ROLLBACK-TRAIT,D-POOLID-API1,D-MEM-COPYSEM1,D-FACT-LAW1,I1,I9 Owner choice: D-SHARED-REVISION1.

1. Two competing publications from one snapshot have at most one winner; stale, ABA, wrong-owner, repeated-ticket, and generation-exhaustion cases never overwrite the current value.
2. Snapshots contain one atomic value/revision pair, preserve ordinary copy and ownership rules, and cannot confer mutation authority or be forged by decoding.
3. Plain Shared writes and committed/aborted/nested transactions preserve the defined revision law, including snapshots captured before later transaction-local writes.
4. The complete interface, typed failures, and lifecycle behave identically on AOT, default run/dev, interpreter, and applicable web targets through one shared semantic implementation.

<a id="card-2950"></a>
### #2950 — Core Testing: reproducible two-implementation comparison with honest outcomes

**Delivers:** One existing test mechanism compares two implementations on the same declared cases and observation relation, retains a useful counterexample, and never calls empty or inconclusive agreement a proof.

**Authority:** D-TESTKIT1,D-RECORD1,D-ADOPT-TIER1,I6,I9 Owner choice: D-TEST-COMPARE1.

1. An ordinary Jet test compares two pure functions and two explicitly isolated stateful/foreign scenarios, then replays the same identified mismatch through CLI and ordinary editor output.
2. Empty, entirely discarded, unavailable, timed-out, cancelled, and invalid-oracle runs cannot pass; equal sampled results cannot be described as universal proof.
3. Reduction preserves the same mismatch and observation relation, including ordered effects or typed failure where declared; input/environment contamination is detected.
4. The same Core behavior and failure meaning reach AOT, default run/dev, interpreter, and applicable web modes through one Prelude implementation, with no external dependency or second test runner.

<a id="card-2951"></a>
### #2951 — Optimization experiments: checked laws, rejected counterexamples, and whole-job cost

**Delivers:** Every retained optimization candidate has a machine-semantic law, artifact-bound legality evidence or explicit non-proof, falsifying inputs, and same-job cost evidence before production adoption.

**Authority:** D-COMPILER-PROOF1,D-TIER-ONEIR1,D-ACCEL1,D-PLACE1,D-LOOPREAD1,I9

1. All five families have executable controls that distinguish an invalid rewrite from a legal slow rewrite and a legal measured improvement.
2. Every accepted legality claim is bound to the actual transformation/checker and exact machine semantics; bounded search, sampled agreement, timeout, and missing premises remain non-proofs.
3. Each candidate reports the same-job required metrics and modes with raw identities and explicit wins/losses/unavailable results; no excluded cost or substituted peer yields a win.
4. A selected candidate is explainable through existing inspect evidence and assigned to its exact implementation owner; a rejected candidate stays out of production without creating a parallel compiler or weakening law.

<a id="card-2952"></a>
### #2952 — Test economics: preserve unique defect detection while removing redundant cost

**Delivers:** The existing test and hardening portfolio identifies which behavioral defects each test can detect, what it costs, and what evidence a removal would lose. Selection cannot hide mandatory obligations.

**Authority:** D-HARDENING-GATE1,D-RECORD1,I4,I5,I9

1. The report names each retained, consolidated, and removed test’s obligation, detected defect shape, measured cost, and replacement evidence rather than using test count as value.
2. All mandatory capabilities/modes and uniquely detected historical or seeded defects remain covered after selection, with deterministic selection and raw same-candidate evidence.
3. Zero-valid, all-excluded, discarded-output, broken-oracle, wrong-identity, and harness-error controls fail the qualification path rather than yielding a green result.
4. Measured reductions preserve the ratified hardening and release requirements and use the existing runners; no skipped obligation is hidden behind a smaller denominator.

<a id="card-2953"></a>
### #2953 — Foreign reasoning: one boundary contract from binding through accepted replacement

**Delivers:** A user can inspect a foreign call, identify every assumption and conversion, debug through it, compare an explicit replacement, and retain foreign source authority until accepting that replacement.

**Authority:** D-FFI-CPP1,D-FFI-UNIFY1,D-ADOPT-TIER1,D-MIGRATE-SRC1,D-TEACH-FOREIGN1,I6,I9

1. Each named language family has a consumed boundary row with an executable supported path or exact documented unsupported/binder-only disposition; no command-presence claim substitutes for behavior.
2. Real ownership/cleanup, exception/error, callback/thread, layout/encoding, stale-artifact, and copy-cost witnesses agree across applicable Jet modes and remain inspectable from ordinary tools.
3. A supported foreign-to-Jet replacement retains matched and mismatched raw observations and explicit assumptions; sampled agreement is not universal proof, and the foreign source is not silently replaced.
4. Native export and existing foreign build hosts reproduce the same source/tool/target contract, while unsupported C/C++ source conversion and Ada/Pascal binder-only classification remain honest.

<a id="card-2954"></a>
### #2954 — Frontier hypotheses: falsifiable same-job experiments and independent replication

**Delivers:** Each hypothesis has an executable intervention, a comparable baseline, a result that could refute it, and independent replication on identified whole jobs. Novelty is claimed only when the evidence and prior-art comparison support it.

**Authority:** D-COMPILER-PROOF1,D-TIER-ONEIR1,D-FACTMODEL1,D-HARDENING-GATE1

1. All four hypotheses have an executed baseline/intervention pair, explicit falsifier, raw identities, result disposition, and independent replay.
2. Whole-job comparisons cover the eight required mission areas without easier substitutions, omitted modes, averaged losses, or evidence classes promoted beyond their scope.
3. Temporal studies distinguish revision, generation, lifetime, and clock semantics; explanation studies distinguish model friction from actual human transfer.
4. The final result states what is new relative to named prior work, what failed, what remains unknown, and the exact implementation owners; no breakthrough or universal quality claim exceeds the evidence.

<a id="card-2955"></a>
### #2955 — Self-hosting evaluation: compiler-shaped workloads before a port decision

**Delivers:** A concrete workload evaluation informs #217 without treating self-compilation as correctness or silently making a compiler port a 1.0 requirement.

**Authority:** D-TIER-ONEIR1,D-COMPILER-PROOF1,D-HARDENING-GATE1

1. All eight compiler-shaped workload families have executable intended behavior, exact identities, supported-mode results, resource/latency evidence, and a same-job oracle or named boundary.
2. The report distinguishes implementation gaps, readability/repair friction, measured cost, and proof/build trust rather than using successful self-compilation as a readiness certificate.
3. Every blocking defect shape has exact ownership, and the retained Rust/foreign boundary is explicit without starting or pretending to complete the compiler port.
4. The existing bootstrap owner receives a go/no-go evidence package; no new release gate, compatibility layer, or owner approval is implied.


## Preserved records and clean boundaries

- No writes to finalized D-PLACE1, D-ACCEL1, or D-LOOPREAD1.
- No writes to #2920–#2923 or owner-frozen #2420. Existing bugs were not filed again.
- D-FFI-CPP1=A remains the C++ authority; its archived state is not a reason to reopen it.
- #2925 owns the proof-policy work; #2926 owns the learning census, not the missing learning engine; #2927 owns truthful release/capability claims. The new child and delivery cards make the missing implementation explicit.
- No current compatibility baseline, compiler port, new language keyword/sigil, invariant exception, stdlib external dependency, AI mode, or separate production obligation registry was introduced.

## Generalize every finding

[The design section's complete shape table](08-new-capabilities.md#generalize-every-finding) maps each retained defect to its mechanism, predicted siblings, structural correction, and owning card. The delivery contracts require a census or a rejecting relation for those sibling instances, not only a fix for the illustrated input.

Publication verification is recorded in [design-verification.json](design-verification.json): exact Tower readback, all 30 question dispositions, dependency closure, complete short ballots, and local report references. The existing [model execution receipt](demonstrator-receipt.json) remains bounded non-Jet evidence.
