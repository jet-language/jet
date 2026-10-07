---
name: mine-for-jet-v2
description: >-
  Deep, state-first mining of named external sources for Jet: exhaustive micro
  and macro extraction, canonical alignment, and independent review before publication.
---

# Mine for Jet v2

## Purpose and completion bar

Jet's goal is to be the **ONLY programming language anyone ever needs**, at every
level, for every domain, at any time, in any scenario, for every level of
expertise. Every mining run serves that goal, aligned with
`Docs/spec/philosophy.md`: guide people to learn and become better programmers
while writing better, safer, more performant code that is easier to reason about
and maintain. Read that document for ranked trade-offs; this purpose does not
replace it or authorize another strategy document.

**Depth, micro and macro, exhaustive:** mine deeply and thoughtfully, never a thin
surface pass or “one relevant thing per source.” Extract **all concepts that could
improve Jet**, at every scale: constructs and semantics; features and APIs; idioms
and teaching/learning paths; diagnostics and tooling; systems and subsystems;
architecture and infrastructure (build, packaging, runtime, compiler pipeline,
concurrency, memory, IO, deployment, debugging, observability); ecosystem and
process. Classify each contribution as **adds-new, enhances-existing, fills-gap,
fixes-issue-or-misalignment, confirms-choice, or refutes-choice**. A confirmation
needs a concrete mechanism and witness, not praise.

**Depth is enforced, not asserted** ([depth contract](references/depth.md)).
Split every source into bounded segments (video/audio ≤ 5 min; text ≤ 1,500
words within one section; repository = one module/directory). While reading each
segment, write its **concept ledger**: every concept, technique, mechanism,
claim, demo, measurement and caveat it presents. Map every ledger item to its own
claim, or to already-covered (with the Jet ref) or out-of-scope (with a specific
reason). Fill a segment × lane matrix with claim IDs or concrete
segment-specific reasons. `scripts/check_coverage.mjs` validates all of it,
reports ledger/claim densities, flags thin segments for written justification,
and must exit 0 before hand-in. Exhaustiveness covers the declared corpus, not
an unbounded linked-source crawl. Never silently narrow a broad request, hide
findings behind a cap, or stop at attractive language spellings.

**Orchestration:** legwork agents produce captures and segment indexes only;
the thinking model reads every body and writes ledgers, claims, lane rows and
alignment. Bound each miner's batch in hours of source it can read in full,
ledger written while reading, within one working context; split by segment
range rather than skim.

**Ballot quality is non-negotiable:** before drafting, reconcile current, planned
(including ratified-unbuilt), in-flight, and proposed Jet state via the digest.
Design downsides away; redesign until the recommendation has at most **two real
residual losses, each concretely mitigated**. Aim for a researched, best-of-all-worlds,
no-compromise design under rule 9: combine demonstrated strengths without
surrendering Jet's governing outcomes; never claim literal perfection or conceal
necessary trade-offs. The short surface must sell that design through its useful
observable result. Ballots failing this bar are **rejected before posting** by
the required fresh-context reviewer. No “slop at the wall”: fewer excellent ballots
beat many weak ones. Findings needing no owner choice attach to existing work
as cards or evidence, not new votes.

## Scope and permission

Name source URLs/paths, versions, kind, Jet subject, workload, supplied owner
questions, and requested outcome before capture. Read `AGENTS.md`,
`.agents/skills/_shared/audit-dispositions.md`, and relevant authoritative slices.
Use the shared standing lens contextually; do not manufacture a competitive
study for a focused question. No unrequested implementation or extra sources.

Normal mining retains its method-owned Tower obligations **only after alignment
and independent review**. Explicit report-only is read-only with respect to
Tower; chat-only remains chat-only. Repository publication requires permission.
Workers never write Tower: return reviewed payloads to the orchestrator for
supported CLI publication and readback. A recommendation does not authorize coding.

## Establish Jet state before extraction

Run in the exact checkout, through `Tools/agent/jet-env`:

```sh
Tools/agent/jet-env node Tools/agent/jet-state.mjs --subject 'SUBJECT' --format markdown
```

