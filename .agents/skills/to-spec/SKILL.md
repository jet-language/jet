---
name: to-spec
description: Turn the current conversation into a spec and publish it to a Tower card — no interview, just synthesis of what you've already discussed.
disable-model-invocation: true
---

This skill takes the current conversation context and codebase understanding and produces a spec (you may know this document as a PRD). Do NOT interview the user — just synthesize what you already know.

## Contract

- **Requested outcome:** A synthesized spec published through Tower, not an interview.
- **Supplied inputs:** The current conversation, codebase understanding, domain glossary, ADRs, and the relevant Tower card.
- **Allowed child result:** Passive reads may ground the spec. No child workflow interviews the user, creates implementation tickets, or starts implementation on this route.
- **Completion owner:** `to-spec` owns synthesis and publication; Tower owns the published spec state.
- **Return point:** After passive exploration, return to synthesis; record missing or gated choices as unresolved instead of opening an interview, then publish through Tower.
- **Stopping condition:** Stop after the spec is published with its decisions, tests, and out-of-scope terms. Do not advance an unresolved card to `ready` or open a planning or implementation agenda automatically.

Read `AGENTS.md` for Jet's Tower work-state authority. Do not run a generic
tracker installer.

## Process

1. Explore the repo to understand the current state of the codebase, if you haven't already. Use the project's domain glossary vocabulary throughout the spec, and respect any ADRs in the area you're touching.

2. Sketch out the seams at which you're going to test the feature. Existing seams
   should be preferred to new ones. Use the highest seam possible. If new seams
   are needed, propose them at the highest point you can. Derive the seams from
   the supplied conversation, codebase, glossary, and ADR context; do not open an
   unrequested interview. The fewer seams across the codebase, the better — the
   ideal number is one. Record any genuinely missing or gated choice in the spec
   instead of guessing. An unresolved spec is not ready for an agent.

3. Write the spec using the template below, then publish it to the relevant
Tower card. Keep unresolved or gated choices in `decide`; advance to `ready`
only when no required decision remains. Do not start planning or implementation
automatically.

<spec-template>

## Problem Statement

The problem that the user is facing, from the user's perspective.

## Solution

The solution to the problem, from the user's perspective.

## User Stories

A LONG, numbered list of user stories. Each user story should be in the format of:

1. As an <actor>, I want a <feature>, so that <benefit>

<user-story-example>
1. As a mobile bank customer, I want to see balance on my accounts, so that I can make better informed decisions about my spending
</user-story-example>

This list of user stories should be extremely extensive and cover all aspects of the feature.

## Implementation Decisions

A list of implementation decisions that were made. This can include:

- The modules that will be built/modified
- The interfaces of those modules that will be modified
- Technical clarifications from the developer
- Architectural decisions
- Schema changes
- API contracts
- Specific interactions

Do NOT include specific file paths or code snippets. They may end up being outdated very quickly.

Exception: if a prototype produced a snippet that encodes a decision more precisely than prose can (state machine, reducer, schema, type shape), inline it within the relevant decision and note briefly that it came from a prototype. Trim to the decision-rich parts — not a working demo, just the important bits.

## Testing Decisions

A list of testing decisions that were made. Include:

- A description of what makes a good test (only test external behavior, not implementation details)
- Which modules will be tested
- Prior art for the tests (i.e. similar types of tests in the codebase)

## Out of Scope

A description of the things that are out of scope for this spec.

## Further Notes

Any further notes about the feature.

</spec-template>
