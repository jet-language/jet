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

## Owner style — how he actually decides

- **Concrete over abstract.** He decides from worked use cases: terminal
  output, file contents, the exact error text. Plain language; if jargon is
  unavoidable, define it first.
- **Naming ballots get a rich menu** — many high-quality original candidates
  (jet/aviation theme). Never echo his own suggestion back as an option;
  never offer only derivative variants.
- **Anti-repetition.** He cuts syntax that makes users repeat themselves;
  drive options from one full real-world example, not fragments.
- **"Take inspiration from X"** = concrete transplants with worked code in
  THIS project's syntax — never a survey or comparison document of X.
- **Re-read the live ballot right before briefing** any sub-agent on it —
  option wording can be reworked after minting; never paraphrase from an
  earlier read.

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
- Once ratified, remove the decision from any open-ballot doc — only open
  items stay in the queue (decision fatigue is real).
