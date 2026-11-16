---
name: to-tickets
description: Break settled implementation work from a plan, spec, or resolved conversation into tracer-bullet Tower cards with native blocking edges.
disable-model-invocation: true
---

# To Tickets

Break settled implementation work into **tickets** — tracer-bullet vertical
slices, each declaring the tickets that **block** it. Do not use this route to
resolve design decisions.

Read `AGENTS.md` for Jet's Tower work-state authority. Do not run a generic
tracker installer.

## Contract

- **Requested outcome:** A dependency-ordered set of implementation tickets cut from a settled plan or spec.
- **Supplied inputs:** An approved plan/spec or an already-resolved conversation, Tower board context, project glossary, and relevant ADRs.
- **Allowed child result:** Passive reads may clarify slice boundaries. No child workflow resolves design questions, starts implementation, or creates a second ticket ledger. If the source is not settled, return it to the decision owner instead of guessing.
- **Completion owner:** `to-tickets` owns the proposed slice boundaries and blocking edges until the user approves them; Tower owns published work state.
- **Return point:** After exploration, return to the slice draft; after user approval, return to publication and report the Tower result.
- **Stopping condition:** Stop after approved tickets are published with blocking edges and no implementation has started. Do not turn a planning request into a build or a design session.

## Process

### 1. Gather context

Work from whatever is already in the conversation context. If the user passes a reference (a spec path, an issue number or URL) as an argument, fetch it and read its full body and comments.

### 2. Explore the codebase (optional)

If you have not already explored the codebase, do so to understand the current state of the code. Ticket titles and descriptions should use the project's domain glossary vocabulary, and respect ADRs in the area you're touching.

Use codebase reading only to size slices and preserve settled decisions. If a
design remains unresolved, return it to the decision owner instead of
inventing a design ticket or speculative prefactoring.

### 3. Draft vertical slices

Break the settled work into **tracer bullet** tickets.

<vertical-slice-rules>

- Each slice cuts a narrow but COMPLETE path through every layer (schema, API, UI, tests) — vertical, NOT a horizontal slice of one layer
- A completed slice is demoable or verifiable on its own
- Each slice is sized to fit in a single fresh context window
- Preparatory work belongs in a slice only when the approved plan already requires it

</vertical-slice-rules>

Give each ticket its **blocking edges** — the other tickets that must complete before it can start. A ticket with no blockers can start immediately.

**Wide mechanical changes still land as one coherent cutover.** If a rename or
retype spans many callers, assign one integration-owned ticket for the complete
migration and deletion of the old form. Disjoint preparation — such as
inventory, generated updates, or compile-fix notes — may be separate only when
it cannot leave a compatibility form or partial behavior; block the cutover on
that preparation. Do not add expand–contract compatibility forms or batches of
migration tickets. A retained old/new form requires an explicit owner exception
and a bounded retirement ticket.

### 4. Quiz the user

Present the proposed breakdown as a numbered list. For each ticket, show:

- **Title**: short descriptive name
- **Blocked by**: which other tickets (if any) must complete first
- **What it delivers**: the end-to-end behaviour this ticket makes work

Ask the user:

- Does the granularity feel right? (too coarse / too fine)
- Are the blocking edges correct — does each ticket only depend on tickets that genuinely gate it?
- Should any tickets be merged or split further?

Iterate until the user approves the breakdown.

### 5. Publish the tickets to Tower

Publish the approved tickets through Tower's configured CLI and card workflow.
Create one card per ticket, in dependency order, and use native `blockedBy`
links for real blocking edges. Keep the card body focused on the end-to-end
behavior and acceptance criteria; Tower owns the card's phase and durable
status.

Work the **frontier**: any ticket whose blockers are all done. For a purely
linear chain that means top to bottom.

Do not create `.scratch` ticket files or a second ticket ledger.

<card-template>

## Parent

A reference to the parent Tower card, if the source was an existing card; otherwise omit this section.

## What to build

The end-to-end behaviour this ticket makes work, from the user's perspective — not layer-by-layer implementation.

## Acceptance criteria

- [ ] Criterion 1
- [ ] Criterion 2

## Blocked by

- A reference to each blocking ticket, or "None — can start immediately".

</card-template>

When writing the card, avoid specific file paths or code snippets — they go stale fast. Exception: if a prototype produced a snippet that encodes a decision more precisely than prose can (state machine, reducer, schema, type shape), inline it and note briefly that it came from a prototype. Trim to the decision-rich parts — not a working demo, just the important bits.
