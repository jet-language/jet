---
name: research
description: Investigate a question against high-trust primary sources and capture the findings as a Markdown file in the repo. Use when the user wants a topic researched, docs or API facts gathered, or reading legwork delegated to a background OMP task.
---

## Contract

- **Requested outcome:** In a standalone run, one cited Markdown result that answers the supplied research question; in a bounded caller handoff, cited findings returned to the caller's owned result.
- **Supplied inputs:** The question, source boundaries, repository conventions, and any existing report location; a bounded caller also supplies its output boundary.
- **Allowed child result:** One bounded OMP evidence-gathering task may return primary-source claims and links. It cannot open an undeclared agenda, change the question, or start implementation.
- **Completion owner:** `research` owns source checking and standalone synthesis; the caller owns synthesis and output when research is a bounded support handoff.
- **Return point:** Evidence returns to research synthesis before a standalone result is saved, or directly to the bounded caller before it completes its owned result.
- **Stopping condition:** A standalone run stops when every reported claim is traced to its source and the single result is saved; a bounded handoff stops when cited findings return to the caller without a required second artifact or follow-on workflow.

## Retrospective evidence-adequacy branch

Use this branch only when the standalone question is what a supplied,
unchanged-tree portfolio of claims, manifests, receipts, scans, or benchmark
records actually proves. It produces a claim-to-evidence adequacy report, not a
code, card, milestone, security, competitive, or release verdict. A request
for current change closeout routes to `verify`; a ratified-law comparison to
`spec-compliance-audit`; a fixed-diff review to `code-review`; a security
qualification to its existing security-closure procedure; and a matched-peer
qualification to `gauntlet`. If the request asks for one of those outcomes,
that owner takes precedence even when records are supplied.

The inputs are the claim set, governing contracts, exact program/build/tool/
input identities, raw unedited records, producer and checker identity, and
existing plausible failure controls. For every claim, map the observation to
the promised behavior, inspect the independent expected-answer source, and
retain missing obligations. Use existing test-economics normalization where it
applies. A copied representation is one observation, not independent
agreement; a failed or unavailable check is a non-result.

Use this fixed six-record exercise without giving the reviewer the answer
labels. The controller keeps the expected dispositions separately and hides
only display labels:

| Record | Identity and observation | Adequacy disposition |
| --- | --- | --- |
| R1 | Matching `P1/B1/T1/I1`, raw stdout `2\n`, independent minimum of `[2, 10]` is `2` | Supports this narrow claim only |
| R2 | Old `P0/B0/T1/I1`, raw stdout `2\n` | Stale for `P1`; supports a separately identified P0 claim only when every identity, input, expectation, and raw record matches |
| R3 | A copied representation of R1 | Count once as R1, not as a second run |
| R4 | The test tool crashed before completion | Failed attempt only; no correctness support |
| R5 | The requested path was unavailable and never ran | Unmeasured, not passing |
| R6 | Matching `P1/B1/T1/I1`, raw stdout `10\n`, checker passes because its expected-answer source incorrectly says `10` | Reject the correctness claim and identify the broken oracle |

The numeric expectation is independently established: the minimum of `[2, 10]`
is `2`. Preserve the narrow support in R1 even when R6 blocks a broader claim;
do not average a wrong green result with a valid observation. A stale record
can support an older claim only when that separate claim is named and all
required evidence matches. Keep exact identities and record contents immutable
for the bounded exercise. Do not create a maintained fixture suite, runner,
schema, second report owner, security bypass, or release approval.

Stop after the six records have dispositions and the requested route boundary
is explicit. Recommend a narrower claim or stronger observation when needed,
but do not modify the implementation or silently turn retrospective research
into another workflow.

For a standalone run, if the host supports child tasks and delegation helps,
submit one **background OMP task** under `AGENTS.md`;
otherwise do the research in the current context. For a bounded caller
handoff, return cited findings to the caller's output boundary instead of
requiring a separate research artifact. The child returns evidence only; keep
the research workflow and the applicable output contract below.

Its job:

1. Investigate the question against **primary sources** — official docs, source code, specs, first-party APIs — not a secondary write-up of them. Follow every claim back to the source that owns it.
2. For a standalone run, write the findings to a single Markdown file, citing
   each claim's source. For a bounded caller handoff, return those cited
   findings to the caller instead of creating a required second file.
3. For a standalone run, save the file where the repo already keeps such notes;
   match the existing convention, and if there is none, put it somewhere
   sensible and say where.
