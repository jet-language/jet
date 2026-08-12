---
name: persona-audit
description: >-
  Persona-based status checks for Jet: practical use, push and pull factors,
  feel for development state.
---

# Persona Audit

Generate fresh personas (beginner through expert, distinct domains). For each,
define a concrete project and its core loop, run representative examples with
`scripts/agent/jet-env`, and report push/pull factors plus a clear verdict
(`ship-ready` / `usable-with-friction` / `blocked`).

## The standing lens

Apply `.agents/skills/_shared/standing-lens.md` in full: the four questions, the
five agent-optimality quantities, the micro sweep, probe the running binary, and
the honesty rules. The owner never has to ask for any of it.

## One persona is always a coding agent

Include an unattended agent among the personas every run. It is the reader Jet
is ultimately built for, and it has a core loop like any other persona: read
context, edit, run the checker, read the verdict, repeat, and stop when clean.

Give it a real project and run it. Its push and pull factors are the five
quantities — whether the checker caught the mistake, how long the verdict took,
whether the report could be acted on without guessing, how many tokens the loop
burned, and whether one error admitted one obvious repair or several. Its
verdict uses the same three words as any other persona.

Do not soften a `blocked` verdict here because the tooling around it is young.
An agent that cannot finish the loop is blocked.

## Report the feel, not only the outcome

Verdicts capture whether a persona finished. Push and pull factors capture
whether they wanted to. Both matter, and the second is the one usually lost.

Walk the UX and DX slice of the micro sweep for each persona: where they waited,
where the tool surprised them, what they had to say twice, what they had to know
before they could start, and which error text left them stuck. Record a
throwaway "this reads nicely" or "this made me sigh" verbatim. A preference
remark is evidence about the surface even when it is not evidence about the
technology.

## Output

Write one markdown report under `docs/audits/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/audits/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
