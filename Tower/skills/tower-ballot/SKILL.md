---
name: tower-ballot
description: Author a ballot-ready decision on a Tower card — the standard for teaching the underlying concept, then presenting the gist, story, in-the-wild example, worked options, and recommendation needed to decide from the ballot alone. Use when raising an owner-facing choice, when asked to "queue a decision", "make this ballot-ready", or when a plan hits a choice only the owner may make.
---

# Tower — raise a ballot-ready decision

Any owner-facing choice becomes a `decision` on its card. The owner decides
from the ballot alone, in the board's focus mode — if they would have to ask
you something to decide, it is not ready. A plan-writer **proposes**; the
owner **picks**; never pre-empt the pick.

## The fields (fill them all)

- **`gist`** — one very short plain-language sentence: what is being chosen.
  No jargon.
- **`lesson`** — a self-contained mini lesson for a reader starting with virtually
  no subject knowledge. Define the concept and unavoidable terms, give a useful
  mental model, explain how it works, show a tiny concrete example, and explain
  why it matters here. Teach enough that the reader could accurately explain the
  idea to someone else. Do not argue for an option yet, assume background
  knowledge, or turn this into a glossary dump. Aim for a quick two-to-four-minute
  read; use short paragraphs and plain language.
- **`story`** — a short paragraph naming a real person and what they're
  doing, so the owner knows *why this decision exists* before any detail.
- **`inWild`** — realistic code/usage from a plausible real project where
  the choice actually bites (renders syntax-highlighted). Not a toy.
- **`options[]`** — `{key, name, detail, code}` for **every** option. Each
  uses plain-language `detail` for user impact, gain, and loss. Put exact
  protocol, type, ABI, schema, or lowering law in optional `technical`; Focus
  Mode hides it behind “Technical details.” Each option carries a worked
  `code` example showing exactly what the person
  types and sees — including the error they hit, when that's the point. No
  option described only abstractly. Rich menu of genuine alternatives, never
  2–3 derivative spellings of one idea.
- **`comparisons[]`** — `{lang, note, code}`: how other languages/tools/
  products spell the same thing, when a comparison genuinely informs.
- **`rec`** — the recommended option key.
- **`recommendation`** — `{why, whyNot, tradeoff}`. Explain why the winner best
  serves this decision, why every other option loses here, and which downside
  the recommendation accepts. `whyNot` contains one `{key, reason}` per losing
  option. Never use empty phrases such as “best balance.”
- **`hybrid`** — `{result, synthesis, harvest}` written only after every option
  is complete. `result` must equal `rec`. `harvest` contains one
  `{key, aspect, use}` per option: identify its strongest idea, then explain how
  the synthesis uses it or why a semantic conflict prevents that use.
- **`group`** — one of the project's `decisionGroups` (see `.tower/config.json`)
  so the queue stays organized.

Write the lesson before the options. Test it against a zero-context reader:
after reading only `gist` + `lesson`, they should understand the mechanism,
stakes, and vocabulary needed to compare every option.

Assume technical curiosity but no subject expertise. Prefer common words.
Expand acronyms on first use and define unavoidable terms where they first
appear. Use one idea per sentence. Lead with user impact; move formal law into
`technical`. Tower rejects prose sentences over 32 words and paragraphs over 90
words, but those limits do not excuse unexplained jargon. Read the plain fields
without code or technical appendices; rewrite anything a newcomer cannot retell.

Run hybridization last. Let different ideas develop before combining them.
Then harvest every compatible strength into one canonical mechanism with a
simple beginner path and explicit expert control. Never average spellings or
label an early compromise “hybrid.” If a strength cannot fit, state the exact
semantic conflict. Rewrite the resulting option before recommending it.

## Mechanics

```
cat > /tmp/ballot.json <<'EOF'
{
  "cardId": "#12",
  "id": "D-CACHE1",
  "title": "Cache invalidation strategy",
  "group": "architecture",
  "gist": "How cached results expire.",
  "lesson": "A cache keeps a reusable copy of expensive work. Invalidation is the rule that decides when that copy is too old to trust...",
  "story": "Dana ships a pricing page. A vendor updates a rate at 9am; ...",
  "inWild": "...",
  "options": [
    { "key": "A", "name": "TTL per entry", "detail": "...", "technical": "...", "code": "..." },
    { "key": "B", "name": "Event-driven purge", "detail": "...", "technical": "...", "code": "..." }
  ],
  "comparisons": [ { "lang": "Rails", "note": "...", "code": "..." } ],
  "rec": "B",
  "hybrid": {
    "result": "B",
    "synthesis": "B keeps immediate purge events and borrows A's fallback clock for missed events.",
    "harvest": [
      { "key": "A", "aspect": "A stale entry eventually expires without an event.", "use": "Use its clock only as a safety net." },
      { "key": "B", "aspect": "Known updates purge immediately.", "use": "Keep this as the primary rule." }
    ]
  },
  "recommendation": {
    "why": "Updates become visible as soon as the source announces them, without serving known-stale prices.",
    "whyNot": [{ "key": "A", "reason": "A time limit still serves stale prices until its clock expires." }],
    "tradeoff": "Every writer must emit a purge event. That is acceptable because this system already owns every price update."
  }
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
