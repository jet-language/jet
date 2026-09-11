---
name: jet-router
description: >-
  Route a Jet request to one primary audit, research, planning, cleanup, or
  verify skill. Use when the skill is unclear, or for a pulse or health check.
---

# Jet Router

## Contract

- **Requested outcome:** One primary Jet skill and outcome for the request.
- **Supplied inputs:** The user's wording, relevant existing context, and the routing index.
- **Allowed child result:** The selected primary may return only bounded evidence, records, reviews, or support named by its own contract. Child work cannot become another primary or open an undeclared agenda.
- **Completion owner:** The selected primary owns completion; Tower owns durable board state when applicable.
- **Return point:** Every child handoff returns to the selected primary before it records its result.
- **Stopping condition:** Stop when the primary's completion criterion is met. Passive references are not extra routes, and host metadata is not runtime proof.

Choose one **primary skill** from `.agents/skills/JetSkillsRouter.md`. The primary
owns the requested outcome. It may use only the bounded handoffs or passive
references that its contract names; every child result returns to that primary.
Keep composition acyclic, refuse silent scope expansion, and do not jump from
planning to implementation.
