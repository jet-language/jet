---
name: tower-ballot
description: Author a complete short or full Tower ballot with simple prose, worked options, ordered review passes, and a recommendation that explains why it wins and why every alternative loses. Use when raising an owner-facing choice, when asked to "queue a decision", "make this ballot-ready", or when a plan hits a choice only the owner may make.
---

# Tower — raise a ballot-ready decision
## Contract

- **Requested outcome:** A complete `short` or `full` ballot that lets the owner decide from the reading surface alone.
- **Supplied inputs:** The card and choice, ratified decisions, real current and in-the-wild examples, project priorities, and the required ballot profile.
- **Allowed child result:** In `full`, one fresh beginner reader returns RLI5 friction and a separate fresh adversarial reader returns challenge findings. Neither reader helped author the ballot. Either may share the author's model family. They cannot decide, publish, or open a follow-on workflow.
- **Completion owner:** `tower-ballot` owns the draft and repair; the owner owns the choice and ratification.
- **Return point:** Each reader result returns to the ballot's design-away and recommendation checks.
- **Stopping condition:** Stop when the selected profile is complete, every loss is addressed or justified, and Tower accepts the ready ballot. Do not implement the choice.

Any owner-facing choice becomes a `decision` on its card. The owner decides
from the ballot alone, in the board's focus mode — if they would have to ask
you something to decide, it is not ready. A plan-writer **proposes**; the
owner **picks**; never pre-empt the pick.
- A performance-motivated surface ballot must include an executable candidate/plain two-program cell in the canonical gauntlet matrix before owner ratification; apply the standing comparator and keep every loss carded.

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

Ratified 2026-09-02 (D-BALLOT-PROCESS1 = C). New full ballots use the current
two-reader process: `beginner` and `adversarial`. Stored process 2/3 ballots
keep their historical six-pass records and render unchanged; never rewrite
those records just to fit the current process.

- **`short`** is the default for a ballot inside one mechanism with at most
  three options. It is one complete base draft: the reading surface, every
  decision field, complete options, a recommendation, no review passes. Set
  `ballotMode: "short"`; Tower accepts it without `shortAuthorizedBy` when the
  card is a one-mechanism choice with at most three options.
- **`full`** is required for new syntax (anything that touches `Syntax.rs`),
  any invariant carve-out (I1-I9), and any card the owner tags `full`. It is
  the same base draft plus the two independent readers, recorded in
  `reviewPasses`:
  `beginner` must begin `Fresh agent: <agent-id>. Skill: rli5.` and the agent
  must be fresh; `adversarial` must begin
  `Author model family: <family>. Adversarial model family: <family>.` and
  identify the actual reviewer with `Fresh agent: <agent-id>.` in its summary.
  Owner direction (2026-09-05): the reviewer must be fresh, but may use the
  same model or model family as the author. Family labels are provenance,
  not an independence test. Follow the owner's requested reviewer and routing.
  The four self-graded summaries (base, boil the ocean, hybrid, cooperative)
  are retired: a drafter grading its own draft is not a review.
- The scaffold (`tower decision scaffold <card> --id <D-…>`, carded) fills
  `surface.trio.current` from the card's probe run and `surface.trio.wild`
  from the card's cited evidence; the author still writes every word.

"Simple ballot" is not a profile name. `/simple` applies to both profiles.
Ratified decisions are immutable history.

## The reading surface (write it first)

The owner reads the surface and decides from it; the long-form fields below
are provenance behind a fold. Owner law (2026-09-02): "I want to see current,
proposed, and the best in-the-wild option side by side; the comparison, what
the recommended option loses and gains, and why not the other options. The
rest can be hidden." Tower refuses a ballot without a valid `surface`.

`surface` is an object:

- **`gist`** — one question, under 22 words: what is being decided?
- **`lesson`** — one plain paragraph, under 70 words: the concepts a beginner
  needs, what Jet has today, what stays owed whichever option wins.
- **`trio.current`** — `{note, code}`: what a Jet user types and sees today
  (the real error or the workaround). `trio.wild` — `{lang, note, code}`: the
  best-in-the-wild tool doing the same job, real API, cited in a code comment.
- **`options[]`** — one per ballot option, same keys and order:
  `{key, name, gist, gains[1-3], losses[0-3], proposed: {code}}`. `proposed.code`
  is the same workload as `trio.current` and `trio.wild`, at most 14 lines.
  **The recommended option is `A` and listed first** in every new ballot, so
  it sits next to the current and in-the-wild code (owner, 2026-09-02; Tower
  refuses a new ballot that recommends anything else; ballots already open
  keep their letters). Gains and losses are the reality only: each bullet is a
  concrete fact the long form or the code supports. Never pad a list; an
  option with no known loss lists none.
- **`recommendation`** — `{rec, why, gains[], losses[{loss, whyUnavoidable}], whyNot[{key, reason}], tradeoff}`.
  `rec` equals the ballot `rec`; `why` under 40 words; `whyNot` names every
  losing option. Every remaining loss carries `whyUnavoidable`: the concrete
  reason it cannot be designed out (physics, a ratified law, a measured cost,
  or "removing it brings back option B's loss X"). A loss without that reason
  means the design is not finished. Tower refuses it. Use plain strings only
  in the legacy `surface.options[*].losses` arrays.

### The design-away pass (owner law, 2026-09-02)

"The recommendation is not good enough until all losses are designed away, or
until it is impossible to design any more of them away. Spend the effort up
front so we do not pay for them later; get the best of all worlds."

Before writing `rec`, take the leading option and, for each loss on it:

