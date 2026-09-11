---
name: lessons-learned
description: >-
  Peer-language lineage lessons and regrets so Jet does not repeat known
  failures.
---

# Lessons Learned

For each lesson: peer failure → Jet risk → guard (invariant, ratified decision,
open card, or ballot needed). Cover systems, managed, functional, scripting,
config/OS, and proof-oriented families. Include a do-not-ballot list where law
already covers the concern.

Before running, read `.agents/skills/_shared/audit-dispositions.md`. It owns
shared publication, workflow-boundary, and disposition mechanics; this method
still owns peer-family coverage, avoid/beat evidence, and guards.


## The standing lens

Apply `.agents/skills/_shared/standing-lens.md` in full: the four questions, the
five agent-optimality quantities, the micro sweep, probe the running binary, and
the honesty rules. The owner never has to ask for any of it.

## Both halves, always

A regret list is only half of lineage. Every peer that survived long enough to
have regrets also got things right, and the reason it won is as transferable as
the reason it hurt. Report both, in equal depth:

- **Avoid** — peer failure → Jet risk → guard. The existing shape. Include
  failures Jet is structurally immune to; immunity is a design asset, stated
  once.
- **Beat** — peer strength → the mechanism behind it → whether Jet's answer is
  shipped, ratified-but-unbuilt, or absent. Rank by how categorical the win is.
  For each, name what that peer would have to change to match Jet. A strength
  they cannot copy without breaking their own model is the one worth building
  toward.

A lesson that is only "they suffered, we must not" is incomplete. Say what Jet
does instead, and whether it exists yet.

## Output

This is a report-only method. Write one markdown report under `docs/research/`
through the project-approved non-serve CLI. Do not create Tower work or
implementation edits unless the owner explicitly asks. Read
`.agents/skills/_shared/audit-dispositions.md` before the run and use it for
publication rules and the required finding-disposition table. Report completion
does not implement a guard or change Jet law.
