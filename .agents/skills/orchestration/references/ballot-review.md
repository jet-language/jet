# Fresh ballot review

Use this reference only for a new, draft, or updated **full ballot**. It does
not replace ordinary criterion proof or the milestone review in
[`closeout.md`](closeout.md).

## Beginner pass

Assign a fresh true-beginner agent that did not author or review an earlier pass
of the ballot. Provide the complete ballot, not a summary, and invoke `/rli5`.
Require all five observations:

- explain what the ballot decides and why;
- predict the result of each concrete alternative;
- modify the smallest example or choice;
- derive the rule from the stated facts; and
- return a friction table of terms, missing context, and misleading or
  unexplained steps.

Record this exact prefix in `reviewPasses.beginner`:

```text
Fresh agent: agent-id. Skill: rli5. The beginner pass tested the complete ballot.
```

## Adversarial pass

Use a separate fresh agent that neither authored the ballot nor performed its
beginner pass. Give that reviewer the complete ballot and the beginner findings
and ask it to attack the recommendation, alternatives, assumptions, edge cases,
trade-offs, and beginner/expert paths. Record actual reviewer identity and
model-family provenance without requiring a different provider or naming a
specific model:

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
