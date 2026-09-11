# Domain documentation

Jet's current terminology lives in `docs/spec/vocabulary.md`. Ratified Tower decisions and their acceptance terms govern the corresponding domain specs; `docs/spec/syntax-decisions.md` records the language rulings.

## Before changing a domain concept

1. Search the vocabulary and relevant spec for the existing term.
2. Read the governing Tower decision when meaning or scope is disputed.
3. Use that term consistently in code, tests, examples, cards, and proposals. Do not introduce a synonym for an established concept.
4. For an actual vocabulary gap, use `domain-modeling` and update the existing vocabulary or topical spec. Do not create a parallel glossary or generic context/ADR layout.

Read `docs/spec/contributing/examples.md` when the change affects executable examples.

## Conflicting decisions

Name the decision, the conflicting behavior, and the reason for reconsidering it. Never silently override a ratified ruling. Raise a real owner choice through Tower; after ratification, update the existing spec and all affected consumers.