1. Ask what change to the option removes the loss without adding a new one.
   Steal from the other options and from the in-the-wild tools; this is the
   synthesis the owner expects, not a separate "hybrid" option.
2. If a change works, make it: the option's code, gist, gains, and the long
   form all change, and the loss is deleted, not softened.
3. If nothing works, write `whyUnavoidable` in one sentence a beginner can
   check, naming what was tried when that is not obvious.
4. Repeat until every loss is gone or carries its reason. Only then recommend.

A recommendation whose losses were never attacked is a draft, not a ballot.
Record what the pass changed in the card log so the next reader sees the
work, not only the result.

How the owner sees it (Tower Focus Mode and the html skill's ballot page):
the question, the lesson, then the code stacked full width with no sideways
scrolling: `Current`, `In the wild` (side by side only when both fit), then
the options with `A` first, each the same shape: name, gist, gains and losses,
proposed code. Then the recommendation and, folded, the long form.

Caps Tower enforces: sentences under 24 words, bullets under 14 words, the
whole surface under 430 words of prose, and no project jargon ("ratchet",
"seam", "facet", "substrate", "tier parity", "Ring 0/1"). Say the plain thing:
"core owns the meaning", "a package backend", "the same answer on every run
mode", "field group". Exemplar: `D-M-SIGNAL1`.

Write the surface before any long-form field. The long-form fields restate
and ground it; they never re-decide it. `gist`, `lesson`, option `detail`, and
`recommendation.why` carry the surface wording (option `detail` is the option
gist plus its gains and losses); anything longer goes in `technical`.

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
  the recommendation still accepts after the design-away pass. `whyNot` contains
  one `{key, reason}` per losing option. `tradeoff` names only losses that
  carry an unavoidability reason. Never use empty phrases such as "best
  balance", and never accept a loss the pass did not attack.
- **`group`** — one of the project's `decisionGroups` (see `.tower/config.json`)
  so the queue stays organized.

## Ground comparisons in real use

For user-facing syntax, workflow, or API choices, search current external
practice before ranking options. Start with primary specifications and official
documentation. Then measure adoption or reception with a reproducible public
source when one exists.

State what each number measures. A package manager's downloads or stars measure
the tool, not one feature. A code-search count measures indexed matches, not
users. Give the query, date, exclusions, and known limits. Never present a
repository census as market evidence.

Identify the strongest praised design, why people praise it, and its recorded
complaints. Keep the useful mechanism. Repair its observed faults instead of
copying its accidental syntax. Put source URLs and measured signals in
`comparisons`.

Make each option internally cohesive. When one canonical mechanism can serve
beginners and experts, present that complete mechanism as a normal option. Do
not create a separate hybrid option unless it is a real final design.

## Build and review in this exact order

1. **Surface and base draft** — write the reading surface, then the complete
   long form: every credible option, worked code on the same workload, real
   gains and losses, in-the-wild grounding. Fold mere tactics into their parent
   options; add a genuinely distinct option when the search finds one.
2. **Design-away pass** — attack every loss on the leading option as described
   above; change the option, delete the loss, or write `whyUnavoidable`. Then
   write `rec` (always `A`, listed first), `why`, `whyNot`, `tradeoff`.
   A short ballot ships here.
3. **Beginner** (full only) — dispatch one fresh agent that had no role in
   the earlier steps. The brief must invoke `/rli5`, provide the complete
   ballot, and use the true-beginner profile unless the owner named another
   reader. The agent attempts explain, predict, modify, and derive tasks and
   returns the RLI5 friction table. Revise the ballot and record its exact
   agent id after `Fresh agent: <agent-id>. Skill: rli5.`
4. **Adversarial** (full only) — use a separate fresh agent to attack
   the recommendation, assumptions, evidence, failure modes, and every
   `whyUnavoidable`. Repair the ballot; change the recommendation if it does
   not survive; re-run the design-away pass on anything the attack reopened.
   The agent must not have authored the ballot or performed its beginner pass;
   a different model family is optional. Record the actual agent ID.

The two summaries are evidence, not status labels: say what was tested,
added, removed, or repaired. After the adversarial pass, check that
`recommendation.whyNot` still covers every losing option and that no loss
lost its reason.

Focus Mode shows the beginner pass in blue and the adversarial pass in orange,
the recommendation in blue, and reasons against alternatives in muted red.
Labels and icons carry the same meaning when color is unavailable.

## Mechanics

```
mkdir -p ~/.cache/jet-luna
# A card probe and cited reference are copied into a new draft:
tower decision scaffold #12 --id D-CACHE1 --out ~/.cache/jet-luna/ballot.json
cat > ~/.cache/jet-luna/ballot.json <<'EOF'
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
    "gains": ["Price changes reach readers without waiting for a timer."],
    "losses": [{ "loss": "Every writer must emit a purge event.", "whyUnavoidable": "Only the source knows when a price changes." }],
    "whyNot": [{ "key": "A", "reason": "A time limit still serves stale prices until its clock expires." }],
    "tradeoff": "Writers must emit purge events because only the source knows when a price changes."
  },
  "reviewPasses": {
    "beginner": "Fresh agent: reader-17. Skill: rli5. The beginner pass found one undefined term and one ambiguous example. Both were repaired.",
    "adversarial": "Author model family: family-a. Adversarial model family: family-a. Fresh agent: reader-18. The adversarial review attacked the recommendation and repaired one failure mode."
  }
}
EOF
tower decision add --file ~/.cache/jet-luna/ballot.json --by <me>
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
- Use `short` only for one mechanism with at most three options. No
  `shortAuthorizedBy` is needed. Syntax groups, `carve-out` cards, and `full`
  cards require the two-reader `full` profile.
