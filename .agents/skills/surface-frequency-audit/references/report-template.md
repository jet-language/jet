# Report contract

Write one Markdown report at `docs/audits/surface-frequency-audit-YYYY-MM-DD.md`. Apply the `simple` skill. Lead with the result.

Keep the main narrative below 4,000 words. Keep the executive summary below 800 words. Tables, citations, and collapsed appendices do not count toward this limit.

Use this structure:

```markdown
# Surface frequency audit — YYYY-MM-DD

## Executive summary

State the main result, the strongest evidence, the largest Jet opportunities, and the most important limits.

### Decision view

| Rank | Job or surface | Why it matters | Evidence strength | Jet action |
| --- | --- | --- | --- | --- |

## What people do most

Rank language-agnostic jobs and operations. Show project prevalence, opportunity share, breadth, difficulty, and confidence.

## Which surfaces they use

Rank exact syntax, operators, APIs, idioms, and tooling paths inside comparable groups. Do not mix parent and child groups.

## Beginner adoption path

Show frequent entry and general tasks, first-use friction, repeated workarounds, and the effect of the `1.15` beginner factor.

## Expert production path

Show frequent advanced tasks, safety and control value, production friction, and required expert opt-ins.

## Jet recommendations

Use `Keep`, `Reduce friction`, `Add`, `Remove`, or `Study`.

For each recommendation, show:

- Measured evidence
- Jet-specific inference
- Priority components and sensitivity rank
- Beginner effect
- Expert effect
- Safety, control, and diagnostics gate
- Tower status with exact IDs

Discuss at most 20 recommendations.

## Keep

Name Jet defaults and surfaces that the evidence supports.

## Watchlist

List lower-priority or weak-confidence findings without full proposals.

## What changes the ranking

Summarize deduplication, weighting, parser, symbol-resolution, and leave-one-out sensitivity results.

## Coverage and limits

State corpus size, languages, domains, strata, parsed files, normalized lines or tokens, source sites, skipped files, weak cells, unavailable cells, and the static-source limit.

<details>
<summary>Complete coverage matrix and long-tail results</summary>

Put complete compact tables here.

</details>

## Methods and provenance

State the frozen method digest, collection dates, parser and tool versions, sampling rules, source pins, exclusions, deduplication rules, formulas, and review procedure.

<details>
<summary>Corpus manifest</summary>

List canonical source IDs, links, commit or content pins, language, domain, stratum, and inclusion status.

</details>

## Sources

Link exact repositories, official specifications, documentation, telemetry sources, and local Jet evidence.
```

## Prose rules

- Use short sentences and concrete verbs.
- Define a technical term before its first use.
- Put one claim in each sentence.
- Put evidence next to the claim that it supports.
- Use tables for repeated exact comparisons.
- Use no marketing language, rhetorical filler, or unsupported certainty.
- Keep source quotations short. Prefer paraphrase.
- Label inference, weak evidence, and missing evidence.

## Report-only rule

The report recommends. It does not act.

Do not add:

- Tower cards or ballots
- Acceptance criteria
- Implementation plans
- Commits or pull requests
- A separate data ledger
- A second report

Temporary checkpoint files may exist while the run is incomplete. Remove them after the reviewed report is installed.
