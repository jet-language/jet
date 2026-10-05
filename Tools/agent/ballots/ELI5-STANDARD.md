# Owner ballot standard (owner directive, 03:45). It overrides earlier ballot guidance where they differ.

## 1. Situation summary first

Open with a short summary written for someone who hasn't read any code: 4–7 plain sentences. It answers, in this order:

1. What is the thing? Name it in everyday words first, technical name second.
2. What's happening today, and what problem or gap does that cause? Give one concrete example of where it bites.
3. Why does it matter, and who feels it: beginners, experts, performance, safety?
4. What exactly are you being asked to decide?
5. What do we recommend, in one sentence?

It is not a list of facts, files, decision IDs or card numbers. Those go in the detail section.

## 2. ELI5 readability pass on every field

- Short sentences and common words.
- Define every term the first time it appears, or replace it with a plain one.
- Use an analogy where it helps.
- No unexplained sigils, internal names or jargon chains.
- Code examples are small, commented and complete. Show what the user writes and what happens.
- Keep the exact technical contract, but put it after the plain explanation (in `technical` / `detail`). Precision stays; it just isn't the first thing a reader meets.
- Total length may grow about 25% beyond the current ballot for added context. Don't pad.

## 3. Design the downsides away

The owner instruction: the recommended solution must effectively have NO downsides.

- For every loss or downside any option has, try to remove it in the design: a creative hybrid, a better default, a staged opt-in, compiler-inferred behavior, a lint, a library helper, a different split of responsibilities.
- Rework the recommended option until its `losses` list is empty, or truly negligible, with a concrete mitigation stated for each.
- "whyUnavoidable" is no longer an acceptable answer. Replace it with the mitigation that makes the loss go away.
- If a downside is genuinely physically impossible to remove, say so plainly, show the mitigation that minimizes it, and flag it in the summary. This should be rare.
- Alternatives can keep their real downsides; that is how the owner sees why the recommendation wins.
- Every option stays a real same-program alternative, with the same example program across options.

## 4. Unchanged rules

- I8: one mechanism per meaning.
- Code is truth: claims about current behavior must be checked against code.
- Exact syntax.
- Beginner and expert paths.
- Edge cases.
- Nothing is "owner-approved" until the owner votes.
- Each payload passes the Tower validator and gets one fresh adversarial review.
