# Ballot clarity

Apply these owner rules to every new or edited ballot, alongside the schema and
validator. They take precedence over expansive prose; keep every required field.

1. **Does the owner need to decide?** If ratified law implies the answer or the
   choice is implementation under an approved contract, do not ballot it.
   Record “settled by D-XXX” or “implementation choice” in its work context.
2. **Situation: at most four short sentences, plain words.** Say what a Jet
   programmer sees today, why it is a problem, and what the vote decides. No
   compiler-internal names, file paths, or decision IDs in situation, gist, or
   story.
3. **Show, do not describe.** Give one tiny before/after code example per option,
   at most six lines per snippet, readable by a newcomer. Explain each needed
   term the first time it appears, in plain words.
4. **Design downsides away.** The recommendation keeps at most two losses, each
   negligible and concretely mitigated. If more remain, redesign: narrow scope,
   separate a later step, or reuse an existing mechanism. Explain shared costs
   once as shared work, not as losses of one option.
5. **At most three real choices.** No straw men. Each alternative's `whyNot`
   gives one honest line explaining why it loses.
6. **Keep evidence out of the way.** File:line facts and cited decisions belong
   in `detail` and `technical` only. Situation, gist, story, option names and
   gists, and recommendation must read without them.
7. **Around 250 words for the short surface.** If more are needed, split or
   narrow the question; schema maxima are not a writing target.
8. **Self-check before handing in.** Read the short surface as someone who has
   never seen Jet internals. Rewrite any sentence needing outside knowledge.
9. **Research first, then sell the best-of-all-worlds answer.** Study how proven
   ecosystems and systems solve the same problem, with adoption, measured
   friction, migration pain, and failure post-mortems where data exists. Record
   proven right and wrong ways, evidence, and what went wrong in `detail`.
   Recommend the no-compromise design combining the best approaches, backed by
   experience and evidence. Make the short surface convincing: why it wins,
   which known mistakes it avoids, and why the alternatives are worse.

Use [authoring details](authoring.md) for research and loss redesign, and the
[root skill](../SKILL.md) for the complete short/full submission contract.
