# Domain documentation

Use this guide when a change introduces or changes a concept that appears in
multiple parts of Jet. The canonical vocabulary is
[`docs/spec/vocabulary.md`](../vocabulary.md); language rulings are in
[`docs/spec/syntax-decisions.md`](../syntax-decisions.md), and the affected
topical spec is the source for that domain's contract. The
[`domain-modeling` skill](../../../.agents/skills/domain-modeling/SKILL.md)
records an agreed term or ADR; Tower remains the work ledger.

## Before changing a concept

1. Search the vocabulary and the relevant spec for the existing term.
2. Read the governing Tower decision when meaning or scope is disputed.
3. Use the established term consistently in code, tests, examples, cards, and
   proposals. Do not introduce a synonym for an existing concept.
4. If the vocabulary has a genuine gap, use `domain-modeling` and update the
   existing vocabulary or topical spec. Do not create a parallel glossary or a
   generic context/ADR layout.

When the change affects executable teaching material, also read
[`examples.md`](examples.md) and trace the example's tests and expected output.

## Record a conflict explicitly

Name the decision, the conflicting behavior, and the reason to reconsider it.
A ratified ruling is not silently overridden by a local implementation or a
new word. Raise a genuine owner choice through Tower. After ratification,
update the existing spec and every affected consumer in one cutover.

Keep the explanation focused on the durable meaning and its reason. Put plans,
dependencies, acceptance criteria, and delivery state on the owning Tower card,
not in this document.
