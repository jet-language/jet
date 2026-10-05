# Ballot authoring details

Read this reference after the root skill identifies a genuine owner-only gate.
It records the required result without forcing a fixed campaign itinerary.

## Design away losses

Before recommending an option, attack each of its losses:

1. Ask whether a change removes the loss without adding another.
2. Borrow the useful part of another option or the strongest real-world design.
3. If the change works, update the option's code, gist, gains, losses, and long form; delete the removed loss.
4. If a small loss remains, record one concrete, beginner-checkable `mitigation` sentence that removes or minimizes it.
5. Repeat until losses are empty or negligible with a mitigation for each. If removing a loss is physically impossible, flag it in the summary and show how to minimize it.

The required result is fixed. The order and tools used to reach it are not.
A recommendation with an unattacked loss is still a draft. Keep at most two
negligible, concretely mitigated losses; if more remain, redesign or narrow the
choice. Explain costs shared by every option once as shared work. Keep decision
rationale and design changes in the ballot's `detail`, not a substitute card log.

## Research before drafting

Before drafting an unresolved owner choice, study how proven ecosystems and
systems solve the same problem. Start with primary specifications and official
documentation. Seek data where it exists: adoption, measured friction,
migration pain, and failure post-mortems.

State what each number measures. Downloads and stars measure a tool, not one
feature. Code-search counts measure indexed matches, not users. Record query,
date, exclusions, and known limits. Never present a repository census as market
evidence or invent data that is unavailable.

Record proven right ways and wrong ways, sources, and what went wrong in
`detail`; use `technical` for implementation facts. Combine the best approaches
into the no-compromise recommendation: design away avoidable costs instead of
offering a known-bad trade-off. The short surface must explain why it wins,
which known mistakes it avoids, and why the genuine alternatives are worse,
without showing the evidence machinery.

A choice settled by ratified law or an implementation detail is not a ballot:
record "settled by D-XXX" or "implementation choice" in its proper work context.
Do not research or sell a new owner choice where none exists.

## Full-profile review handoff

A full ballot needs two independent fresh readers. Use the canonical
[ballot review mechanics](../../../../../.agents/skills/orchestration/references/ballot-review.md)
for dispatch, complete-ballot input, RLI5 tasks, receipt shape, and provenance.
The beginner pass invokes `/rli5` and attempts explain, predict, modify, and
derive tasks, returning a friction table. The adversarial pass attacks the
recommendation, evidence, failure modes, and every `mitigation`.

Neither reader may have authored the ballot or performed the other pass. A
model-family change is optional; freshness and separate agent IDs are required.
After both receipts, repair material findings, re-run the loss checks, and
ensure `recommendation.whyNot` names every losing option. A short ballot ends
after the complete base draft and design-away result; it has no review passes.
