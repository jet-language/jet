# Surface frequency audit operations

Load this reference after the root router selects the run shape. The
checkpoint and aggregation scripts are executable authorities. This reference
only points to their interfaces and states the evidence contract.

## Select the run shape

- **Full audit:** use the complete baseline and feature catalog in
  [`method.md`](method.md). Cover every requested language, dialect, adjacent
  declarative surface, domain, project stratum, task difficulty, and catalog
  measurement. Keep the denominator, opportunity, and coverage rules unchanged.
- **Focused audit:** use only the owner-declared language, domain, workload,
  source set, or surface question. Record the exact scope and every omitted
  cell or metric. Do not call a focused result a full-corpus result. Use the
  same source identity, denominator, provenance, resume, and missing-evidence
  rules for the cells that are in scope.

Do not silently turn a full request into a focused run. If the owner changes
the requested scope or policy, reconcile it before collection. An initialized
run pins its normative inputs; a changed input requires restoration or a new
run, with the change stated rather than hidden.

## Start or resume

Use one stable run directory and one report target. Never keep progress only in
chat or `/tmp`.

```sh
RUN=.tmp/surface-frequency-audit/YYYY-MM-DD
REPORT=docs/audits/surface-frequency-audit-YYYY-MM-DD.md

scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py init "$RUN" \
  --report "$REPORT" \
  --config AGENTS.md \
  --config .agents/skills/surface-frequency-audit/SKILL.md \
  --config .agents/skills/surface-frequency-audit/references/method.md \
  --config .agents/skills/surface-frequency-audit/references/report-template.md \
  --config .agents/skills/surface-frequency-audit/references/operations.md \
  --config .agents/skills/isomorphic-ontology-audit/ontology.md \
  --config .agents/skills/simple/SKILL.md \
  --config .agents/skills/_shared/standing-lens.md \
  --config .agents/skills/_shared/audit-dispositions.md
```

The checkpoint tool also pins its checkpoint and aggregation scripts. The
`run.json` config list is the run's pinned normative input list. It includes the
full method, report contract, operations, ontology, prose and shared-policy
inputs above, not only the files loaded during the first stage.

If the run directory exists, resume it. Do not reinitialize it. Inspect
progress:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py status "$RUN"
```

Stop if a saved config digest changed. Restore the pinned input or start a new
run. If owner policy changed, record the active policy and reconcile the run;
never mix policies silently.

## Freeze the corpus

Build a stratified source manifest before counting features. Pin each repository
or source set to an exact commit or content digest. Record for every source:

- canonical source ID and URL;
- exact commit, tree, or content digest;
- language and version or dialect;
- domain, project stratum, and task difficulty;
- license and public-access status;
- inclusion or exclusion reason;
- fork, mirror, generated-code, vendor, and copied-code identity;
- parser or scanner name and version.

Use the full baseline in `method.md` for a full audit. Add relevant languages or
domains only when the declared scope or a material population requires them.
Mark weak or unavailable cells. Never replace missing data with zero.

## Plan resumable work

Partition collection by `repository × language parser pass`. One pass emits all
normalized feature measurements for that source. Keep semantic review and
synthesis as later read-only stages.

For a full audit, create `$RUN/inbox/catalog.json` from official language
specifications and documentation. Record the full official section inventory;
map each section to measurement keys or give an unmatched reason. A second
agent compares the inventory with the official table of contents. Builder and
reviewer IDs must differ. For a focused audit, create the same catalog for every
language or dialect in scope and record the excluded official sections as out
of scope, not as zero-use measurements.

Create `$RUN/inbox/units.json`. Each unit needs `id`, one `source_id`,
`source_identity`, `catalog_id`, source provenance, `language`, `domain`,
`stratum`, and `payload`. The checkpoint tool derives expected measurements
from the frozen catalog. Use stable IDs. Plan once:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py plan "$RUN" \
  "$RUN/inbox/units.json" --catalog "$RUN/inbox/catalog.json"
```

One agent owns each claimed unit. Agents may work in parallel only on different
unit IDs. One close owner alone changes the final report draft.

Claim the next unit:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py next "$RUN" \
  --owner AGENT_ID --lease-hours 4
```

Checkpoint after each bounded source slice and before a usage limit or handoff:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py checkpoint "$RUN" UNIT_ID \
  --owner AGENT_ID --cursor 'exact next source/file/range' \
  --note 'done / left / warnings' \
  --partial "$RUN/inbox/UNIT_ID.partial.json"
```

Expired leases may be reclaimed. A new agent reads the saved cursor, result
files, and warnings before continuing.

## Collect evidence

Use source code as the primary record of written use. Use official
specifications and documentation to define features and APIs. Prefer an
official parser, compiler frontend, semantic index, or stable parser library.
Label heuristic or text-only scans. Never merge them with symbol-resolved
counts without a sensitivity table.

