---
name: tower-ballot
description: Author a complete short or full Tower ballot with simple prose, worked options, ordered review passes, and a recommendation that explains why it wins and why every alternative loses. Use when raising an owner-facing choice, when asked to "queue a decision", "make this ballot-ready", or when a plan hits a choice only the owner may make.
---

# Tower — raise a ballot-ready decision

Any owner-facing choice becomes a `decision` on its card. The owner decides
from the ballot alone, in the board's focus mode — if they would have to ask
you something to decide, it is not ready. A plan-writer **proposes**; the
owner **picks**; never pre-empt the pick.

## Apply `/simple` to everything

Use the `simple` skill for every user-visible word on every ballot. This rule
includes headings, context, stories, option names and details, technical notes,
comparisons, review summaries, recommendations, reasons against other options,
tradeoffs, instructions, and prose inside examples. Code stays valid code.

Use common words and one idea per sentence. Expand an acronym the first time.
Define an unavoidable term where it appears. Lead with user impact. Put formal
rules in `technical`. Tower rejects prose sentences over 32 words and
paragraphs over 90 words.

## Choose the profile

`full` is the default. Use `short` only when the owner explicitly asks for a
short ballot in the current request.

- A **short ballot** is one complete base draft. It has every decision field,
  complete options, and a recommendation, but no review passes. Set
  `ballotMode: "short"`, copy the owner's request into `shortAuthorizedBy`, and
  omit `reviewPasses`.
- A **full ballot** starts with the same complete base draft, then runs all four
  review passes below. Set `ballotMode: "full"` and record all five stage
  summaries in `reviewPasses`.

"Simple ballot" is not a profile name. `/simple` applies to both profiles.

## The decision fields

- **`gist`** — one very short plain-language sentence: what is being chosen.
  No jargon.
- **`lesson`** — a few plain sentences in one short paragraph. Explain the
  situation and stakes so a new reader has enough context to compare the
  options. Do not write a tutorial, glossary, mechanism tour, or argument for
  one option. Put option-specific facts in the options.
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
- **`group`** — one of the project's `decisionGroups` (see `.tower/config.json`)
  so the queue stays organized.

Make each option internally cohesive. When one canonical mechanism can serve
beginners and experts, present that complete mechanism as a normal option. Do
not create a separate hybrid option unless it is a real final design.

## Build and review in this exact order

Do not start a later pass early. Revise the ballot after each pass, then write a
one- or two-sentence summary of what that pass found or changed.

1. **Base** — write the complete first draft. This is the same finished draft
   that a short ballot would ship. It includes all fields, every credible option,
   worked examples, why the recommendation wins, why every other option loses,
   and the accepted tradeoff.
2. **Boil the ocean** — search the full solution space. Add a genuinely distinct
   option if the base draft missed one. Fold mere tactics into their parent
   options instead of padding the menu.
3. **Hybrid** — test whether the best parts of different options can form a
   stronger coherent choice. Revise the options and recommendation when they
   can. The older detailed `hybrid` synthesis and harvest fields are optional;
   the `reviewPasses.hybrid` summary is required.
4. **Cooperative** — steelman every option on its own terms. Make each choice the
   strongest honest version of itself, including the options you expect to lose.
5. **Adversarial** — attack the recommendation, its assumptions, and its failure
   modes. Repair the ballot and change the recommendation if it does not survive.

The summaries are evidence, not status labels. Say what was tested, added,
removed, combined, strengthened, or repaired. After the adversarial pass, check
that `recommendation.whyNot` still covers every losing option.

Focus Mode shows these stages in order: base in slate, boil the ocean in violet,
hybrid in cyan, cooperative in green, and adversarial in orange. It shows the
recommendation in blue and reasons against alternatives in muted red. Labels and
icons carry the same meaning when color is unavailable.

## Mechanics

```
cat > /tmp/ballot.json <<'EOF'
{
  "cardId": "#12",
  "id": "D-CACHE1",
  "title": "Cache invalidation strategy",
  "group": "architecture",
  "ballotMode": "full",
  "gist": "How cached results expire.",
  "lesson": "A cache keeps a reusable copy of expensive work. This choice decides when that copy is too old to trust and how quickly a new price reaches customers.",
  "story": "Dana ships a pricing page. A vendor updates a rate at 9am; ...",
  "inWild": "...",
  "options": [
    { "key": "A", "name": "TTL per entry", "detail": "...", "technical": "...", "code": "..." },
    { "key": "B", "name": "Event-driven purge", "detail": "...", "technical": "...", "code": "..." }
  ],
  "comparisons": [ { "lang": "Rails", "note": "...", "code": "..." } ],
  "rec": "B",
  "recommendation": {
    "why": "Updates become visible as soon as the source announces them, without serving known-stale prices.",
    "whyNot": [{ "key": "A", "reason": "A time limit still serves stale prices until its clock expires." }],
    "tradeoff": "Every writer must emit a purge event. That is acceptable because this system already owns every price update."
  },
  "reviewPasses": {
    "base": "The first draft compared time limits with purge events and recommended purge events because known changes become visible at once.",
    "boilOcean": "The breadth review also tested versioned keys, manual clearing, and no cache. Those were folded into the two main choices because they did not change who controls expiry.",
    "hybrid": "The hybrid review kept purge events and added a long safety limit. The limit clears an entry if an event is ever lost.",
    "cooperative": "The cooperative review strengthened time limits with random staggering and per-entry settings. It still permits known-stale prices after an update.",
    "adversarial": "The adversarial review tested lost, repeated, and delayed events. A safety limit and safe repeated purges repair those risks, so purge events remain the recommendation."
  }
}
EOF
tower decision add --file /tmp/ballot.json --by <me>
```

The card's lane flips to `decide` automatically; leave it there. Nudge the
owner if it's urgent (new ballots show on the live SSE board — web push removed).

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
- Never mark a ballot `short` because time is tight or the choice looks easy.
  Only the owner's explicit request authorizes that profile.
