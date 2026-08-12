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

Write one markdown report under `docs/research/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section research --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/research/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
