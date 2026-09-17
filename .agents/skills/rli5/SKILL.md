---
name: rli5
description: >-
  Read an artifact as a genuine beginner and report where it fails to teach
  itself. Use only when the user asks for rli5, “read like I'm five,” a beginner
  lens, or a learnability review. Findings only; do not rewrite the artifact.
---

# RLI5

RLI5 inverts ELI5. ELI5 simplifies output for a reader; RLI5 simulates the reader and measures what the artifact fails to teach. Output friction findings, never a rewrite.

## Persistence

ACTIVE for the whole read or review it was invoked for. Stop on `stop rli5` or `normal mode`. While active, every interpretation uses the beginner lens; do not silently return to expert reading mid-artifact.

## Reader

Default profile: a true beginner seeing this kind of artifact for the first time. The reader can read and follow instructions but knows no programming languages, jargon, conventions, tooling, or folklore.

The caller may override the profile, such as “rli5 as someone who knows Python”, “rli5 as a sysadmin”, or “rli5 as an unattended agent”. The protocol stays the same; only permitted priors change.

## No-priors evidence

1. **Derive everything from the artifact.** Every symbol, keyword, term, abbreviation, and convention must be explained by the artifact itself or a glossary the caller explicitly permits. For each construct you claim to understand, show the derivation chain: “X appears next to Y, which was defined above, so X must mean …”. No chain means underivable and becomes a finding.
2. **Log outside knowledge.** Every time the correct reading needs knowledge not on the page, log it as friction. Permitted priors for a non-default profile are allowed but still logged: “This makes sense only if you already know Python” is data.
3. **Attempt real tasks in order.**
   - **Explain:** what does this do or what is it for?
   - **Predict:** what happens when it runs, an error path fires, or a button is pressed?
   - **Modify:** make or describe one small concrete change named by the caller.
   - **Derive:** show the chain for each term the caller probes.

   A stumble counts only when the task outcome shows it: wrong prediction, failed modification, or missing chain. Feelings alone are not findings; tasks are scored.
4. **Record reactions verbatim.** Record every wait, sigh, re-read, surprise, and “oh, that's nice” as it happens. These are simulated reactions from this lens, not actual user-study observations. Never claim that a real user said or felt them.
5. **Do not fix while reading.** The beginner cannot rewrite the artifact. Put suggested fixes in the findings table, out of character.

## Output

Return one friction table, then stop:

| # | location | stumble | task evidence | derivable from artifact? | severity | suggested fix |
|---|---|---|---|---|---|---|

Severity follows centrality: a stumble on the main path outranks one in a rare corner. Include good news as keep-rows marked `keep`.

Technical terms are welcome in fixes. A finding is not “uses a technical term”; it is “uses a term the artifact never teaches”. The goal is to catch unnecessary obtuseness before a human hits it.

## Boundaries

RLI5 changes how you read, not how you write findings. The friction table and any surrounding report use normal expert prose. Do not roleplay a child's voice. The beginner is a lens, not a persona costume. Code blocks, error strings, and quoted text stay verbatim. Findings only: do not silently rewrite, repair, or open an implementation workflow.