Resolve API symbols when feasible. A matching name alone does not prove API
identity. Keep generated, vendored, copied, fixture, example, benchmark, and
test code in separate strata. Preserve unmatched syntax and unresolved API
sites; they count against coverage. Record sampled source sites. Keep runtime
telemetry separate with its source and population.

Each finished unit writes one JSON result under `$RUN/inbox/` with `schema`,
`unit_id`, `source_ids`, `tool`, `coverage`, `measurements`, `citations`, and
`warnings`. Account for every frozen measurement key, including zero-use and
`not-recorded` rows. See `method.md` for field meanings.

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py complete "$RUN" UNIT_ID \
  --owner AGENT_ID --result "$RUN/inbox/UNIT_ID.result.json"
```

If evidence cannot be collected, close the unit as a gap. Use the executable
`block` command, not an invented status:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py block "$RUN" UNIT_ID \
  --owner AGENT_ID --reason 'exact blocking reason'
```

Add `--unavailable` when no sound public sample exists. A blocked or unavailable
unit is not complete and must be named in the retained report.

## Normalize and rank

Map each item through two layers:

1. language-agnostic concept, task, and operation;
2. exact language surface, operator, syntax form, idiom, or resolved API.

Use ontology IDs. Extend the existing ontology only through its extension
protocol. Do not invent a parallel taxonomy.

Calculate every applicable metric in `method.md`. Every table names its
numerator, denominator, eligible opportunity, population, and coverage. Keep
raw counts as support, not as the headline rank. A denominator never includes
an opportunity that was not eligible, and unavailable evidence is not zero.

After every unit is terminal, generate the repeatable base aggregation inside
the checkpoint directory:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/aggregate.py "$RUN" \
  --output "$RUN/analysis/aggregate.json"
```

Review the aggregate before Jet-specific friction scoring. The checkpoint tool
binds it to the final result-set digest. Use deterministic run-local analysis
for sensitivity, trend, friction, and priority views. Do not hand-calculate
headline totals.

Run the required sensitivity views:

- raw and deduplicated source;
- equal project, language, domain, and stratum weights;
- total-weighted counts;
- median and p90 project density;
- leave-one-project-out and leave-one-domain-out;
- symbol-resolved only versus all classified sites;
- alternative priority weights from `method.md`.

Explain unstable ranks. Do not choose the view that best supports a preferred
Jet change.

## Cross-check and close

Only after evidence tables and recommendations are nearly final:

1. Search Jet specs, syntax decisions, source, examples, tests, and tooling for
each top finding.
2. Read Tower through its CLI.
3. Classify each recommendation as `Covered`, `Partly covered`, `Not covered`,
or `Conflicts with current plan`.
4. Cite exact card and decision IDs.
5. Make no Tower writes. Do not draft cards or ballots.

Read `plugins/tower/skills/tower/SKILL.md` only for this late read-only pass.
Separate measured evidence from Jet-specific inference. Protect memory safety,
type safety, clear diagnostics, and expert control. Shorter syntax does not win
when explicit syntax buys those properties.

At report stage, load `references/report-template.md` and
`.agents/skills/simple/SKILL.md`. Do not make either a mandatory prose read
before collection. Follow the template's required finding-disposition marker
and include the declared scope, denominator, coverage, provenance, and limits.
Keep long tables inside the same report.

Before installation:

1. Run checkpoint validation with `--require-complete`.
2. Reconcile raw totals with project, language, domain, and stratum aggregates.
3. Ask a fresh-context reviewer to sample source classifications, parser gaps,
denominators, arithmetic, rank stability, Jet claims, Tower mappings, and report
clarity.
4. Fix concrete findings.
5. Re-run validation.

Install the reviewed draft atomically:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py install "$RUN" \
  "$RUN/report.tmp.md"
```

Then remove only the completed run directory:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py clean "$RUN"
```

The retained output is one Markdown report. Report its path, declared scope,
coverage, strongest limit, and review result. Do not create follow-up work.

## Failure guards

- Do not confuse written source frequency with runtime frequency, importance, or
  approval.
- Do not let one monorepo, ecosystem, domain, or copied codebase dominate rank.
- Do not compare raw glyph or token counts across languages as equivalent
  semantics.
- Do not hide parse failures, unsupported syntax, unresolved symbols, blocked or
  unavailable sources, or omitted focused cells.
- Do not treat absence as proof that users do not need an operation.
- Do not penalize safety, accessibility, clear errors, or expert escape hatches
  as ceremony.
- Do not retain a second report, ledger, manifest, or handoff file after
  successful closeout.
- Do not mutate Tower or translate recommendations into work. This method is
  report-only, including for focused runs.