Use JSON for complete candidate rows; the compact text view is a bounded preview.
Read the latest owner direction separately, then relevant winning decisions and
acceptance terms, cards and phases, dev-versus-master and named-worktree changes,
research/proposals, and registry spellings. Missing providers are **unknown**, not
absence. The digest is candidate retrieval, not semantic alignment or runtime
proof. Inspect focused code/examples and related branch hunks; do not resurrect
a retired implementation because it is easier to probe. Record digest identity
and direct refs in claims. Reinvoke the relevant subject immediately before any
authorized publication if board or tree identity changed; inconsistent digests
must be refreshed.
See [digest inputs, receipt contract, and freshness](references/jet-state.md)
when a provider or identity needs explanation.

Keep **three independent axes** per claim: observed implementation; intended
owner decision; work/proposal state. Ratified-unbuilt is explicit. Ready is not
in-flight; done is not built; proposal-only done is still a proposal. A defect
under ratified law is repair, not a new design vote. Date sorting alone does not
supersede a decision.

## Source identity and efficient capture

Check the source registry before costly retrieval:

```sh
Tools/agent/jet-env python3 .agents/skills/mine-for-jet-v2/scripts/check_sources.py \
  --registry Docs/spec/reference/prior-art.md 'SOURCE_URL'
```

The existing checker conventions remain: `new`, `tracked`, `duplicate-input`;
exit 3 for duplicate/unapproved tracked input; exit 4 for untracked
`--verify-tracked`. Explicit approved reruns use `--allow-rerun CONTENT_ID`
(the source ID when there is no content selector).
An optional `--evidence-index DIGEST_JSON` joins retained report/source leads.
Tracked does not mean fully mined; new does not prove never mined. Preserve
historical source IDs separately from content selectors, revision and subresource.
Never silently remap historical identities. Playlist expansion needs explicit scope.

Load [capture](references/capture.md) and only relevant source-kind branches.
All disposable captures go under **`$JET_SCRATCH_ROOT/mine/<run-id>/`**; set
`JET_SCRATCH_ROOT` explicitly (default `~/.cache/jet-dev/scratch`) and pass the
location to commands. Never repo `target-mine-*` directories or `/tmp`. Process
**transcripts/text first**. Retrieve
frames only when on-screen code, diagrams, or results matter to a claim; use
localized timestamps, not a full-video frame sweep. Read bounded text chunks,
reuse captures, and use repository scripts rather than per-run ad-hoc Python.

## Extract, align, review, publish

1. Follow [review](references/review.md) and [depth](references/depth.md):
   segment ledgers and lane matrix, owner-question matrix, normalized claims,
   seven hard gates, dedupe and value ranking. Relevant audience, live-probe and
   multi-source lanes remain conditional; no mandatory real-user study. Hand in
   only when the gate passes:

   ```sh
   Tools/agent/jet-env node .agents/skills/mine-for-jet-v2/scripts/check_coverage.mjs "$CAPTURE_ROOT"
   ```

2. Obtain the **required fresh-context reviewer** after extraction/dedupe and
   before publishing any new recommendation/card/ballot. It first runs the
   **recall audit** (re-mines a seeded sample of segments from the raw body; any
   miss sends the batch back for re-mining) and `check_coverage.mjs --audit`
   must exit 0. Use existing review and tower-ballot skills as applicable, not
   another reviewer catalog. Return a bounded review brief to the orchestrator
   if this host cannot delegate. No fresh reviewer means publication is
   blocked, not waived.
3. Resolve substantive reviewer findings; dispositions are keep,
   attach-existing, redesign, reject-conflict, evidence-gap, or undecided.
4. Follow [output](references/output.md) for compact synthesis, retained exact
   evidence, `audit-dispositions:v1`, and authorized Tower obligations. Retain
   normalized claims and essential reproducible witnesses with the report;
   scratch-only JSON is not durable evidence. Remove disposable captures only
   after that evidence is retained.

Completion requires all declared sources, segments, lanes and owner questions
accounted for (gate and recall audit at exit 0), claims evidenced or explicitly
unknown, quality gates and reviewer findings resolved, and authorized
artifacts/payloads handed off. Review is not
ratification or runtime verification. Do not begin unrelated implementation.
