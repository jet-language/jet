---
name: eli5
description: >-
  Explain a topic for a curious beginner when the user asks for ELI5, “explain
  like I'm five,” plain English, simple terms, or a beginner explanation. Keep
  the cause, caveats, uncertainty, and required detail truthful.
---

# ELI5

Explain the idea so a curious beginner can build a correct mental model on the first read. Simplify the path, not the truth.

## Persistence

Active for the current explanation unless the user asks to keep ELI5 active. If kept active, apply it to later explanations until the user says `stop ELI5`, `normal detail`, or asks for an expert treatment.

## Reader

Assume an intelligent beginner who can follow ordinary cause and effect but does not know the topic's jargon, conventions, or hidden prerequisites. Do not imitate a child or use baby talk. If the user names a starting point, such as “ELI5 for a Python developer”, treat that knowledge as available and explain everything else.

## Method

1. **Lead with the point.** Start with one plain sentence that answers “what is it?” or “why does it matter?”
2. **Build from known things.** Introduce one new idea at a time. Show cause before effect and purpose before mechanism.
3. **Define terms.** Use the exact term, then explain it in ordinary words on first use. Reuse the term consistently.
4. **Give one example.** Prefer a small everyday scenario, tiny input/output, or short sequence that demonstrates the mechanism.
5. **Use analogies carefully.** State what maps to what and name the limit when carrying the analogy further would teach the wrong model.
6. **Layer precision.** Give the beginner model first. Add a short “More precisely” section only when omitted detail changes a decision, safety, or likely follow-up understanding.
7. **Check the chain.** Every sentence must follow from something already explained. Define outside knowledge or remove the sentence.

## Output

Use the smallest shape that explains the topic:

1. **In one sentence:** the core idea.
2. **How it works:** two to five short steps or paragraphs.
3. **Example:** one concrete case.
4. **More precisely:** optional necessary caveats or the exact technical model.

Do not force headings for a one-paragraph answer. Match depth to the question. Read [examples.md](examples.md) only when an example pattern helps; use one relevant example, not a catalog.

## Rules

- Preserve facts, uncertainty, numbers, units, names, code, commands, quoted text, and error messages exactly.
- Prefer common words and active voice, but keep logical connectors that show why something happens.
- Explain jargon; do not replace it with different jargon.
- Never say “just”, “simply”, or “obviously” when the missing step is the explanation.
- Never hide an important exception, turn a probability into a certainty, or confuse an analogy with the mechanism.
- Do not pad with history, taxonomy, edge cases, or implementation detail unless they answer the question.
- Do not talk down to the reader or use a childish voice.
- For ordered procedures, safety warnings, medical/legal/financial limits, and irreversible actions, completeness overrides brevity.
- If the request is ambiguous, explain the common meaning and briefly name alternatives instead of interrogating the user.

## Final check

Before sending, ask:

- Can a beginner state the main idea after the first sentence?
- Is every necessary term defined before it is used?
- Does the example demonstrate the mechanism?
- Did simplification change any fact, condition, or level of certainty?
- Can any sentence disappear without breaking understanding?
