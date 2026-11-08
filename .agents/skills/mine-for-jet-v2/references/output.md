# Compact output and publication boundary

Read the shared audit-dispositions contract. Report-only creates no Tower cards,
decisions, ballots or implementation. Chat-only stays chat-only. Repository
publication requires authorization, except the prior-art rows below, which
every run adds in every mode. Markdown is default, HTML only on request.

## Prior-art registry

Every source a run mines gets a row in the Source index of
`Docs/spec/reference/prior-art.md` before hand-in, in a section for the run:

```md
### Subject (mined YYYY-MM-DD)

Run: `RUN_ID`. Report: [report-name](../../research/report-name.md).

- https://www.youtube.com/watch?v=VIDEO_ID
- Linked page the run read in full — https://example.org/source
```

Write one row per source URL, in the spelling of the `ref` in `coverage.json`;
a label before ` — ` says what the row is when the URL alone does not.
`check_coverage.mjs` fails while the run ID is absent or any http(s) `ref` has no
row (identity as in `check_sources.py`); a `ref` of `local:PATH` marks the run's
own captures and needs none. Add the `Report:` link when the report is
published; a run with no retained report names its Tower records instead.
Workers add the section in the checkout they hand in, and the orchestrator lands
it with the report or, in report-only and chat-only runs, on its own. Never name
an unmined parent stream or linked source by URL or ID in the section: the
registry treats any occurrence as tracked.

## Compact front, retained evidence

1. Plain verdict: strongest useful lesson, actual Jet deficit and deliberately
   unproposed ideas with reasons.
2. Three to seven prioritized findings when sufficient; more to cover scope,
   never an arbitrary cap hiding findings.
3. One alignment table:

| Source mechanism | Current / planned (ratified-unbuilt) / in-flight / proposed | Incremental contribution | Existing / new / no-action disposition | Evidence and acceptance |
| --- | --- | --- | --- | --- |

4. Short owner questions **only for genuine remaining choices**. Scratch READY
   and a reviewed payload are not approval or a posted/ratified decision.
5. Coverage/confidence: the `check_coverage.mjs` output (per-source and
   per-segment ledger/claim densities, justified low-density segments), every
   recall audit's seed, sample, misses and outcome, versions read, retrieval
   versus body review versus extraction, contradictions, binary/tree identity
   and whether examples were exercised. Keep the exact owner-question matrix;
   no generic victory paragraph substitutes for an answer.
6. Retained evidence: `coverage.json`, `ledger.json` and normalized
   `claims.json` (plus recall audits), essential exact snippets,
   commands/oracles, primary locators, counterexamples, measurement limits,
   source independence, rule-9 evidence and owner-feedback corrections. Retain
   reviewer findings, resolution and identity; link large evidence rather than
   dumping transcripts or repeated per-language praise.
7. Strongest remaining unverified assumption, if any.

Do not duplicate Tower phases/plans as a living catalog or commit a generated
Jet-state snapshot. Checkout identity/direct refs belong in dated claims. Code
presence is not exercised behavior; backend implementation code is not Jet idiom.
Keep audience participants anonymous unless identity materially affects evidence.

## Required dispositions

A retained report includes `audit-dispositions:v1` exactly as the shared contract
specifies, one row per finding. Existing card/decision citations are read-only.
Use no-action with a concrete reason only when no record owns the finding and
no new write is authorized; scope deferral/report-only restrictions are reasons,
not fabricated rejected law. Preserve rejected/conflicting and evidence-gap
findings with their exact reasons.

## Normal Tower-enabled mining

Alignment, seven gates, rule-9 research, dedupe and **fresh-context review precede
all new publications**. No automatic feature factory:

- Attach new source witnesses to existing matching cards/decisions; no extra
  card merely to give an already-owned idea somewhere to live.
- For remaining reviewed actionable findings, one card per coherent end-to-end
  deliverable and one ballot per independently decidable owner-only choice.
- Ratified implementation/repair consequences attach to existing owners, not votes.
- Use dependencies when order matters; freeze only on exact owner instruction.
- Workers return reviewed payloads; only the orchestrator performs supported
  non-serve CLI writes, lint and readback. Never hand-edit `.tower`.
- Publish the authorized dated Markdown synthesis under `Docs/research/` using
  the shared non-serve `docs add` procedure. In report-only runs publish only
  the authorized report, without work/decision writes. No unowned “revisit later”
  action in normal Tower-enabled mode; evidence gaps get honest dispositions.

A report can be complete while implementation is not. Source captures remain
under `$JET_SCRATCH_ROOT/mine/<run-id>/`; delete disposable captures only after
essential reproducible evidence and normalized claims are retained with the
result. A scratch-only claims JSON is not a durable deliverable. Do not require
an owner-rejected mandatory real-user study, invent adoption data, expand the
corpus, or start implementation at close.
