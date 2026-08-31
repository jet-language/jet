---
name: rli5
description: >
  Read Like I'm 5 — the inversion of ELI5. Instead of simplifying an explanation
  for a beginner, the agent BECOMES the beginner: read, think, interpret, and
  review any artifact (code, docs, error messages, UIs, APIs, configs) as a
  genuine newcomer with no prior knowledge, and report where the artifact fails
  to teach itself. Use when user says "rli5", "read like I'm five", "beginner
  lens", "how would a newcomer read this", or when a review should surface
  learnability friction before a human sees it.
---

ELI5 simplifies output for a reader. RLI5 inverts it: simulate the reader and
measure what the artifact fails to teach. Output is friction findings, never a
rewrite.

## Persistence

ACTIVE for the whole read/review it was invoked for. Off: "stop rli5" /
"normal mode". While active, every interpretation goes through the beginner
lens — no silent reversion to expert reading mid-artifact.

## The reader you become

Default profile: **true beginner** — this is your first time seeing anything
like this artifact. You know how to read and follow instructions; you know no
programming languages, no jargon, no conventions, no tooling, no folklore.

The caller may override the profile (e.g. "rli5 as someone who knows Python",
"rli5 as a sysadmin", "rli5 as an unattended agent"). The protocol below is
identical for every profile; only the permitted priors change.

## The no-priors protocol

You know too much. Simulated ignorance drifts into polite pretending, so the
protocol demands evidence instead of opinion:

1. **Derive everything from the page.** Every symbol, keyword, term,
   abbreviation, and convention must be explained by the artifact itself (or a
   glossary the caller explicitly permits). For each construct you claim to
   understand, show the derivation chain: "X appears next to Y which was
   defined above, so X must mean…". No chain = underivable = finding.
2. **Outside knowledge is a logged event, not a resource.** Every time the
   correct reading requires something not on the page, log it as friction.
   For non-default profiles, appeals to the permitted priors are allowed but
   still logged — "makes sense only if you already know Python" is data.
3. **Attempt real tasks; do not just opine.** In order:
   - **Explain** — what does this do / what is this for?
   - **Predict** — what happens when it runs / when the error path fires /
     when the button is pressed?
   - **Modify** — one small concrete change the caller names; actually write
     or describe it.
   - **Derive** — for each term the caller probes, show the chain.
   A stumble only counts when the task outcome shows it: wrong prediction,
   failed modification, missing chain. Feelings are recorded, tasks are scored.
4. **React verbatim.** Record every wait, sigh, re-read, surprise, and "oh,
   that's nice" as it happens. Preference remarks are product data.
5. **Never fix while reading.** The beginner cannot rewrite the artifact.
   Suggested fixes go in the findings table at the end, out of character.

## Output

One friction table, then stop:

| # | location | stumble | task evidence | derivable from artifact? | severity | suggested fix |
| --- | --- | --- | --- | --- | --- | --- |

Severity by centrality: a stumble on the artifact's main path outranks one in
a rare corner. Include the good news — things the artifact taught effortlessly
— as keep-rows, marked `keep`.

Technical terms are welcome in fixes; the finding is never "uses a technical
term", it is "uses a term the artifact never teaches". The goal is not to dumb
anything down — it is to catch unnecessary obtuseness before a human hits it.

## Boundaries

RLI5 changes how you READ, not how you write findings: the friction table and
any surrounding report are normal expert prose. Do not roleplay a child's
voice; the beginner is a lens, not a persona costume. Code blocks, error
strings, and quoted text stay verbatim.
