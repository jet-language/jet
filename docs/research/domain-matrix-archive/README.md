# Domain matrix archive (2026-09-02)

**Status:** research archive. The domain-matrix slate it fed (Tower epoch e15: 46 ballots, 219 cards) was withdrawn by the owner on 2026-09-02 and its ballots and cards deleted with the owner's one-time permission. This directory keeps the research so nothing is re-mined from scratch. It is not a plan and it decides nothing.

## What the owner ruled instead

Core keeps four things: the language primitives a library cannot add for itself (syntax, the type system, memory and effects, execution tiers, the runtime); the ecosystem and developer-experience tools (build, packages, devtools, live loop, tests, receipts, diagnostics, editor support); the core library that ships today; and the shared data types two libraries must agree on (one table, one unit, one receipt). Niche libraries leave core. A library author must have every tool needed to build them in Jet.

Each former mechanism is re-judged by probes: if it can be built today with existing Jet, it becomes an executable example at most; if not, the missing piece is proposed as a primitive, never as the niche library. Eight critical areas (web, games, CLI and scripts, data analysis, backend services, AI/ML applications, GUI apps, embedded) get full "everything a builder needs" probes and, where needed, first-party batteries. The performance gate's required cells are the foundations plus one real workload per critical area; a niche gets a cell only when Jet ships a battery for it.

The proposal that this archive fed is kept for history at `docs/proposals/domain-matrix.md` (marked superseded).

## Contents

| File | Rows | What it is | Read it with |
|---|---|---|---|
| `SUMMARY.md` | | The global merge summary the slate was minted from: 108 domains, 127 mechanism ids before merges, 11 families | plain text |
| `families/<family>.md` | 11 | One synthesis per family: shared mechanisms, incumbents, beat vectors, defects | plain text |
| `ballots.json.gz` | 46 | Every ballot record as it stood in Tower before deletion: options, code, comparisons, recommendation, review passes, and the reading surface | `zcat ballots.json.gz \| jq '.[] \| {id, rec, gist}'` |
| `mechanisms.json.gz` | 127 + 108 | `mechanisms` (id, families, domains served, jet state, definitions), `domainMatrix` (one row per domain: family, incumbent, gauntlet coverage, P0 counts, top beat vector), `defects` (the 9 substrate defects), `overlapCandidates` (the merges) | `zcat mechanisms.json.gz \| jq '.mechanisms[] \| {id, domain_count}'` |
| `census.jsonl.gz` | 5,698 | One line per domain feature: what the best tool does (`feature`, `why_great`, `needed`), the mechanism it maps to, Jet's state with evidence, priority, owner gate | `zcat census.jsonl.gz \| jq -c 'select(.domain=="acoustics-audio-engineering")'` |
| `claims.jsonl.gz` | 3,171 | Source-linked claim ledger: claim, source, locator, Jet evidence, correction, stance, confidence | `zcat claims.jsonl.gz \| jq -c 'select(.confidence=="high")'` |
| `performance.jsonl.gz` | 108 | Per domain: the incumbent workload and what a benchmark would measure | `zcat performance.jsonl.gz \| jq -c '.'` |

Sizes are gzip level 9; total about 2.5 MB. The machine-local working set (per-domain reports, probes, reviews, worker briefs) lived under `~/.cache/jet-luna/dx2/` and is not part of the repository.

## Provenance

Mined 2026-09-01/02 by Luna max workers (108 domain censuses, 11 family syntheses, one global merge), balloted and reviewed 2026-09-02, ratified in part by the owner the same day, then withdrawn after the owner's rescope. Every row names its sources; nothing here was verified beyond the evidence it cites.
