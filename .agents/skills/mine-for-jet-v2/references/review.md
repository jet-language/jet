# Extraction, alignment, and independent review

## Read and account for the declared corpus

Read the source body in bounded timestamp, section, file/directory, or post
chunks. Reconstruct mechanisms, thesis, causal chain, measurements, fixes,
caveats, refusals, failures and unresolved questions. A README/abstract or title
is not body review. Demos are primary surface evidence: retain essential exact
commands, snippets, outputs and diagnostics with locators. Author commentary,
quoted material, linked-source evidence and inference stay separate.

For **each source**, follow the [depth contract](depth.md): bounded segments,
a concept ledger written while reading each segment and fully mapped, and a
segment × lane matrix in which every row carries claim IDs or a concrete,
segment-specific reason. An unread or blocked body is a coverage gap, not “not
applicable”; do not call that source complete. Retrieval, body review and
extraction stay distinct fields. `check_coverage.mjs` must exit 0 before hand-in.

## Extraction lanes: micro and macro

| Lane (`slug`) | Inspect | Preferred consequence |
| --- | --- | --- |
| Mechanism and architecture (`mechanism-and-architecture`) | Fact ownership, representation, demand, phases, caching/invalidation, ABI, control flow; systems/subsystems and compiler/runtime/infrastructure | Sharpen existing owner/acceptance, not a second subsystem |
| Default/common job (`default-common-job`) | First run, complete ordinary tasks, invisible obligations, failure/recovery, build/packaging/deployment | Useful safe defaults and lower repeated reasoning cost |
| Semantic obligations (`semantic-obligations`) | Constructs, ownership/aliasing, cancellation, concurrency/ordering, cleanup, memory/IO, errors/absence, exact values | Law-preserving counterexample or completion |
| Expert descent (`expert-descent`) | Allocation/placement, target/profile, inspectable costs, debugging/observability, explicit opt-out/refusal | Expert control over the same mechanism |
| Cost and measurement (`cost-and-measurement`) | Whole-job time/bytes/allocations, cold/warm, crossover/scaling, host/toolchain identity | Bounded hypothesis through the canonical proof gate |
| Proven experience (`proven-experience`) | Adoption/migration, ecosystem/process, support burden, post-mortems and corrected designs | Rule-9 evidence for the boundary |
| Interop and portability (`interop-and-portability`) | Host authority, packaging, foreign lifetimes, reproducible inputs, platform refusals | Complete boundary contracts, not happy-path adapters |
| Audience corrections (`audience-corrections`, conditional) | Verified corrections, specific repeated pain, workarounds, unknown prevalence | Counterexamples/leads, not vote totals |
| Reuse and anti-lessons (`reuse-and-anti-lessons`) | Existing law, structural immunity, mismatched jobs, rejected approaches; idioms/teaching and learning paths | Concrete confirmation/refutation or attach/no-action |

Account for features/APIs, diagnostics/tooling, learning paths and process as
well as syntax, at the applicable micro and macro scales. Classify each
contribution: adds-new, enhances-existing, fills-gap,
fixes-issue-or-misalignment, confirms-choice, or refutes-choice.

## Owner-question matrix

Preserve every supplied question/comment; map it to a source locator, direct
Jet-state evidence and answer/disposition. Generic victory prose satisfies none.
For an authorized second pass label new mechanism, corrected first-pass claim,
or already-covered item, preserving the previous locator and correction.

| Exact owner question/comment | Source locator | Jet authority/current/work witness | Answer/disposition | Second-pass delta |
| --- | --- | --- | --- | --- |

## Conditional evidence lanes

Audience review runs only when requested or materially necessary. Sample
high-reaction roots, recent roots, low-visibility technical items, substantive
replies and corrections; preserve root/reply boundaries and retrieval gaps.
Keep participants anonymous unless maintainer identity affects credibility.
Likes/stars/counts discover leads; they never decide technical truth or consensus.
Verify corrections against primary evidence; report contradictions honestly.
A real-user lane is conditional, not a mandatory rejected user-study requirement.

Live contrasts run only when relevant and authorized. Record binary/tree,
backend/tier/profile, input, command, output, exit code and oracle. Rebuild before
compiler smoke runs under repository proof rules. If execution is unavailable,
report a source-reading candidate, never a freshly observed defect. Performance
claims need matched whole-job measurements and the existing strict gate; do not
mix incremental/clean, CPU-summed/wall, debug/release, or imply phase attribution
without phase data.

