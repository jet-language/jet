---
name: tower-ballot
description: Author a ballot-ready decision on a Tower card — the standard for what the owner needs to decide from the ballot alone (gist, story, in-the-wild example, worked options, recommendation). Use when raising an owner-facing choice, when asked to "queue a decision", "make this ballot-ready", or when a plan hits a choice only the owner may make.
---

# Tower — raise a ballot-ready decision

Any owner-facing choice becomes a `decision` on its card. The owner decides
from the ballot alone in focus mode; if he must ask what an option does, the
ballot is not ready. A plan-writer proposes; the owner picks.

## Required fields

- **`gist`** — one short plain-language sentence naming the choice.
- **`story`** — a real person doing real work, showing why the choice exists.
- **`inWild`** — plausible project code/usage where the choice matters.
- **`options[]`** — `{key, name, detail, code}` for every genuine option.
  Each option shows exactly what the person types and sees, including errors.
- **`comparisons[]`** — `{lang, note, code}` when another product materially
  informs the choice.
- **`rec`** — recommended option key, with its one-line reason in
  `detail` or `explainer`.
- **`group`** — a configured `decisionGroups` value from
  `.tower/config.json`.

## Required review pass

- **Beginner:** ceremony-free defaults; expert policy stays hidden until needed.
- **Expert:** explicit control over graph, authority, generated code, toolchain,
  cache, scheduler, and audit behavior.
- **Hybrid:** prefer one canonical semantic mechanism with ergonomic entrypoints
  over siloed beginner/expert systems (I8).
- **Kill criteria:** reject any option that hollows out the useful default,
  dictates a file/project structure, or carves around a safety/invariant
  guarantee. Fix the option before it reaches the owner.
- **Effort:** implementation difficulty never affects a ranking or recommendation.

## Owner design profile

- Concrete over abstract: show terminal output, file contents, exact errors, and
  complete workflows. Define unavoidable jargon.
- Naming ballots need many high-quality original candidates in Jet's aviation
  family. Do not echo the owner's suggestion or offer derivative variants.
- Cut repetition. Drive every option from the same full real-world example.
- “Take inspiration from X” means transplant useful mechanics into worked Jet
  usage, not write a survey of X.
- Design options vary **UX**: information architecture, interaction model, and
  primary loop. Palette-only variants are one option repeated.
- UI copy and visuals use no metaphor theming, mascots, cockpit decoration, or
  invented product jargon. Product names are fine; controls name their action.
- Frontend acceptance requires the complete mock matrix in the owner's real
  terminal: every archetype, relevant viewport/state, keyboard/focus behavior,
  ANSI/NO_COLOR behavior for terminals, and real rendered review—not a textual
  claim or one cherry-picked screenshot.
- Re-read the live ballot immediately before briefing any agent. Ballots can be
  edited after minting; never delegate from a remembered paraphrase.

## Mechanics

```json
{
  "cardId": "#12",
  "id": "D-CACHE1",
  "title": "Cache invalidation strategy",
  "group": "architecture",
  "gist": "How cached results expire.",
  "story": "Dana ships a pricing page. A vendor updates a rate at 9am; ...",
  "inWild": "...",
  "options": [
    { "key": "A", "name": "TTL per entry", "detail": "...", "code": "..." },
    { "key": "B", "name": "Event-driven purge", "detail": "...", "code": "..." }
  ],
  "comparisons": [
    { "lang": "Rails", "note": "...", "code": "..." }
  ],
  "rec": "B"
}
```

Write the JSON to a temporary file, then:

```
tower decision add --file /tmp/ballot.json --by <agent>
```

The card moves to `decide`; leave it there. An unfinished ballot uses
`--draft` and must become ready before owner review.

An owner ruling is also a ballot record, never a log note:

```
tower verdict '#N' --outcome "owner's ruling" --title "Short title" --by owner
```

## Rules

- Decision IDs are unique and stable; check Tower and
  `docs/spec/syntax-decisions.md` before minting.
- Never offer a choice contradicting a ratified decision.
- Every option is independently understandable and worked. Do not split
  beginner ease and expert control unless semantics truly conflict.
- Honor every word in a ratification. A question/comment inside it must be
  addressed before implementation.
- Owner asks for changes via a question: edit the live decision, then reply.
- Once ratified, remove it from open-ballot docs; history stays in Tower and the
  ratified record.
