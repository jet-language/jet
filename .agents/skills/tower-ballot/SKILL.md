---
name: tower-ballot
description: Author a ballot-ready decision on a Tower card — the standard for teaching the underlying concept, then presenting the gist, story, in-the-wild example, worked options, and recommendation needed to decide from the ballot alone. Use when raising an owner-facing choice, when asked to "queue a decision", "make this ballot-ready", or when a plan hits a choice only the owner may make.
---

# Tower — raise a ballot-ready decision

Any owner-facing choice becomes a `decision` on its card. The owner decides
from the ballot alone in focus mode; if he must ask what an option does, the
ballot is not ready. A plan-writer proposes; the owner picks.

## One decision per ballot

Every independently decidable owner choice gets its own ballot, including a
status-quo option when viable. An umbrella ballot may set a default direction,
but it must not silently settle downstream choices the owner could resolve
differently.

## Required fields

- **`gist`** — one short plain-language sentence naming the choice.
- **`lesson`** — a self-contained mini lesson for a reader starting with virtually
  no subject knowledge. Define the concept and unavoidable terms, give a useful
  mental model, explain how it works, show a tiny concrete example, and explain
  why it matters here. Teach enough that the reader could accurately explain the
  idea to someone else. Do not argue for an option yet, assume background
  knowledge, or turn this into a glossary dump. Aim for a quick two-to-four-minute
  read; use short paragraphs and plain language.
- **`story`** — a real person doing real work, showing why the choice exists.
- **`inWild`** — plausible project code/usage where the choice matters.
- **`options[]`** — `{key, name, detail, code}` for every genuine option.
  `detail` explains the option in plain language: what changes for the user,
  what it gains, and what it gives up. Each option shows exactly what the
  person types and sees, including errors. Put exact protocol, type, ABI,
  schema, or lowering law in optional `technical`; Focus Mode hides it behind
  “Technical details” so precision does not bury the decision.
- **`comparisons[]`** — `{lang, note, code}` when another product materially
  informs the choice.
- **`rec`** — recommended option key.
- **`recommendation`** — `{why, whyNot, tradeoff}`. `why` explains why the
  recommendation best serves this decision. `whyNot` contains one
  `{key, reason}` for every other option and explains why each loses here.
  `tradeoff` names the recommended option's real downside and why accepting it
  is still right. Never restate option names or say only “best balance.”
- **`group`** — a configured `decisionGroups` value from
  `.tower/config.json`.

## Required review pass

The author is the sole implementer of the ballot. Before it reaches the owner,
run two fresh-context reviews in order: Sol checks teaching quality, option
completeness, governing decisions, and recommendation logic; the author fixes
and Sol rechecks material findings; then Terra independently checks the revised
ballot and rechecks its material findings. Reviewers do not rewrite the ballot.

- **Plain language:** assume technical curiosity but no subject expertise.
  Prefer common words. Expand acronyms on first use. Define every unavoidable
  term where it first appears. Use one idea per sentence, lead with user impact,
  and move formal law into `technical`. Tower rejects prose sentences over 32
  words and paragraphs over 90 words; passing those limits is only a floor, not
  proof that dense jargon is acceptable.

- **Beginner:** ceremony-free defaults; expert policy stays hidden until needed.
- **Expert:** explicit control over graph, authority, generated code, toolchain,
  cache, scheduler, and audit behavior.
- **Cohesion:** each option must stand on its own. When one canonical mechanism
  can serve beginners and experts, present that complete mechanism as a normal
  option. Do not require a separate hybrid option or harvest ritual.
- **Kill criteria:** reject any option that hollows out the useful default,
  dictates a file/project structure, or carves around a safety/invariant
  guarantee. Fix the option before it reaches the owner.
- **Effort:** implementation difficulty never affects a ranking or recommendation.

## Owner design profile

- Concrete over abstract: show terminal output, file contents, exact errors, and
  complete workflows. Define unavoidable jargon.
- Write the lesson before the options. Test it against a zero-context reader:
  after reading only `gist` + `lesson`, they should understand the mechanism,
  stakes, and vocabulary needed to compare every option.
- Read every plain-language field without its code or technical appendix. If a
  technically inclined newcomer cannot retell it, rewrite it. Remove stacked
  nouns, unexplained abbreviations, internal IDs, and implementation vocabulary
  that does not change the owner's choice.
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
  "lesson": "A cache keeps a reusable copy of expensive work. Invalidation is the rule that decides when that copy is too old to trust...",
  "story": "Dana ships a pricing page. A vendor updates a rate at 9am; ...",
  "inWild": "...",
  "options": [
    { "key": "A", "name": "TTL per entry", "detail": "...", "technical": "...", "code": "..." },
    { "key": "B", "name": "Event-driven purge", "detail": "...", "technical": "...", "code": "..." }
  ],
  "comparisons": [
    { "lang": "Rails", "note": "...", "code": "..." }
  ],
  "rec": "B",
  "recommendation": {
    "why": "Updates become visible as soon as the source announces them, without serving known-stale prices.",
    "whyNot": [{ "key": "A", "reason": "A time limit still serves stale prices until its clock expires." }],
    "tradeoff": "Every writer must emit a purge event. That is acceptable because this system already owns every price update."
  }
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