Multiple sources keep distinct records. Repeated corroboration requires two
independent upstream identities, not mirrors/repeated speaker claims. Group by
reviewed shared topic, not similar glyphs. Resolve conflicts by reading primary
bodies, not count/confidence/popularity; merge actions only when the semantic
owner, job and acceptance match. Linked primary material is bounded corroboration
for load-bearing claims, not permission to crawl an ecosystem.

## Normalized claim record

Write claims under the run capture root while extracting, then retain them with
the authorized report. Parse JSON; reject unknown enums; make topic identities
intentional. A claim keeps source ID/kind/ref, upstream `source_identity`, content
selector/revision/subresource, retrieval date, exact locator, claim, confidence
(low/medium/high), stance (supports/disputes/neutral), correction and inference.
Source kind: primary, audience, linked-source, local-evidence, or inference.

Add `segment` (coverage segment ID), `lane`, `scale` (micro/macro), `workload`,
`contribution`, coverage/measurement limits, `canonical_owner`, `jet_state`
(digest identity and exact refs), independent `observed`, `intended`, `work`
axes, reviewer disposition, and why the contribution is genuinely incremental.
Each distinct contribution is its own claim; several ledger items share a claim
only when they witness the same mechanism, never to summarize a segment.
Example shape, not invented current Jet evidence:

```json
{
  "id": "example-cache-03",
  "topic": "cache-input-identity",
  "claim": "The measured cache key includes profile inputs.",
  "source_id": "example.org/cache",
  "content_id": "example.org/cache?edition=2",
  "source_kind": "primary",
  "source_ref": "https://example.org/cache?edition=2",
  "source_identity": "example.org/cache",
  "version": "edition=2",
  "locator": "section 3, table 2",
  "segment": "example.org/cache/03",
  "retrieved_at": "DATE",
  "confidence": "medium",
  "stance": "supports",
  "correction": null,
  "inference": "Jet consequence needs focused source review",
  "lane": "mechanism-and-architecture",
  "scale": "micro",
  "workload": "incremental builds",
  "contribution": "enhances-existing",
  "coverage_limits": "Cold builds not measured",
  "canonical_owner": null,
  "jet_state": { "digest_identity": "HEAD + status hash + Tower revision", "refs": [] },
  "observed": "unknown",
  "intended": "unknown",
  "work": "unknown",
  "review_disposition": "evidence-gap",
  "incremental_contribution": "Candidate hostile invalidation witness; ownership unresolved"
}
```

Enums are exact slugs (put qualifications in `coverage_limits` or `inference`).
Observed: `present` (registry/code present), `exercised` (at matching identity),
`partial`, `contradictory`, `unknown`. Intended: `ratified`, `undecided`,
`rejected-conflicting`, `retired-superseded`, `unknown`. Work: `none` (no record
found by a working provider), `planning`, `ready`, `building-verify`, `frozen`,
`done`, `proposal-only`, `open-ballot`, `draft-ballot`, `unknown`. Review
disposition: `keep`, `attach-existing`, `redesign`, `reject-conflict`,
`evidence-gap`, `undecided`. Preserve raw Tower phase in `jet_state.refs`;
ratified-unbuilt is a planned consequence, not a new owner vote. Done can mean
proposal-only; historical receipts do not prove today's dirty tree. These are
dated evidence, never a mutable second work ledger.

## Seven hard gates, before ranking or drafting

1. **Source:** exact source/version and load-bearing body actually reviewed?
   Navigation/title/abstract-only stops at coverage-gap.
2. **Authority/current owner:** identify active implementation and exact law;
   reconcile latest direction and explicit supersession. Dates alone do not
   amend unrelated decisions. An unrecorded-chat gap stays unknown.
3. **Duplicate ownership:** search ratified decisions, open ballots, cards,
   active changes and proposals by topic-equivalent names, source IDs/URLs,
   report finding IDs, intended action/job and acceptance, not titles alone.
4. **Canonical Jet:** check active syntax/marker/lexicon registries and executable
   examples. Prior-art prose and generated backend code are not Jet idioms.
5. **Observed defect:** matching binary/tree witness? Otherwise source-reading
   candidate/unavailable execution; never infer absence from a keyword scan.
6. **Coherence/boundaries:** preserve one meaning, safe useful defaults, expert
   descent, portability and all required tiers. Another validator/data tree/
   effect system or obsolete architecture is reject/redesign, not a feature.
7. **Real owner choice:** ratified consequences and implementation choices
   attach to existing owners. An external name alone never justifies a ballot.

