---
name: tower-ballot
description: Author a ballot-ready decision on a Tower card — the standard for what the owner needs to decide from the ballot alone (gist, story, in-the-wild example, worked options, recommendation). Use when raising an owner-facing choice, when asked to "queue a decision", "make this ballot-ready", or when a plan hits a choice only the owner may make.
---

# Tower — raise a ballot-ready decision

Any owner-facing choice becomes a `decision` on its card. The owner decides
from the ballot alone in focus mode; if he must ask what an option does, the
ballot is not ready. A plan-writer proposes; the owner picks.

## Plain language — mandatory, not optional

Ballots are read by the owner to make a decision. Write them the way you would
explain the choice to a colleague at a whiteboard: plain, direct, concrete. Do
not write them the way a model "polishes" prose.

**Banned outright** — these read as clever/creative filler and hide the actual
choice:
- Invented framing words and metaphors: "the disease", "bright spots", "blast
  radius", "poisons", "coats of paint", "the win", "footgun" as decoration,
  "surface" as a noun for code, "the arc", "north star".
- Dramatized verbs and stakes: "beautifully", "ruthlessly", "hard", "brutal",
  "sharpest edge", "load-bearing", "at the worst moment".
- Rhetorical devices: rhetorical questions, alliteration, tricolons ("X, Y, and
  Z" for rhythm), em-dash pile-ups used for effect, one-word sentences for drama.
- Abbreviated arrow-chains as prose ("A → B → fails"), and "sibling test" style
  invented jargon.

**Required instead:**
- Name the actual construct, file, and decision ID. State the choice as a fact:
  "Today `executable` is written as a bare keyword. `Wrap` is written as
  `Wrap.{ … }`. This decides whether they use the same shape."
- Let the real before/after code carry the weight. Every option shows the exact
  thing the user types today and what they would type after — real snippets from
  the tree, not illustrations.
- Everyday words. Short declarative sentences. If a sentence has a metaphor or an
  adjective doing persuasion, cut it.
- The recommendation gives the concrete reason (what breaks, what gets simpler),
  never an abstract virtue ("cleaner", "more elegant", "coherent").

Before saving a ballot, reread every field and delete any word that is there for
tone rather than information. If you cannot say it plainly, you do not yet
understand the choice well enough to put it in front of the owner.

## One decision per ballot — the owner controls each resolution

Every distinct owner-facing choice is its own ballot with its own options,
including a "leave as-is" option whenever the status quo is viable. Do **not**
bundle several resolutions under one umbrella ballot such that ratifying it
silently pre-decides the others. If a "law" or "principle" ballot would commit
the direction of downstream choices, that is a hidden prescription the owner
cannot see or adjust — split it so each real choice is a ballot he picks. An
umbrella ballot may set a default lean, but each downstream area still gets its
own ballot with real, independently-decidable options.

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
