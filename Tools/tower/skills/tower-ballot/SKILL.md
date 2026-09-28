---
name: tower-ballot
description: >-
  Prepare an unresolved owner-only Tower choice as a short or full ballot the
  owner can ratify from its reading surface. Use for new public syntax, APIs,
  commands, dependencies, invariant or product changes, visual direction, or
  another explicit owner gate; routine work under an approved contract is not a
  ballot.
---

# Tower — raise a ballot-ready decision

## Contract

- **Requested outcome:** A complete `short` or `full` ballot that lets the owner
  decide from the reading surface alone.
- **Supplied inputs:** The card and unresolved choice, ratified decisions,
  current and in-the-wild examples, project priorities, and the required
  profile.
- **Allowed child result:** In `full`, one fresh beginner reader returns RLI5
  friction and a separate fresh adversarial reader returns challenge findings.
  Neither reader authors, decides, publishes, or opens follow-on work. The
  readers may share the author's model family.
- **Completion owner:** `tower-ballot` owns drafting and repair. The owner owns
  the choice and ratification.
- **Return point:** Reader results return to the ballot's readiness and
  recommendation checks.
- **Stopping condition:** Stop when the selected profile is complete, every
  loss is removed or justified, and Tower accepts the ready ballot. Do not
  implement the choice.

## Use this route only for an owner gate

An unresolved choice belongs in a `decision` only when the owner must choose
among alternatives. Examples are:

- new public syntax, API, command, or external standard-library dependency;
- an invariant or safety change, including an I1–I9 carve-out;
- product behavior, epoch or scope direction, or another explicit owner ruling;
- visual, user-experience, or developer-experience taste and direction; or
- a card the owner explicitly marks `full`.

Do not ballot routine implementation under an approved contract. A plan may
propose implementation details, and an implementer may choose among them,
when no owner gate changes. A performance-motivated surface choice still needs
an executable candidate/plain two-program cell in the canonical gauntlet
matrix before ratification. Apply the standing comparator and keep every loss
carded.

## Write user-visible prose simply

Use the `simple` skill for every user-visible word: headings, context, stories,
option names and details, technical notes, comparisons, recommendations,
review summaries, instructions, and prose inside examples. Code stays valid
code. Use common words, one idea per sentence, define needed terms, expand an
acronym once, and lead with user impact. General Tower limits are 32 words per
sentence and 90 words per paragraph; the reading surface has stricter caps
below.

## Choose the profile

Ratified 2026-09-02 (D-BALLOT-PROCESS1 = C): new full ballots use two fresh
reader passes, `beginner` and `adversarial`. Stored process 2/3 ballots keep
their historical six-pass records and render unchanged; never rewrite them.

- **`short`** is the default for one mechanism with at most three options. It
  is one complete base draft: reading surface, every decision field, complete
  options, recommendation, and no review passes. Set `ballotMode: "short"`;
  `shortAuthorizedBy` is not needed.
- **`full`** is required for new syntax (anything touching `Syntax.rs`), an
  I1–I9 invariant carve-out, or a card the owner marks `full`. It is the same
  base draft plus the two independent fresh readers recorded in
  `reviewPasses`.

A full reader's `beginner` summary must begin exactly:
`Fresh agent: <agent-id>. Skill: rli5.` The beginner attempts explain,
predict, modify, and derive tasks and returns a friction table. The
`adversarial` summary must begin exactly:
`Author model family: <family>. Adversarial model family: <family>.` It must
identify the actual reviewer with `Fresh agent: <agent-id>.` The adversarial
reader is separate from both the author and beginner. A different model family
is optional. Follow the owner's requested reader and routing.

## Reading surface

Write the surface before long-form fields. The owner decides from the surface
alone; long form is folded provenance and must not re-decide it. Tower refuses
a ballot without a valid `surface`.

- **`gist`** — one question under 22 words.
- **`lesson`** — one plain paragraph under 70 words: needed concepts, what Jet
  has today, and what remains owed whichever option wins.
- **`trio.current`** — `{note, code}` showing what a Jet user types and sees
  today, including the real error or workaround.
- **`trio.wild`** — `{lang, note, code}` showing a real tool doing the same
  job. Cite the source in a code comment or note.
- **`options[]`** — each option has the same ordered keys:
  `{key, name, gist, gains[1-3], losses[0-3], proposed: {code}}`. The proposed
  code uses the same workload as current and in-the-wild code and is at most
  14 lines. Gains and losses are concrete facts, never padding.
- **`recommendation`** — `{rec, why, gains[], losses[{loss, whyUnavoidable}],
  whyNot[{key, reason}], tradeoff}`. `rec` equals the ballot `rec`; `why` is
  under 40 words; `whyNot` names every losing option. Every remaining loss has
  a beginner-checkable `whyUnavoidable` reason.

For every new ballot, list the recommended option as `A` first and set `rec` to
`A`. Tower rejects another new recommendation. Existing open ballots keep
their letters. Keep losses as plain strings in legacy `surface.options[*]`.

