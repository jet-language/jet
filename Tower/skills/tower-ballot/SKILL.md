---
name: tower-ballot
description: Author a ballot-ready decision on a Tower card — the standard for what the owner needs to decide from the ballot alone (gist, story, in-the-wild example, worked options, recommendation). Use when raising an owner-facing choice, when asked to "queue a decision", "make this ballot-ready", or when a plan hits a choice only the owner may make.
---

# Tower — raise a ballot-ready decision

Any owner-facing choice becomes a `decision` on its card. The owner decides
from the ballot alone, in the board's focus mode — if they would have to ask
you something to decide, it is not ready. A plan-writer **proposes**; the
owner **picks**; never pre-empt the pick.

## The fields (fill them all)

- **`gist`** — one very short plain-language sentence: what is being chosen.
  No jargon.
- **`story`** — a short paragraph naming a real person and what they're
  doing, so the owner knows *why this decision exists* before any detail.
- **`inWild`** — realistic code/usage from a plausible real project where
  the choice actually bites (renders syntax-highlighted). Not a toy.
- **`options[]`** — `{key, name, detail, code}` for **every** option. Each
  carries its own worked `code` example showing exactly what the person
  types and sees — including the error they hit, when that's the point. No
  option described only abstractly. Rich menu of genuine alternatives, never
  2–3 derivative spellings of one idea.
- **`comparisons[]`** — `{lang, note, code}`: how other languages/tools/
  products spell the same thing, when a comparison genuinely informs.
- **`rec`** — the recommended option key; put the one-line *why* in `detail`
  or `explainer`.
- **`group`** — one of the project's `decisionGroups` (see `.tower/config.json`)
  so the queue stays organized.

## Mechanics

```
cat > /tmp/ballot.json <<'EOF'
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
  "comparisons": [ { "lang": "Rails", "note": "...", "code": "..." } ],
  "rec": "B"
}
EOF
tower decision add --file /tmp/ballot.json --by <me>
```

The card's lane flips to `decide` automatically; leave it there. Nudge the
owner if it's urgent (new ballots already trigger a push notification).

## Rules

- Decision `id` must be unique and stable (`D-…`); check existing ids first
  (`tower decision list --json`).
- Never invent choices that contradict an already-ratified decision — read
  the project's ratified record first.
- Implementation difficulty must never appear in a tradeoff, ranking, or
  recommendation. Rank on the project's actual priorities.
- When the owner ratifies with a comment, **honor every word** — a question
  inside a ratification is not a clean pick; address it before building.
- Owner asks for changes via a question → edit the ballot
  (`tower decision update <id> --file …`), then reply.