## Value ranking

Rank survivors by governing owner request/obligations, then severity and
whole-job user value, common-job frequency/reach, evidence strength, semantic
reuse, and proof feasibility. Separate confirmed high-value repair, useful
completion under existing law, bounded research hypothesis, and real new owner
choice. A high-impact uncertain hypothesis may outrank low-impact confirmed
work but remains a hypothesis. Never reward novelty, clever syntax, popularity,
card count or fabricate numeric scores/universal ecosystem weights.

## Rule 9 and recommendation design

Before any card/ballot draft, research proven ecosystems solving the same job:
primary design/implementation docs, available measured friction/adoption/
migration evidence, failures/post-mortems. Follow two credible approaches where
available; record what worked, failed, why and applicability in detail/technical.
Missing data is an explicit gap; mechanism proof is not user testimony. A token
`inWild` snippet is inadequate.

Redesign through existing semantics/types/authority to eliminate avoidable
obligations. Combine demonstrated strengths and avoid proven mistakes: a
best-of-all-worlds, no-compromise design for Jet's governing outcomes, not literal
perfection. The recommended short surface has at most two genuine residual
losses with concrete mitigations. More losses require redesign or visible owner-
approved narrowing, never silently reduced scope. Shared work/inherent problem
constraints are explained once, not padded as option-specific downsides.

Short surface: at most four plain situation sentences and approximately 250
words total; no paths/decision IDs/internal jargon. At most three real legal
alternatives. Use the same tiny job/input with before/after per option (at most
six lines), observable output/failure/ownership outcome and first-use definitions.
Long runnable examples and provenance stay in technical detail. Explain honestly
why alternatives are worse; no already-illegal straw men. Sell the recommendation
by showing the ordinary user's benefit, not hype.

Cards own one coherent end-to-end deliverable, canonical semantic owner/decision,
concrete job/failure witness, observable acceptance, dependencies and actual
remaining gate. Reuse an existing matching card. Group incidental findings only
when deliverable and proof match; independent owner choices stay separate.
Include callers/tests/docs migration and deletion of obsolete paths for approved
clean cutovers. Evidence/research work does not imply implementation.

## Required fresh-context reviewer: before publication

After extraction/dedupe and a passing `check_coverage.mjs`, supply an independent
reviewer with declared sources, raw captures, the run directory (coverage,
ledger, claims, checker output), question matrix, direct authority and
checkout/digest identities, duplicate search, proposed dispositions, and
short/technical payloads. Use existing review/tower-ballot skills as applicable;
an orchestrator obtains the reviewer when workers cannot delegate. Without fresh
review, do not publish new recommendations/cards/ballots.

**Recall audit first.** The reviewer checks recall, not only ballot quality:
it draws the seeded sample (at least 2 segments or 10% per source), re-mines
each sampled segment from the raw transcript/body before reading the miner's
ledger, and records the result in `recall-audit.json` per the
[depth contract](depth.md#5-recall-audit-recall-auditjson). Any missed concept or
disputed mapping fails the batch: it returns to the miner for re-mining of every
segment, then a new audit with a new seed. `check_coverage.mjs RUN --audit` must
exit 0 before ballot review continues; the report records every audit's seed,
sample, misses and outcome.

Combine two bounded lenses when appropriate:
- Authority/mechanism: law/supersession, implementation/runtime identity, active
  branch, duplicate owners, false premises, source independence, counterexamples,
  honest performance limits and canonical idioms.
- Newcomer/decision: current problem and option differences understandable from
  short surface alone, same-job idiomatic examples, first-use definitions, genuine
  mitigated losses, convincing useful recommendation.

**Reject before posting**: thin surface mining; a failing coverage gate or recall
audit; any segment/lane unaccounted for; unanswered owner question;
missing current/planned (including
ratified-unbuilt)/in-flight/proposed reconciliation; unsupported rule-9 claims;
non-best-of-all-worlds recommendations that surrender governing outcomes;
more than two residual losses or any unmitigated loss; weak/jargon-heavy short
surface that does not demonstrate/sell the useful result; fabricated evidence;
duplicate/illegal owner choice; obsolete architecture; treating approval/review
as exercised proof. Attach no-choice findings to existing work/evidence instead
of generating low-quality ballots.

Dispositions: keep, attach-existing, redesign, reject-conflict, evidence-gap,
undecided. Resolve all substantive findings before handoff and retain corrections.
A reviewer/schema pass is neither ratification nor runtime verification.
