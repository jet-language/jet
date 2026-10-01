# Elegance census

The census measures every layer at the same depth and ends with the owner's choice of dive target. Measure everything before you judge any of it; hold redesign ideas as candidates until every layer is counted.

## Count each layer

For each of the five layers, read its executable home and record one ledger row:

| Field | Content |
| --- | --- |
| Home | The paths counted, with the command or search used, so the next run can repeat the count. |
| Parts | The count by kind, such as 58 keywords and 11 sigils, or 412 codes in 3 shapes across 19 groups. |
| Exceptions | Each exception with `file:line` and one line on what rule a user must add to predict it. |
| Laws | The layer's current organizing laws, one sentence each, whether or not written down. Write `none` when members are merely listed. |
| Law coverage | The share of parts a newcomer could predict from those laws alone. |
| Clicks | Laws that already land. Celebrate them; they are findings too. |
| Latent clicks | A law the layer almost obeys, with the exceptions that block it. |

Count from registries and source, not from specs or docs. Retired, reserved, and parser-accepted-but-ignored forms count as parts with that fact attached, because a user can meet them. Probe the running binary through `Tools/agent/jet-env` whenever a count depends on behavior rather than a table. Mark a field `unknown` with a reason instead of estimating it.

Diagnostics get extra attention because grouping is their whole shape: count the code shapes, the prefixes, the numeric ranges and what each range means, gaps and strays inside a range, and whether a reader can guess a code's family from its number.

## Map the rhymes

Build a rhyme map over the ten layer pairs, plus one row per layer for rhymes inside it. For each pair, record the strongest existing rhyme, near rhyme, or false rhyme with evidence, or `none` after a real search. Look for these parallels:

- groups in one layer that mirror groups in another, such as diagnostic ranges, checking stages, Core modules, and CLI commands;
- names that recur across layers with one meaning;
- orderings, numberings, and prefixes that could share one law;
- the same concept spelled differently in the language, the API, and the CLI.

A false rhyme is a finding even when nothing replaces it.

## Rank candidates and ask

List each dive candidate with the target, the draft one-sentence law, its estimated area verdict, its estimated click verdict, the kill-check risk, and the rules it would amend. Show the two verdicts side by side; do not merge them into a score. Order the list by how many exceptions the law would explain, and say that this is only an ordering.

Write the census into the report draft (see [`report.md`](report.md)), then show the owner the ledger summary, rhyme map highlights, and candidate list, and ask which candidate to dive into using the host's question tool. Offer the top candidates, "census only", and free input. Wait for the answer.

## Census stop

The census is complete when all five ledger rows have counts, exceptions, laws, and coverage or an honest `unknown`; every layer pair has a rhyme row; candidates list both verdicts; and the owner has chosen a dive target or census only. If the owner chooses census only, go to [`report.md`](report.md).
