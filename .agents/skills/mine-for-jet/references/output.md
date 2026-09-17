# Mine for Jet: output and permissions

Read `.agents/skills/_shared/audit-dispositions.md` before publication. The
owner's requested outcome selects the write boundary.

For a retained report, include the required `audit-dispositions:v1` table.
Use `card` or `decision` when the finding already has that Tower record; citing
it is read-only. Use `no-action` with a concrete reason only when no existing
record covers the finding and new Tower writes are not authorized.

## Report-only override

Normal mining has a method-owned Tower mode: every actionable finding becomes a
card or ballot in the same run. An explicit `report-only` request overrides that
default. In report-only mode, write the requested report, cite existing Tower
records read-only, and create no cards, decisions, ballots, or implementation
edits. A chat-only/no-file request stays in chat without a retained report or
disposition-table scaffold.

Repo edits remain unauthorized unless the owner explicitly requests them.
Report completion does not ratify or implement a recommendation.

## Tower-enabled mode

When the owner has not selected report-only and the normal mining outcome is in
scope:

- read Tower status and search for duplicates first;
- log every finding whose action is not `none` now;
- create one card per coherent deliverable, not one card per bullet;
- add one ballot per independently decidable owner choice, with full
  tower-ballot fields and worked options;
- link cards through `blockedBy` when order matters;
- freeze cards only when the owner asks; record the exact instruction through
  the supported CLI and never hand-edit `.tower/*.json`;
- do not create a ballot for a ratified choice; attach work to its existing
  decision or card;
- run `tower lint` after writes and read cards and decisions back.

Write one Markdown report under `docs/research/` through the project-approved
non-serve CLI. The report is the retained artifact; capture files are temporary
and must be removed after close.

Do not leave a "revisit later" line without a card in Tower-enabled mode. In a
report-only run, cite an existing record when it covers the finding; otherwise
record the deferral with a concrete `no-action` reason.

## Report contract

Lead with the verdict. Include only sections relevant to the declared source,
subject, and outcome, plus:

- source coverage and limits;
- capture quality, retrieval warnings, and unresolved linked sources;
- strongest lessons and verified defects with live contrasts when run;
- corrections and disputed claims;
- Jet alignment and gaps, with shipped versus ratified-and-unbuilt state;
- avoid list and beat vectors when the outcome is comparative;
- agent-optimality and surface coverage when relevant;
- prioritized implement or avoid list and owner gates;
- Tower IDs when Tower mode created them;
- exact local file links and primary-source links.

Keep audience members anonymous unless identity materially affects credibility.
Avoid long quotations. End with one line on the strongest unverified
assumption when one remains.

## Failure guards

- Do not claim complete audience coverage when retrieval warned otherwise.
- Do not expose audience identities by default.
- Do not treat caption cleanup, OCR, or scraped-page cleanup as factual
  verification.
- Do not treat a README or abstract as behavior when code or an artifact is
  available.
- Do not merge claims only because wording resembles; use a reviewed shared
  topic and `source_identity`.
- Do not attribute compiler cost to type checking without phase data.
- Do not compare clean and incremental builds, CPU-summed and wall time, or debug
  and release profiles as equivalents.
- Do not solve backend cost through hidden allocation, silent de-optimization,
  or new surface syntax.
- Do not expose backend diagnostics as Jet user errors.
- Do not expand source or subject scope after the stop condition.
