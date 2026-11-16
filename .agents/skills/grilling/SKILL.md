---
name: grilling
description: Grill the user relentlessly about a plan, decision, or idea. Use when the user wants to stress-test their thinking, or uses any 'grill' trigger phrases.
---

## Contract

- **Requested outcome:** Shared understanding of one plan, decision, or idea through a single-question interview.
- **Supplied inputs:** The user's topic, current conversation, and facts that can be checked in the environment.
- **Allowed child result:** None. Environment reads supply facts; this workflow does not invoke domain recording, research, planning, or implementation as a hidden stage.
- **Completion owner:** `grilling` owns the question order; the user owns the confirmation of shared understanding.
- **Return point:** After each answer, return to the next unresolved decision in the same interview.
- **Stopping condition:** Stop only when the user confirms shared understanding. Do not act on the decision or open a new agenda.

Interview me relentlessly about every aspect of this until we reach a shared understanding. Walk down each branch of the decision tree, resolving dependencies between decisions one-by-one. For each question, provide your recommended answer.

Ask the questions one at a time, waiting for feedback on each question before continuing. Asking multiple questions at once is bewildering.

If a *fact* can be found by exploring the environment (filesystem, tools, etc.), look it up rather than asking me. The *decisions*, though, are mine — put each one to me and wait for my answer.

Do not act on it until I confirm we have reached a shared understanding.
