# Gauntlet Run

Use this playbook only for Run mode. Read
[`shared.md`](shared.md) first.

## Execute and compare

1. Execute the harness over every entry. Verify expected output before timing.
   Wrong output disqualifies the cell and is a failure finding.
2. Compare paired real programs in the same run. Compute each matched-cell and
   metric ratio independently. Never compare against a previous run's numbers.
3. Apply the global performance law to every matched cell and metric:
   - A Jet ratio `<1.00` against each peer is a strict win.
   - Every non-Rust peer requires `<1.00`; a ratio `>=1.00` is a loss.
   - Rust alone permits parity at `<=1.05`. The `1.05` ceiling rejects
     measurement noise; it is not a target or win. A Rust ratio `>1.05` is a
     loss.
   - Unmeasured, invalid, missing, or wrong-output data is a failure. It is not
     a win, parity, or exclusion.
4. List every matrix cell with no entry as **uncovered territory**. Unmeasured
   is not winning.

## Advisory rows

Collect readability proxies, RLI5 rubric scores, and authoring cost as advisory
rows. For RLI5, use one Luna max subagent per persona: true beginner by default,
plus switcher, domain expert, and unattended agent. Apply the global `rli5`
skill to Jet and port sources with the same modify/derive probes. Weight each
stumble by path commonality, using `surface-frequency-audit` data when it exists
and an honest estimate otherwise. Common-path stumbles are findings; rare-path
friction may be acceptable when labeled. Advisory rows never gate a result.

## Report

Write one table-led scoreboard under `docs/audits/` through the
project-approved non-serve CLI. Honor the requested format; use HTML only when
the owner explicitly asks for HTML or an interactive page. Show matrix cells
× metrics with observed wins, parity, losses, and failures. Put losses and
failures first, but
do not invent a loss: a zero-loss report is valid when the required coverage is
complete and every result is valid. Show each peer-specific ceiling and separate
strict wins from Rust-only measurement-noise parity.

Include the beat table: where Jet wins categorically, what mechanism creates the
win, and what a peer would have to break to match it. Mark shipped versus
ratified-but-unbuilt rows. Apply `simple` prose rules. Read
`.agents/skills/_shared/audit-dispositions.md` before publication and include
its required finding-disposition table.

## Ratchet and completion

Historical result files and reports are immutable evidence. Never rewrite an
older artifact to make a new threshold appear green. Record the active policy
with each run and explain policy changes.

For every loss or failure—including an unmeasured, invalid, missing, or
wrong-output required row—deduplicate against existing Tower cards, then
auto-mint or update a card with the entry, metric, ratio or failure, both
sources, and policy. Each loss or failure blocks its owning performance card,
milestone, and release gate until a fresh valid run satisfies the law. It
remains carded after repair. Every uncovered cell worth filling mints or updates
a build-mode corpus card. The shared disposition table does not weaken these
obligations.

Run mode is complete only after the report is written, required coverage and
output checks are accounted for, advisory rows are labeled, and the ratchet
obligations are satisfied.