The surface has these caps: sentences under 24 words, bullets under 14 words,
and prose under 430 words. Avoid project jargon such as "ratchet," "seam,"
"facet," "substrate," "tier parity," or "Ring 0/1." Say the plain thing,
such as "core owns the meaning" or "field group." Exemplar: `D-M-SIGNAL1`.

## Long form and readiness

Long form restates and grounds the surface. It never introduces a new choice:

- `gist`, `lesson`, `story` (a named person and why this exists), and realistic
  `inWild` code;
- `options[]` with `{key, name, detail, code}` for every genuine alternative;
- optional `technical` for exact protocol, type, ABI, schema, or lowering law;
- `comparisons[]` as `{lang, note, code}` when comparison can inform the choice;
- `rec` and `recommendation:{why, whyNot, tradeoff}`.

Each option has a worked example of what the person types and sees, including
an error when that is the point. Do not offer several derivative spellings of
one idea or a separate hybrid that is not a real final design. The
recommendation explains why the winner serves this decision, why every other
option loses, and which downside remains after the loss pass. `tradeoff` names
only losses with `whyUnavoidable`; implementation difficulty never ranks an
option.

A ballot is ready only when the owner can decide without asking the drafter,
all credible options are internally cohesive, source provenance is recorded,
and every recommended loss is removed or carries its reason. Read
[authoring details](references/authoring.md) for the design-away acceptance
and contextual comparison evidence. Use the [complete exemplar](references/exemplar.md)
for the corrected JSON shape (`A` first, recommendation `A`, and `whyNot` for
`B`).

## Review and visual handoff

Do not repeat dispatch or closeout cadence in this skill. For full ballots, use
the canonical [fresh-reader mechanics](../../../../.agents/skills/orchestration/references/ballot-review.md)
for the complete-ballot brief, RLI5 tasks, adversarial attack, receipts, and
provenance.
After both fresh passes, repair material findings, re-check every loss, and
confirm `recommendation.whyNot` still covers every loser. Each reader summary
is evidence, not a status label: state what was tested, added, removed, or
repaired. Preserve every loss reason after the adversarial pass. The
orchestration [closeout reference](../../../../.agents/skills/orchestration/references/closeout.md)
applies only when a later campaign reaches milestone closeout.

Tower Focus Mode and the ballot page show the question, lesson, then code
stacked at full width without sideways scrolling: `Current`, `In the wild`
(side by side only when both fit), then `A` first and every option with the
same shape: name, gist, gains, losses, and proposed code. Recommendation and
long form follow. The beginner pass is blue, adversarial orange,
recommendation blue, and losing reasons muted red. Labels and icons carry the
same meaning without color.

For a two-option ballot, the ordering and recommendation agree:

```json
{
  "options": [{ "key": "A", "name": "TTL per entry" }, { "key": "B", "name": "Event-driven purge" }],
  "rec": "A",
  "recommendation": { "whyNot": [{ "key": "B", "reason": "It requires every writer to publish correctly." }] }
}
```

The [complete exemplar](references/exemplar.md) carries the full required
surface and long-form fields.

## Mechanics

Scaffold into a scratch file. Finish the applicable fields and required reader
evidence there before submission: even a full draft must pass Tower's reader
metadata validation. The scaffold retains `draft: true`; adding it does not
expose the ballot to the owner.

```sh
mkdir -p ~/.cache/jet-luna
tower decision scaffold '#12' --id D-CACHE1 --out ~/.cache/jet-luna/ballot.json
```
The scaffold may seed `surface.trio.current` from the card probe and
`surface.trio.wild` from cited evidence. The author still writes and checks
every word.

After authoring and review, submit the complete draft, then explicitly make it
ready:

```sh
tower decision add --file ~/.cache/jet-luna/ballot.json --by <agent>
tower decision update D-CACHE1 --ready --by <agent>
tower decision show D-CACHE1 --json
tower card show '#12' --json
```

Confirm the decision is non-draft and open, and the card's computed lane and
actor expose the choice to the owner. A stored draft is not owner-decide.
Keep incomplete work in the scratch file; use `--ready` only when the owner
can decide from the ballot alone. Nudge the owner only when urgent.
New ballots appear through the live SSE board; web push is removed.

## Rules

- Check `tower decision list --json` before choosing a unique stable `D-…` id.
- Read ratified decisions first. Never invent an option that contradicts one.
- The owner alone ratifies. A ratification comment containing a question is
  not a clean pick; address it before building.
- An owner question requests a ballot edit:
  `tower decision update <id> --file …`, then reply on the board.
- Ratified decisions are immutable history. Leave the card in `decide` until
  the owner ratifies it.
- Use `short` only for one mechanism with at most three options. New syntax,
  invariant changes, and owner-tagged full cards require the full two-reader
  profile. Do not invent `shortAuthorizedBy`.
