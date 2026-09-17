# Audit-derived skill proposals

Date: 2026-09-11. Source inspection: current main checkout during the 2026-09-10 study. Work home: [Tower #3037 / c0if11mo](http://127.0.0.1:7878/api/card?id=%233037).

## Initial Astra recommendation

Deepen two existing skills: **spec-compliance-audit** and **pragmatism-audit**. Offer two separate design choices: a controlled-trial extension to **persona-audit**, and a new, narrowly scoped **evidence-audit**. Do not approve them as a bundle.

The corpus contains more reusable procedure than Jet needs new skill names. Its strongest recurring methods are source-derived denominators, observable counterexamples, library-first attribution, controlled user trials, and deliberate challenges to apparently green evidence. Most already have a suitable owner. The distinct uncovered question is: **would this evidence reject a plausible wrong implementation?**

| Proposal | Destination | Recommended choice | Distinct addition | Owner ballot |
|---|---|---|---|---|
| A1. Observable conformance reruns | Extend spec-compliance-audit | Design + implement | Join declarations to observable behavior; compare scoped reruns without hiding denominator changes; use independent expected results, not tier agreement alone | `D-SKILL-CONFORMANCE1` |
| A2. Library-first workload attribution | Extend pragmatism-audit | Design + implement | Separate missing foundations, missing libraries, broken adapters, and unused existing features before proposing product changes | `D-SKILL-WORKLOAD1` |
| A3. Controlled first-session and repair trials | Extend persona-audit | Design only | Freeze tasks and assistance policy; separate checker success, semantic success, recovery, and earned preference | `D-SKILL-PERSONA-TRIAL1` |
| A4. Evidence challenge | Create evidence-audit | Design only | Audit the strength and scope of proof records, oracles, and gates rather than audit product conformance or close a code change | `D-SKILL-EVIDENCE1` |

These are independent choices. A1 and A2 remain useful if A3 and A4 are deferred. No new runner, registry, work ledger, daemon, or blanket audit coordinator is proposed.

### Ballot design refinement — Main, 2026-09-11

The table above and the detailed A3/A4 proposals below preserve the study's initial recommendations. Subsequent ballot review removed two costs without adding another mechanism:

- **A3:** Make the controlled trial an optional branch of persona-audit, not a prerequisite for ordinary persona work. A bounded agent trial can use the retained configuration fixture and existing hosts; it does not authorize recruiting people or creating a runner. Missing trial prerequisites block that trial and its claims, not the ordinary audit. This permits an independent design-and-implementation choice.
- **A4:** Strengthen existing research guidance and routing before adding evidence-audit. Verify still owns current change closeout; spec-compliance-audit still owns checks against ratified contracts; research can own a retrospective evidence challenge. A separate skill remains a design alternative, not the default.

A1 reinforces an existing duty: spec-compliance-audit already checks behavior against approved rules. Its proposed addition makes repeat-audit comparisons more explicit; it does not establish that the current skill ignores correctness.

The owner clarified A2 on 2026-09-11: routine work should use Jet's own CoreLib and first-party libraries, without requiring foreign-library setup or bindings. Library code is not automatically preferable to compiler support. Repeated boilerplate and awkward patterns warrant comparing both approaches on the same job, including performance, usability, readability and ease of reasoning. The revised ballot follows that direction rather than the initial foreign-library probe.

These are design reasons, not approval or implementation evidence. Tower carries the exact independent choices and their authorization boundaries.

## What was examined

The inventory covered the root of `docs/audits` and all seven discovered subtrees: `raw`, `jet-refounding-2026-09-05`, `jet-foundations-and-trust-2026-09-04`, `domain-matrix-2026-09-02`, `domain-foundations-2026-09-02`, `domain-dx2`, and `jit-aot-parity-2026-08-31`. It also covered the retained research and proposal families exposed by [documentation navigation](../README.md), report cross-links, and the Tower Docs index.

Coverage is at the **method-family level**, not a claim to have read every generated row. Markdown, HTML, JSON, TSV, compressed JSONL, receipts, logs, and image assets were grouped by the investigation they represent. Substantive report sections, conclusions, schemas, selected records, and relevant generator functions supplied the method evidence. Generated repetitions and historical raw outputs were not treated as independent methods. The method-family inventory below records the reading depth and source boundaries.

The requested Astra study produced the initial inventory and proposals. Integration and independent review separated three additional method boundaries below. Those corrections do not represent new workload, accessibility, or hardening runs.

Existing-skill comparison used the current main-checkout [router](../../.agents/skills/JetSkillsRouter.md), repository skills, and Tower plugin skills. It included the newly present [pulse](../../.agents/skills/pulse/SKILL.md) skill. The router's retirement and managed-source dispositions were retained. A historical report title is not permission to restore a retired route.

The study ran no compiler, audit generator, skill pilot, test, or benchmark. Its conclusions come from source inspection and attributed historical evidence. Integration checked report links and Tower ballot readiness; those checks do not establish the proposed skills' behavior.

## What the corpus teaches

### Presence, reachability, execution, and correctness are different claims

The census families make this distinction concrete. The [example/Core report](../audits/example-core-census-2026-09-05.md) records static golden-backed coverage separately from runtime observations. Its generator's `classifyLocations`, `coverageFields`, and `executeRepresentative` functions explicitly distinguish static coverage from one representative executable witness. A passing representative example does not execute every represented Core operation.

The [syntax census](../audits/syntax-roundtrip-census-2026-09-03.md) distinguishes freshness checking from its executable matrix gate. The [authority census](../audits/effect-authority-census-2026-09-03.md) uses paired allow/refuse programs and records missing tiers. The [diagnostic census](../audits/diagnostic-actionability-census.md) classifies proposed fixes statically; it does not establish that a person or agent can apply them successfully.

These are not reasons to invent four more census skills. They are evidence for a stronger conformance procedure and, where interaction matters, a controlled persona procedure.

### Agreement can preserve the same wrong answer

The [silent-data sweep](../audits/silent-data-sweep-2026-08-28.md#method) used independent oracles and consumed computed results. The later [2026-09-10 mine](../audits/mine-for-jet-2026-09-10.md) retained a particularly clear counterexample: numeric map values `[2, 10]` produced the wrong ordering through display-string comparison. That report owns the historical observation; this study did not rerun it.

The reusable method is broader than that defect: define the expected relation independently, preserve relevant failure and effect behavior, exercise the public path, compare applicable execution modes, and reduce the failing relation. Cross-tier agreement is valuable adapter evidence, but it is not an independent semantic oracle. The trust report states this explicitly in [Q22](../audits/jet-foundations-and-trust-2026-09-04/04-trust.md#q22).

### A domain gap is not automatically a Core feature

The [domain-foundations method](domain-foundations-method.md), [retained results](../audits/domain-foundations-2026-09-02/), and [history of the withdrawn matrix approach](domain-foundations-history.md) separate what a normal library can build from what the compiler or platform must supply. Concrete probes expose bridge ownership, exact arithmetic, execution-tier, scheduling, and hardware boundaries that feature-name inventories can conceal.

The [friction audit](../audits/friction-audit-2026-08-30.md#thesis) found a complementary failure: some apparent library gaps were existing features that canonical programs and generators did not use. Its prevention analysis distinguishes a better default or recipe from a lint that merely complains about a missing API. Together, these methods deepen pragmatism-audit without creating a competing domain planner.

### A successful check is not successful work, and successful work is not preference

The [agent codegen experiment](../audits/agent-codegen-benchmark-2026-08-23.md#harness-definition) separated diagnostic-guided repair from semantic checking. Its conclusion explicitly rejects broad language-quality claims from one agent and six tasks.

The [earned-preference campaign](../proposals/dogfood-jet-experience-5-of-5.md), [rerun protocol](../audits/fresh-agent-5-of-5-rerun-2026-09-05.md), and [retained task fixtures](../audits/raw/2393-r2/fixtures.json) add frozen task contracts, controlled assistance, participant identity, refusal cases, and explicit non-results. The September rerun report says execution did not start; its fixtures and schema are not participant evidence.

Persona-audit already requires real projects, an unattended coding agent, first-session observations, and push/pull evidence. What it lacks is a reusable controlled-trial branch that prevents a supported demonstration from being reported as cold-user success or earned preference.

### A gate needs evidence that it can fail for the right reason

[Trust Q22–Q25](../audits/jet-foundations-and-trust-2026-09-04/04-trust.md#q22) distinguish useful detection from test count, independent oracles from shared implementations, and same-candidate qualification from assembled green fragments. The current [test-economics implementation](../../tools/agent-eval/test-economics/test-economics.mjs) already contains evidence normalization, identity checks, and false-green controls. The [security closure procedure](../spec/contributing/security-closure.md) separately owns sealed scan identity and completeness.

The missing reusable question is not another implementation of those mechanisms. It is a bounded challenge to an evidence claim: identify what the claim promises, inspect what the evidence actually observes, and demonstrate which plausible wrong behavior it rejects. That question has a different result from spec compliance, a diff review, or code closeout. It justifies designing A4, but not assuming a new public route is already worth its maintenance cost.

## Proposed changes and their boundaries

### A1. Extend spec-compliance-audit

**Trigger:** conformance or regression checking of a named ratified surface, including a requested rerun of an earlier audit. **Inputs:** current law, executable source identities, relevant prior evidence, applicable targets, and an authorized finite probe scope. **Procedure:** derive the denominator from existing registries; distinguish static and executable evidence; select value-consuming success, refusal, and boundary cases; establish an independent expected relation where needed; compare current results with comparable prior results; identify newly exposed combinations separately from historical defects.

The [current skill](../../.agents/skills/spec-compliance-audit/SKILL.md) already demands live probes and honest shipped/partial/gap classifications. A1 adds the missing longitudinal and adversarial method, not a second compliance engine. It must preserve the skill's report-only boundary and prohibition on reopening syntax.

**Future proof:** rerun the exact numeric-map reproducer retained by the September mine, using numeric expectations and applicable modes. A deliberately supplied comparison bundle in which every mode agrees on the same wrong value must remain a failure. A missing mode must remain unmeasured or blocked, not become a pass. No such proof was run here.

### A2. Extend pragmatism-audit

**Trigger:** a real workload seems to require new language machinery, a new battery, or repeated hand-written plumbing. **Inputs:** a concrete job, its observable success contract, normal Jet package boundaries, current APIs and decisions, and representative evidence from more than one affected use where available. **Procedure:** attempt the ordinary package solution first; trace a failure to its owning layer; distinguish capability absence from broken execution and poor discoverability; reuse the existing friction taxonomy; show the beginner default and expert reject/override path; identify the earliest appropriate prevention point.

The [current skill](../../.agents/skills/pragmatism-audit/SKILL.md#method) already walks real jobs and classifies friction. A2 adds causal attribution and library-first counterexamples. It does not replace the domain registry, create per-domain skills, automatically add APIs, or launch a performance campaign.

**Future proof:** reuse the [foreign-library probe](../audits/domain-foundations-2026-09-02/results/prim-bridges.md). Distinguish a function that can be called from a safe, owned, dependency-complete library boundary. As a negative control, present an application that merely needs a domain algorithm implementable over adequate existing primitives; the audit must not call that a compiler defect or mandate a new Core module.

### A3. Extend persona-audit with controlled trials

**Trigger:** cold-start usability, diagnostic recovery, sustained edit/run experience, or earned preference is the requested outcome. Ordinary persona checks retain their existing lighter process. **Inputs:** frozen behavioral tasks, permitted documentation and assistance, exact participant/model and tool identities, session state, success and refusal oracles, and declared limits. **Procedure:** separate check success from semantic success; retain the first failure; enforce the assistance policy; record recovery and interaction evidence; ask preference only after the declared task; report missing participants and failed preflight as non-results.

This belongs in [persona-audit](../../.agents/skills/persona-audit/SKILL.md), not in a new dogfood or diagnostic-audit wrapper. It adds experimental controls to the skill's existing first-session and coding-agent requirements. GUI claims still require actual frames and interactions. Agent results must not be generalized to human preference.

**Future proof:** run the retained `2393-r2` T1 configuration task under an explicitly authorized fresh-participant protocol. Compare valid output and duplicate-field refusal after the allowed repair phase. A coached session or a green checker result with wrong output must not count as cold-session semantic success. Astra initially recommended design only until the host and participant requirements were explicit; Main's refinement above supplies a bounded alternative through the owner ballot.

**Design-only deliverable and stop:** persona-audit's owning card receives a proposed controlled-trial branch, its participant and host prerequisites, bounded delegation contract, frozen pilot protocol, and acceptance criteria. The active skill, router, runners, and product remain unchanged. The pilot and implementation require a later explicit owner authorization; accepting the design does not authorize participant recruitment or execution.

### A4. Design evidence-audit

**Trigger:** the owner requests a standalone retrospective challenge to a bounded, unchanged-tree evidence portfolio: a test suite, receipt set, scan, or benchmark. A current code, card, milestone, or release closeout claim routes to verify instead. **Inputs:** the claim set, governing contracts, existing manifests and raw records, producer/checker identity, and plausible failure controls. **Procedure:** map each claim to observations and assumptions; inspect oracle independence and run identity; challenge the claim with existing or explicitly authorized controls; retain every missing obligation; recommend narrower claims or stronger observations without modifying the implementation.

The result is a **claim-to-evidence adequacy report**, not a code-completion verdict. [Verify](../../.agents/skills/verify/SKILL.md) owns change closeout. [Code-review](../../.agents/skills/code-review/SKILL.md) owns Standards and Spec review of a fixed diff. Neither currently owns a standalone, unchanged-tree challenge to the adequacy of an evidence portfolio.

**Future proof:** compare the proposed procedure with bounded research on the same frozen claim set, records, controls, and assistance. Declare the observable advantage before either trial: detecting an additional seeded inadequacy or producing a more repeatable claim disposition without losing a correct finding. Use existing test-economics record shapes and controls. A valid independent observation retains its narrow claim; mismatched candidate identity or harness failure cannot discharge it; duplicate representations cannot become independent oracle classes. If the proposed skill adds no demonstrated advantage, defer it and retain bounded research.

**Design-only deliverable and stop:** the evidence-audit proposal's owning card receives a draft procedure, exact routing exclusions, a same-claim-set comparison protocol, cost and stop rules, and an implementation acceptance plan. Do not create an active skill or change the router, runners, or product. The comparison and route activation require later explicit owner authorization.

**One report owner:** A1, A2, and A3 remain branches of their existing skills. If A4 is later approved and implemented, it owns only the standalone evidence-adequacy report. A delegated probe or participant returns bounded observations to that primary owner; it does not start another audit, write Tower, or issue a competing completion verdict.

## Rerun, progress, regression, and opportunity design

Keep this procedure within the owning method and existing evidence formats. Shared evidence rules should have one home, referenced by skills rather than copied into each one.

1. **Pin the comparison boundary.** Record the source and binary identities actually used, governing decision, target, configuration, fixture/oracle version, and requested scope. Historical receipts remain immutable.
2. **Re-derive the denominator.** Match existing semantic keys and decision-backed forms. Record added, changed, removed, and incomparable entries. Line numbers and mutable source hashes are evidence coordinates, not sufficient cross-revision identity. An unexplained disappearance is not progress.
3. **Separate evidence kinds.** Registration, consumer reachability, static examples, runtime observations, independent expected results, interactive success, and performance each support different claims. Do not compress them into one green percentage.
4. **Compare only comparable observations.** Report fixed, recurring, regressed, newly exposed, retired by authority, and not re-exercised findings separately. Retain unavailable targets in the applicable denominator. No weighted average may erase a required failure.
5. **Challenge the measurement.** Include the method's negative control: wrong expected behavior, missing authority refusal, stale identity, ineffective fix, coached participant, or non-equivalent workload. A clean result without a working detector is weak evidence.
6. **Search beyond the old witness.** Probe changed entry points and adjacent combinations suggested by the mechanism: receiver/plain calls, empty and boundary inputs, state transitions, failure order, default versus expert control, and newly applicable targets. The request bounds this search; it is not permission for a new whole-project campaign.
7. **Stop honestly.** Finish when the declared scope has an evidence disposition and the planned controls have a result, including explicit unavailable or blocked results. A clean scan establishes scoped evidence of convergence at a named revision, not permanent bug freedom. Report completion never closes implementation work by itself.

Progress belongs in these dated comparisons. Current work status belongs in Tower; an on-request progress summary belongs to the existing pulse skill. Do not create another dashboard or audit-maintenance queue.

## Conversions to reject

- **Retired field-audit:** gauntlet already owns peer comparison and corpus evolution. Preserve old reports as evidence; do not restore the route.
- **A new mining or lessons wrapper:** mine-for-jet, lessons-learned, surface-research, and surface-frequency-audit already own source capture, peer regret, surface discovery, and frequency measurement.
- **One skill per census:** the generators own executable schemas and denominators. A1 consumes them; A3 measures experienced recovery where static actionability is insufficient.
- **A security-scan wrapper:** the security closure procedure and executable scan contract already own request identity, retained artifacts, and rejecting checks. A4 may inspect a claim's boundary; it must not replace the security provider or scan process.
- **A new refounding, ontology, type, or architecture audit:** first-principles-audit, isomorphic-ontology-audit, type-unification-audit, codebase-design, and improve-codebase-architecture already have distinct methods. Retain the reports' unique rationale rather than copy their chosen designs into skills.
- **A recurring report or repository janitor:** source-history and cleanup reports are dated evidence. Existing cleanup skills cover explicitly requested cleanup. Reports, prompts, and stale paths do not authorize deletion or a recurring cleanup service.
- **A new status or planning skill:** status and the Tower skills already own these requests. No audit method should become a second planner.
- **A visual-report wrapper:** HTML and screenshots are representations and evidence, not independent audit methods. The current HTML route remains explicitly requested.
- **A monetization or proposal-specific skill:** the retained business research, optimizer alternatives, rollback design, and generic-module rationale are source-backed subjects for research or first-principles work, not recurring procedures merely because they are long documents.

## Standing-lens result

**How Jet can beat a matched alternative:** use accurate, source-derived obligations and value-consuming probes to reach a trustworthy answer with less duplicated investigation. This is a proposed advantage, not a measured speed or preference win.

**What to avoid:** static coverage promoted to runtime coverage; shared wrong answers called independent agreement; index counts called native conformance; schemas called completed trials; historical fixes recycled as current findings; broad success claims assembled from incompatible receipts.

**Agent usefulness:** the candidates target the existing five quantities: verdict fidelity, verdict latency, verdict actionability, context economy, and repair determinism. Future trials must measure the relevant quantities rather than infer improvement from shorter reports or more skill names. Runtime and resource-cost claims remain separate, measured obligations under the current performance policy.

**Concrete surfaces:** the examined census generators, example/Core records, authority pairs, diagnostic registry, trial fixtures, domain probes, test-economics records, and security scan procedure provide exact source homes. Their presence is established by source inspection; their current runtime behavior was not exercised in this assignment.

## Method-family inventory

This is the study's dated coverage record, not a current completion dashboard. Source links identify the retained evidence; the depth notes limit what was independently examined.

### External mining and peer-language lessons

Substantive method, verdict, source-quality, limitation, and selected finding sections across older and current reports. The very large September reports were read selectively across distinct finding classes, not line-by-line. Linked external captures and every audience record were not reopened.

**Reusable method:** Separate source claims, corroborated mechanisms, audience signals, current Jet observations, and recommendations; retain exact counterexamples and distinguish optimization hypotheses from qualified wins.

**Disposition:** Existing mining and lineage methods already cover the primary outcome. Retain reports; use their independent-oracle and assurance examples as evidence for A1 and A4.

- [docs/audits/mine-for-jet-2026-09-09.md](../audits/mine-for-jet-2026-09-09.md)
- [docs/audits/mine-for-jet-2026-09-10.md](../audits/mine-for-jet-2026-09-10.md)
- [docs/audits/mine-for-jet-casey-2026-09-10.md](../audits/mine-for-jet-casey-2026-09-10.md)
- [docs/audits/2026-07-24-verse-video-mining.md](../audits/2026-07-24-verse-video-mining.md)
- [docs/audits/2026-07-24-logan-smith-rust-series-mining.md](../audits/2026-07-24-logan-smith-rust-series-mining.md)
- [docs/audits/video-mine-five-languages-2026-08-28.md](../audits/video-mine-five-languages-2026-08-28.md)
- [docs/audits/video-mine-never-operators-supply-chain-2026-09-01.md](../audits/video-mine-never-operators-supply-chain-2026-09-01.md)
- [docs/audits/video-mine-cpp-loops-simd-build-2026-09-03.md](../audits/video-mine-cpp-loops-simd-build-2026-09-03.md)
- [docs/audits/video-mine-cpp-loops-simd-build-2026-09-03.html](../audits/video-mine-cpp-loops-simd-build-2026-09-03.html)
- [docs/audits/deploy-rs-lessons-2026-08-06.md](../audits/deploy-rs-lessons-2026-08-06.md)
- [docs/audits/language-lessons-and-regrets.md](../audits/language-lessons-and-regrets.md)
- [docs/audits/lessons-learned-2026-07-23.md](../audits/lessons-learned-2026-07-23.md)
- [docs/audits/surface-research-2026-07-23.md](../audits/surface-research-2026-07-23.md)

### Diagnostic actionability census

Report rules and distinct status/fix classes; generator constants, classifyFix, classify, and census record construction. No diagnostic was induced or repaired.

**Reusable method:** Derive rows from the diagnostic registry; distinguish active, retired, and reserved entries; classify concrete edits, commands, and paths while keeping static actionability separate from experienced repair.

**Disposition:** Do not create a diagnostic-census skill. Use A1 for conformance and A3 when the requested outcome is actual recovery.

- Source family: `docs/audits/diagnostic-actionability-census.{md,json,tsv}`.
- [scripts/agent/diagnostic-actionability-census.mjs](../../scripts/agent/diagnostic-actionability-census.mjs)
- [docs/spec/guides/diagnostic-recovery.md](../spec/guides/diagnostic-recovery.md)

### Syntax, tier, example/Core, and authority censuses

Report contracts, schema/record examples, distinct coverage and failure classes, and targeted generator bodies. Large generated rows were not exhaustively reread. Generators were not executed.

**Reusable method:** Derive a finite denominator from canonical source; preserve source coordinates and semantic identity; separate static evidence from execution; use named three-surface or three-tier cells and paired allow/refuse witnesses; keep unavailable results visible.

**Disposition:** Deepen one existing conformance skill rather than create four census wrappers.

- Source family: `docs/audits/syntax-roundtrip-census-2026-09-03.{md,json,tsv}`.
- Source family: `docs/audits/tier-census-2026-09-03.{md,json,tsv}`.
- Source family: `docs/audits/example-core-census-2026-09-05.{md,json,tsv}`.
- Source family: `docs/audits/effect-authority-census-2026-09-03.{md,json,tsv}`.
- [scripts/agent/syntax-roundtrip-census.mjs](../../scripts/agent/syntax-roundtrip-census.mjs)
- [scripts/agent/tier-census.mjs](../../scripts/agent/tier-census.mjs)
- [scripts/agent/example-core-census.mjs](../../scripts/agent/example-core-census.mjs)
- [scripts/agent/effect-authority-census.mjs](../../scripts/agent/effect-authority-census.mjs)

### Earlier conformance and capability ledgers

Substantive conformance summaries, completion boundaries, source/decision joins, and selected JSON claim records. Historical status rows were not independently revalidated.

**Reusable method:** Trace a claimed feature through law, source consumers, example or test artifacts, and the exact claimed completion boundary. A narrow implemented seam does not establish the broader feature.

**Disposition:** Existing spec compliance owns the question. Preserve dated ledgers; use them to motivate A1's comparison method and A4's claim-scope checks.

- [docs/audits/spec-compliance-audit-2026-07-22.md](../audits/spec-compliance-audit-2026-07-22.md)
- [docs/audits/syntax-law-source-status-matrix-2026-07-07.md](../audits/syntax-law-source-status-matrix-2026-07-07.md)
- [docs/audits/marker-plane-source-of-truth-matrix-2026-07-07.md](../audits/marker-plane-source-of-truth-matrix-2026-07-07.md)
- [docs/audits/capability-ledger-report.json](../audits/capability-ledger-report.json)
- [docs/audits/feature-claim-report.json](../audits/feature-claim-report.json)
- [docs/audits/language-shape-conformance.md](../audits/language-shape-conformance.md)
- [docs/audits/effect-root-structure-consumers-2026-08-20.md](../audits/effect-root-structure-consumers-2026-08-20.md)
- [docs/audits/core-breadth-audit.md](../audits/core-breadth-audit.md)
- [docs/audits/framework-transplant-closeout.md](../audits/framework-transplant-closeout.md)

### Silent-data, numerical, and execution-tier investigations

Methods, outcome boundaries, selected semantic failures, tier limitations, and the retained failed parity result. Raw shard logs and every case were inventoried rather than fully replayed or reread.

**Reusable method:** Consume values and effect observations; use independent oracles or justified laws; compare applicable modes; retain failure seeds and reduce the semantic relation, not merely the crash.

**Disposition:** Fold the audit procedure into A1. Known-defect repair remains diagnosing-bugs; code closeout remains verify. No new silent-data runner or tier ledger.

- Source family: `docs/audits/jit-aot-parity-2026-08-31/**`.
- [docs/audits/silent-data-sweep-2026-08-28.md](../audits/silent-data-sweep-2026-08-28.md)
- [docs/audits/datetime-and-tier-hardening-2026-08-28.md](../audits/datetime-and-tier-hardening-2026-08-28.md)
- [docs/audits/d-simd3-probe.md](../audits/d-simd3-probe.md)
- [docs/audits/tier-parity-architecture-2026-09-03.md](../audits/tier-parity-architecture-2026-09-03.md)
- [docs/audits/tier-parity-architecture-2026-09-03.html](../audits/tier-parity-architecture-2026-09-03.html)
- [docs/audits/compiler-tiers-explained-2026-09-03.html](../audits/compiler-tiers-explained-2026-09-03.html)

### Security, hostile-platform, and native-fixture evidence

Scan scope and reconciliation sections, detailed boundary labels and selected paths, hostile/native evidence limits, and profile record. Did not run a provider scan, hostile fixture, or native platform job.

**Reusable method:** Record source, control, sink, impact, precondition, and validation; distinguish source reconciliation from an independently completed scan; require exact platform and candidate identity; retain allow/refuse and recovery evidence.

**Disposition:** Do not add a security-scan wrapper. Existing security closure and executable contracts own scan mechanics. A4 may challenge an evidence claim without replacing the provider or qualification process.

- [docs/audits/security-deep-scan-2026-08-03.md](../audits/security-deep-scan-2026-08-03.md)
- [docs/audits/security-deep-scan-2026-08-03-full.md](../audits/security-deep-scan-2026-08-03-full.md)
- [docs/audits/security-deep-scan-2026-08-03-full-tower-control-plane.md](../audits/security-deep-scan-2026-08-03-full-tower-control-plane.md)
- [docs/audits/security-closure-2026-09-02.md](../audits/security-closure-2026-09-02.md)
- [docs/audits/card-1888-hardened-profile.json](../audits/card-1888-hardened-profile.json)
- [docs/audits/prove-hostile-matrix.md](../audits/prove-hostile-matrix.md)
- [docs/audits/process-session-compatibility-closeout-2026-08-23.md](../audits/process-session-compatibility-closeout-2026-08-23.md)
- [docs/spec/contributing/security-closure.md](../spec/contributing/security-closure.md)

### Same-candidate hardening qualification

Read the retained rig's four gates, ratified clean-window thresholds, denominator and identity rules, and value-consuming contract. These are historical method and decision evidence, not a new qualification run.

**Reusable method:** require one candidate to satisfy registry-derived Core conformance on every applicable tier, the ratified 14-day/10-million-case/100-mutations-per-callable differential window, eight fresh adversarial lanes, and zero known open P0s. Missing rows, stale identities, unowned exclusions, early stops, and assembled green fragments cannot qualify that candidate.

**Disposition:** do not create a security-scan or hardening wrapper. The existing qualification contract and its Tower owners retain the thresholds and executable gates; verify owns closeout. A1 can consume the conformance obligations, and a separately requested A4 can challenge a retrospective evidence portfolio. Neither may replace, weaken, or claim to have executed qualification.

- [docs/audits/hardening-rig.md](../audits/hardening-rig.md)
- [docs/audits/hardening-rig-prompt.md](../audits/hardening-rig-prompt.md)

### Foundations, trust, and owned foreign-source boundaries

Corpus index and selected substantive chapters, especially trust Q20–Q25, plus FFI boundary synthesis. Examined test-economics record handling and false-green controls as source, not an exercised result. Remaining chapter and representation files were inventoried.

**Reusable method:** State theorem and observation boundaries; separate optimization legality from profit; choose tests by distinct defect detection; preserve oracle independence, candidate identity, and missing obligations.

**Disposition:** Strongest basis for A4, with conformance and attribution evidence for A1/A2. Architectural proposals remain under existing first-principles and design skills.

- Source family: `docs/audits/jet-foundations-and-trust-2026-09-04/**`.
- [docs/audits/jet-foundations-and-trust-2026-09-04.html](../audits/jet-foundations-and-trust-2026-09-04.html)
- [docs/audits/ffi-owned-source-and-boundaries.md](../audits/ffi-owned-source-and-boundaries.md)
- [docs/audits/ffi-owned-source-and-boundaries.html](../audits/ffi-owned-source-and-boundaries.html)
- [tools/agent-eval/test-economics/test-economics.mjs](../../tools/agent-eval/test-economics/test-economics.mjs)

### Domain-foundations probes and library/Core attribution

Main synthesis and maintained method/history; substantive bridge, exact-numeric, real-time, AI, and embedded probe sections; result and gap/battery representations inventoried. Did not inspect every probe program or rerun the campaign.

**Reusable method:** Build the ordinary package first; distinguish library-buildable work, compiler defects, missing foundation mechanisms, bridge/target failures, and first-party battery scope. Group causes across domains without promoting every requested algorithm into Core.

**Disposition:** Deepen pragmatism-audit through A2. Keep campaign data and rationale in their current evidence homes.

- Source family: `docs/audits/domain-foundations-2026-09-02/**`.
- [docs/audits/core-absorption-survey-2026-08.md](../audits/core-absorption-survey-2026-08.md)
- [docs/audits/embedded-freestanding-profile.md](../audits/embedded-freestanding-profile.md)
- [docs/research/domain-foundations-method.md](../research/domain-foundations-method.md)
- [docs/research/domain-foundations-history.md](../research/domain-foundations-history.md)

### Domain matrix and developer-workload census

Global synthesis, method/conclusion sections from all eleven family syntheses, selected compressed census/claim/performance/mechanism records, and the retained distributed-systems report. Not all domain-level rows or raw external observations were reread.

**Reusable method:** Map concrete work to mechanisms and evidence; deduplicate shared mechanisms across domains; distinguish published peer numbers from matched Jet measurements; retain registry ownership and source mismatches rather than silently repairing history.

**Disposition:** Use A2 for causal attribution. Existing gauntlet and surface-frequency-audit own matched performance and public-code frequency. Do not create one skill per domain or restore a withdrawn domain-planning system.

- Source family: `docs/audits/domain-matrix-2026-09-02/**`.
- Source family: `docs/audits/domain-dx2/**`.
- [docs/research/dx/domain-registry.md](../research/dx/domain-registry.md)

### First-principles and whole-language refounding

Substantive synthesis, same-mechanism tables, beginner/expert ladders, named decision amendments, selected chapter conclusions, and retained experiment result schemas. HTML was inspected as source, not visually accepted. Prompts were treated as historical inputs.

**Reusable method:** Sweep the area and its missing forms; find one underlying mechanism; challenge it with extreme workloads; show before/after surfaces and explicit control; distinguish a proposed amendment from current law.

**Disposition:** Already covered by first-principles-audit, ontology/type audits, and architecture design. Do not copy a particular proposed architecture into a new method.

- Source family: `docs/audits/jet-refounding-2026-09-05/**`.
- [docs/audits/whole-language-frame.md](../audits/whole-language-frame.md)
- [docs/audits/whole-language-frame.html](../audits/whole-language-frame.html)
- [docs/audits/open-tables.html](../audits/open-tables.html)
- [docs/audits/syntax-lexical-space.html](../audits/syntax-lexical-space.html)
- [docs/audits/tier-live-dev.html](../audits/tier-live-dev.html)
- [docs/audits/verdict-loop.html](../audits/verdict-loop.html)
- [docs/audits/records-one-receipt.html](../audits/records-one-receipt.html)
- [docs/audits/shapes-one-fact.html](../audits/shapes-one-fact.html)
- [docs/audits/rights-one-row.html](../audits/rights-one-row.html)
- [docs/audits/claims-one-ladder.html](../audits/claims-one-ladder.html)
- [docs/audits/structure-program-is-a-value.md](../audits/structure-program-is-a-value.md)
- [docs/audits/developer-experience.md](../audits/developer-experience.md)
- [docs/audits/whole-language-audit-prompt.md](../audits/whole-language-audit-prompt.md)

### Persona, onboarding, and script-to-system journeys

Persona verdicts, first-success and recovery methods, and journey stages. Did not replay the terminal, browser, or user journey.

**Reusable method:** Measure first useful success separately from the later loop; preserve failure recovery and reversibility; distinguish a supported environment, usable output, and user preference.

**Disposition:** Existing persona-audit and rli5 cover the primary lenses. A3 adds controlled-trial rigor; A2 consumes workload friction where relevant.

- [docs/audits/persona-audit-2026-07-23.md](../audits/persona-audit-2026-07-23.md)
- [docs/audits/persona-audit-2026-08-14.md](../audits/persona-audit-2026-08-14.md)
- [docs/audits/beginner-onboarding-2026-08-24.md](../audits/beginner-onboarding-2026-08-24.md)
- [docs/audits/card-1415-script-to-system.md](../audits/card-1415-script-to-system.md)

### Diagnostic projections and assistive-access evidence

Read the retained audit's canonical report surface, fixture and source inventory, and explicit screen-reader evidence boundary. No diagnostic or screen-reader session was run.

**Reusable method:** trace one diagnostic meaning through human terminal, JSON, LSP, PTY, color policy, stream order, paths, and fix edits. Distinguish a checked-in projection fixture from an actual assistive-technology interaction; structured output or color compliance alone does not prove spoken order, usable navigation, or a successful repair.

**Disposition:** A1 owns a requested conformance check across diagnostic projections. Persona-audit owns an actual user interaction, with A3's controls when a trial is requested; rli5 remains the newcomer-reading lens. Keep one primary owner for the requested result. No new accessibility wrapper or renderer is proposed.

- [docs/audits/cli-diagnostics-accessibility.md](../audits/cli-diagnostics-accessibility.md)

### Frozen agent-workload conformance and comparison

Read the retained corpus's frozen contract, policy receipt, scoring rules, baseline identity, native OS matrix, and fail-closed gate. The report and selected record shapes were evidence of the method; no adapter or native matrix was rerun.

**Reusable method:** close the task denominator over a manifest and exact input/output checksum set; propagate one policy digest through receipts; compare exact cold and warm outcomes across required adapters; retain native-platform, authority, cleanup, and unavailable-metric obligations. Ambient execution cannot establish confinement merely by parsing hostile fixtures.

**Disposition:** the existing corpus owns its executable schema and scoring contract. Gauntlet owns requested matched-peer performance work; A1 may consume a bounded conformance result, and A4 may challenge a retrospective qualification claim. Do not turn this into a persona or benchmark wrapper, and do not substitute a historical corpus scoring rule for the current strict performance gate.

- [docs/audits/agent-workload-corpus.md](../audits/agent-workload-corpus.md)
- [docs/audits/agent-workload-corpus-report.tsv](../audits/agent-workload-corpus-report.tsv)

### Cold-agent and earned-preference experiments

Trial contracts, preregistration and stop rules, baseline/scoreboard schemas, selected r1 comparison records, r2 fixtures/schema, and the codegen experiment's method and conclusion. Not every participant transcript or raw result was reread. No participant was run.

**Reusable method:** Freeze behavior and assistance before execution; preserve participant identity; separate checker, semantic, hostile-input, and preference outcomes; keep missing execution distinct from a completed trial.

**Disposition:** Deepen persona-audit through A3 rather than create dogfood, cold-agent, and preference wrappers. A4 can challenge overclaimed evidence.

- Source family: `docs/audits/raw/**`.
- [docs/audits/dogfood-jet-experience-5-of-5.md](../audits/dogfood-jet-experience-5-of-5.md)
- [docs/audits/fresh-agent-5-of-5-rerun-2026-08-30.md](../audits/fresh-agent-5-of-5-rerun-2026-08-30.md)
- [docs/audits/fresh-agent-5-of-5-rerun-2026-08-31.md](../audits/fresh-agent-5-of-5-rerun-2026-08-31.md)
- [docs/audits/fresh-agent-5-of-5-rerun-2026-09-05.md](../audits/fresh-agent-5-of-5-rerun-2026-09-05.md)
- [docs/audits/cold-agent-jet-baseline.json](../audits/cold-agent-jet-baseline.json)
- [docs/audits/cold-agent-jet-scoreboard.json](../audits/cold-agent-jet-scoreboard.json)
- [docs/audits/agent-codegen-benchmark-2026-08-23.md](../audits/agent-codegen-benchmark-2026-08-23.md)
- [docs/proposals/dogfood-jet-experience-5-of-5.md](../proposals/dogfood-jet-experience-5-of-5.md)

### Practical dogfood, friction, and canonical-corpus adoption

Real-job findings, default/reject/override analysis, prevention reasoning, ledger contract, and selected records. Historical fixes and current card statements were not independently re-exercised.

**Reusable method:** Distinguish missing APIs from unused canonical features; find repeated user work; trace it to defaults, generators, or an actual semantic defect; prevent recurrence at the earliest appropriate layer without banning valid expert forms.

**Disposition:** Strong basis for A2. No recurring corpus-cleanup skill, blanket lint campaign, or new dogfood orchestrator.

- [docs/audits/dogfood-tower-2026-08-29.md](../audits/dogfood-tower-2026-08-29.md)
- [docs/audits/dogfood-jetpack-2026-08-28.md](../audits/dogfood-jetpack-2026-08-28.md)
- [docs/audits/dogfood-jetpack-usage-experience-2026-08-30.md](../audits/dogfood-jetpack-usage-experience-2026-08-30.md)
- [docs/audits/dogfood-jetpack-usage-experience-2026-08-30.html](../audits/dogfood-jetpack-usage-experience-2026-08-30.html)
- [docs/audits/friction-audit-2026-08-30.md](../audits/friction-audit-2026-08-30.md)
- [docs/audits/friction-audit-2026-08-30.html](../audits/friction-audit-2026-08-30.html)
- [docs/audits/devtools-dogfood-2026-09-03.md](../audits/devtools-dogfood-2026-09-03.md)
- [docs/audits/devtools-dogfood-2026-09-03.json](../audits/devtools-dogfood-2026-09-03.json)
- [docs/audits/example-corpus-modernization-2026-08-30.md](../audits/example-corpus-modernization-2026-08-30.md)

### Competitive, compiled-workload, build-time, and footprint evidence

Matched-work definitions, result/limitation sections, receipt identity fields, latency matrix distinctions, and compiled-workload source boundaries. No timings were reproduced or recalculated.

**Reusable method:** Compare complete equivalent work with matched source, compiler, target, inputs, outputs, and environment; preserve every required failure or losing cell; distinguish published peer evidence from a same-run result.

**Disposition:** Gauntlet already owns both run and build/update modes and absorbs the retired field-audit role. A4 may examine evidence adequacy, but no new performance skill is justified.

- [docs/audits/field-audit-2026-07-23.md](../audits/field-audit-2026-07-23.md)
- [docs/audits/field-audit-2026-07-31.md](../audits/field-audit-2026-07-31.md)
- [docs/audits/field-audit-python-2026-07-26.md](../audits/field-audit-python-2026-07-26.md)
- [docs/audits/gauntlet-2026-08-27.md](../audits/gauntlet-2026-08-27.md)
- [docs/audits/gauntlet-2026-08-27.html](../audits/gauntlet-2026-08-27.html)
- [docs/audits/gauntlet-2026-08-28.html](../audits/gauntlet-2026-08-28.html)
- [docs/audits/dev-dx-benchmark.md](../audits/dev-dx-benchmark.md)
- [docs/audits/compiler-latency-video-crosswalk-2026-08-24.md](../audits/compiler-latency-video-crosswalk-2026-08-24.md)
- [docs/audits/compiler-speed-check.context](../audits/compiler-speed-check.context)
- [docs/audits/compiler-speed-check.receipt](../audits/compiler-speed-check.receipt)
- [docs/audits/compiled-workload-gate.md](../audits/compiled-workload-gate.md)
- [docs/audits/performance-receipt.md](../audits/performance-receipt.md)
- [docs/research/compiled-workload-source-boundary-2026-09.md](../research/compiled-workload-source-boundary-2026-09.md)

### Package, environment, native-evaluator, and operating-system research

Alternatives, reusable source/authority boundaries, native-versus-index conformance distinction, licence limits, live/model/schema/fixture completion classes, and retained product rationale. Historical commands and product states were not exercised.

**Reusable method:** Separate a model or index from its consuming protocol; compare complete graph/output meaning rather than a convenient projection; isolate licence and product decisions from implementation evidence; show beginner and expert paths using one mechanism.

**Disposition:** Research, spec compliance, first-principles, and pragmatism already own the questions. Retain unique rationale; use selected boundary examples in A2/A4.

- Source family: `docs/proposals/jetpack/**`.
- Source family: `docs/proposals/jetos/**`.
- Source family: `docs/proposals/epoch-3/**`.
- [docs/audits/package-ecosystem-frameworks.md](../audits/package-ecosystem-frameworks.md)
- [docs/audits/ecosystem-shape.md](../audits/ecosystem-shape.md)
- [docs/audits/env-as-code-vs-declarative-2026-08-24.md](../audits/env-as-code-vs-declarative-2026-08-24.md)
- [docs/audits/jetpack-native-nixpkgs-2026-08-24.md](../audits/jetpack-native-nixpkgs-2026-08-24.md)
- [docs/audits/jet-nix-eval-snix-tvix-research-2026-08-24.md](../audits/jet-nix-eval-snix-tvix-research-2026-08-24.md)
- [docs/audits/snix-tvix-license-research-2026-08-24.md](../audits/snix-tvix-license-research-2026-08-24.md)
- [docs/audits/native-nix-evaluator-conformance-2026-08-24.md](../audits/native-nix-evaluator-conformance-2026-08-24.md)
- [docs/audits/2026-07-24-devenv-parity.md](../audits/2026-07-24-devenv-parity.md)

### Surface, mission, API-shape, and public-code placement studies

Historical verdicts, public-code corpus/method limitations, API-shape alternatives, and selected generator/source references. HTML content was inspected as source. Did not rerun the placement scanner or remeasure frequencies.

**Reusable method:** Use representative public code and explicit counting units; distinguish method-placement evidence from syntax preference; compare same-program alternatives; score mission alignment without turning a design preference into a current capability claim.

**Disposition:** Already covered. Do not create a colocation, optional-argument, or mission-wrapper skill.

- Source family: `docs/research/_scripts/**`.
- [docs/audits/surface-audit-2026-07-23.md](../audits/surface-audit-2026-07-23.md)
- [docs/audits/mission-audit-2026-07-23.md](../audits/mission-audit-2026-07-23.md)
- [docs/audits/2026-07-24-rust-struct-impl-colocation.md](../audits/2026-07-24-rust-struct-impl-colocation.md)
- [docs/audits/optional-args-stdlib-2026-08-28.html](../audits/optional-args-stdlib-2026-08-28.html)

### Repository hygiene, source history, and cleanup evidence

Published-tree versus dirty-tree method, source-preservation boundaries, ranked simplification findings, and navigation. No historical tree was reconstructed, report deleted, or cleanup applied.

**Reusable method:** Inspect what is actually published; distinguish retained rationale, executable truth, generated evidence, and obsolete work state; preserve unique source history while avoiding duplicate documentation.

**Disposition:** Existing ponytail-audit, structure-cleanup, and garbage-collection cover explicit requests. No recurring documentation janitor or report-conversion campaign.

- [docs/audits/repo-hygiene-audit-2026-09-07.md](../audits/repo-hygiene-audit-2026-09-07.md)
- [docs/audits/repo-hygiene-audit-2026-09-07.html](../audits/repo-hygiene-audit-2026-09-07.html)
- [docs/audits/repository-cleanup-review-2026-09-04.md](../audits/repository-cleanup-review-2026-09-04.md)
- [docs/audits/docs-cleanup-sweep-2026-07-25.md](../audits/docs-cleanup-sweep-2026-07-25.md)
- [docs/audits/ponytail-audit.md](../audits/ponytail-audit.md)
- [docs/audits/README.md](../audits/README.md)
- [docs/README.md](../README.md)

### Retained technical proposals and decision research

Proposal status, substantive alternatives, source and decision boundaries, preserved rationale, and selected acceptance/limit sections. Did not validate the proposed implementations or treat draft spelling as approved.

**Reusable method:** Keep source-backed alternatives and reasons distinct from ratification and execution; compare semantic obligations before performance; retain an explicit no-go or parked design without turning it into a method.

**Disposition:** Remain evidence and design rationale. Existing research, first-principles, codebase-design, and Tower ballot preparation own future requests.

- [docs/proposals/automatic-build-optimization.md](../proposals/automatic-build-optimization.md)
- [docs/proposals/self-hosted-optimizer-ssa.md](../proposals/self-hosted-optimizer-ssa.md)
- [docs/proposals/generic-modules.md](../proposals/generic-modules.md)
- [docs/proposals/stored-invariant-facts.md](../proposals/stored-invariant-facts.md)
- [docs/proposals/transactional-rollback-regions.md](../proposals/transactional-rollback-regions.md)
- [docs/research/cpp-interop.md](../research/cpp-interop.md)
- [docs/research/language-shape-research.md](../research/language-shape-research.md)
- [docs/research/proposal-decisions.md](../research/proposal-decisions.md)

### Visual acceptance and illustrative UI representations

File inventory, selected HTML source and the dated visual-acceptance receipt metadata, including its explicit supplied-status and unproved-build boundaries. PNG assets were not visually reviewed.

**Reusable method:** Bind a screenshot or interaction receipt to the exact artifact and state shown; separate standalone rendering, supplied status, actual compiler output, and end-to-end behavior.

**Disposition:** No visual-audit wrapper. Existing persona/UI verification owns interaction; HTML remains an explicitly requested representation. A3/A4 can use these evidence distinctions.

- Source family: `docs/proposals/visual-acceptance/**`.
- Source family: `docs/proposals/automatic-build-optimization/mockups/**`.

### Commercial research and market framing

Research frame, source and money-claim limitations, conclusion, and the distinction between a brief and a completed investigation. No external financial facts were refreshed.

**Reusable method:** Trace factual commercial claims to primary sources and separate market hypotheses from measured adoption or revenue evidence.

**Disposition:** Remain research and a historical brief. Two subject documents do not establish a recurring Jet-specific skill.

- [docs/audits/monetization-strategy-2026-08-06.md](../audits/monetization-strategy-2026-08-06.md)
- [docs/research/market-win-brief.md](../research/market-win-brief.md)

## Limits and owner choices

The family inventory is broader than the line-by-line reading. Large generated tables, raw shard outputs, every domain row, and every participant record were not exhaustively reread. HTML was inspected as source; image assets were inventoried and selected receipt metadata read, without a visual-acceptance run. External sources cited by historical reports were not independently re-mined. Historical recovery refs and forbidden personal scratch were not used to reconstruct a second archive.

Main-checkout reads were not an atomic clean-tree snapshot. The Tower Docs index and #3037 were read, not every Tower card or retired record. Host enforcement of skill metadata, model routing, and proposed trial controls was not tested. These limits prevent product-completion, benchmark, preference, or all-corpus-behavior claims; they do not prevent extracting the bounded procedures above.

The owner can select design + implementation, design only, or defer independently in each ballot named above. Each ballot carries its exact scope, acceptance criteria and stop line on Tower #3037. Tower owns all current work state; this report does not authorize execution.

**Strongest unverified assumption:** a standalone evidence-audit invocation is sufficiently more useful than bounded research plus the existing test-economics and verification references to justify another public skill. Its negative-control pilot should decide that before the route is accepted.
