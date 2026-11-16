# Fresh ballot review

Use this reference only for a new, draft, or updated **full ballot**. It does
not replace ordinary criterion proof or the milestone review in
[`closeout.md`](closeout.md).

A full ballot needs one fresh adversarial pass before it is marked ready.

## Adversarial pass

Assign a fresh agent that did not author the ballot or review an earlier pass
of it. Give that reviewer the complete ballot, not a summary, and ask it to
attack the recommendation, alternatives, assumptions, edge cases, trade-offs,
and beginner/expert paths. Record actual reviewer identity and model-family
provenance without requiring a different provider or naming a specific model.
The first line of `reviewPasses.adversarial` uses this exact prefix:

```text
Author model family: author-family. Adversarial model family: adversarial-family. Fresh agent: reviewer-id. The adversarial review attacked the recommendation.
```

Follow the owner's requested reviewer when one exists; otherwise use the
full-review role in `AGENTS.md`. Revise the ballot from material findings.
Once ratified, the decision is immutable history rather than an invitation to
repeat the review.

A ballot must still state the owner-only choice, same-program alternatives,
exact syntax and behavior, trade-offs, edge cases, and beginner and expert
paths. If it includes visual or DX direction, state the observable owner
acceptance separately; machine evidence and visual taste are not interchangeable.
