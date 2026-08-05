---
name: surface-frequency-audit
description: Audit how programming-language features, syntax, operators, semantic operations, built-ins, standard-library and third-party APIs, idioms, and tooling surfaces appear in real public code across languages, domains, project sizes, and experience levels. Use when Codex must produce one evidence-backed Markdown report that ranks common programming work and Jet friction, preserves beginner and expert paths, resumes safely across agents or usage limits, and cross-references Tower without writing to it.
---

# Surface Frequency Audit

Produce one readable report for the owner. Measure first. Recommend only after the evidence is stable.

## Non-negotiable bounds

- Write only the final Markdown report as the retained run artifact.
- Keep unfinished checkpoints under `.tmp/surface-frequency-audit/<run-id>/`.
- Remove that checkpoint directory only after final validation and report installation.
- Never create or change Tower cards, decisions, ballots, Tower docs, or board state. The final audit report is the only docs change.
- Read Tower only after the findings and rankings are nearly final.
- Do not run another audit or research skill. Use their files only as named prior art.
- Apply `.agents/skills/simple/SKILL.md` to the final report.
- Treat public source as evidence of written use, not runtime frequency or private production behavior.
- Never claim literal coverage of all code. State the declared scope and every coverage gap.

## Load the method

Read these files before starting:

1. [`references/method.md`](references/method.md) for the corpus, taxonomy, metrics, and ranking rules.
2. [`references/report-template.md`](references/report-template.md) for the final report contract.
3. [`../isomorphic-ontology-audit/ontology.md`](../isomorphic-ontology-audit/ontology.md) for the canonical category catalog.
4. [`../simple/SKILL.md`](../simple/SKILL.md) for owner-facing prose.

Read `AGENTS.md`. Search the current tree before broad reading. Preserve unrelated worktree changes.

## Start or resume a run

Use one stable run directory and one report target. Never keep progress only in chat or `/tmp`.

```sh
RUN=.tmp/surface-frequency-audit/YYYY-MM-DD
REPORT=docs/audits/surface-frequency-audit-YYYY-MM-DD.md

scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py init "$RUN" \
  --report "$REPORT" \
  --config .agents/skills/surface-frequency-audit/SKILL.md \
  --config .agents/skills/surface-frequency-audit/references/method.md \
  --config .agents/skills/surface-frequency-audit/references/report-template.md \
  --config .agents/skills/isomorphic-ontology-audit/ontology.md
```

The checkpoint tool also pins its checkpoint and aggregation scripts automatically.

If the run directory exists, resume it. Do not reinitialize it. Inspect progress:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py status "$RUN"
```

Stop if a saved config digest changed. Either restore the pinned method or start a new run. Never mix methods silently.

## Freeze the corpus

Build a stratified source manifest before counting features. Pin each repository or source set to an exact commit or content digest.

Record these facts for every source:

- Canonical source ID and URL
- Exact commit, tree, or content digest
- Language and version or dialect
- Domain
- Project stratum and task difficulty
- License and public-access status
- Inclusion or exclusion reason
- Fork, mirror, generated-code, vendor, and copied-code identity
- Parser or scanner name and version

Use the full baseline in `references/method.md`. Add relevant languages and domains when the baseline misses a material population. Mark weak or unavailable cells. Do not replace missing data with zero.

## Plan resumable work

Partition collection by `repository × language parser pass`. One pass emits all normalized feature measurements for that source. Keep semantic review and synthesis as later read-only stages.

Create `$RUN/inbox/catalog.json` from official language specifications and documentation. Record the full official section inventory. Map each section to measurement keys or give an unmatched reason. A second agent must compare the inventory with the official table of contents. The builder and reviewer IDs must differ.

Create a JSON list at `$RUN/inbox/units.json`. Each unit needs `id`, one `source_id`, `source_identity`, `catalog_id`, source provenance, `language`, `domain`, `stratum`, and `payload`. The checkpoint tool derives the expected measurements from the frozen catalog. Use stable IDs. Plan once:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py plan "$RUN" \
  "$RUN/inbox/units.json" --catalog "$RUN/inbox/catalog.json"
```

One agent owns each claimed unit. Agents may work in parallel only on different unit IDs. One close owner alone changes the final report draft.

Claim the next unit:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py next "$RUN" \
  --owner AGENT_ID --lease-hours 4
```

Checkpoint after each bounded source slice. Also checkpoint before a usage limit or handoff:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py checkpoint "$RUN" UNIT_ID \
  --owner AGENT_ID --cursor 'exact next source/file/range' \
  --note 'done / left / warnings' \
  --partial "$RUN/inbox/UNIT_ID.partial.json"
```

Expired leases may be reclaimed. A new agent must read the saved cursor, result files, and warnings before it continues.

## Collect evidence

- Use source code as the primary record of usage.
- Use official specifications and documentation to define language features and APIs.
- Prefer an official parser, compiler frontend, semantic index, or stable parser library.
- Label heuristic or text-only scans. Never merge them with symbol-resolved counts without a sensitivity table.
- Resolve API symbols when feasible. A matching name alone does not prove API identity.
- Keep generated, vendored, copied, fixture, example, benchmark, and test code in separate strata.
- Preserve unmatched syntax and unresolved API sites. They count against coverage.
- Record source sites for sampled validation. Do not dump whole repositories into the report.
- Record runtime telemetry only as a separate metric with its own source and population.

Each finished unit writes one JSON result under `$RUN/inbox/`. It must contain `schema`, `unit_id`, `source_ids`, `tool`, `coverage`, `measurements`, `citations`, and `warnings`. It must account for every frozen measurement key, including zero-use and `not-recorded` rows. See `references/method.md` for field meanings.

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/checkpoint.py complete "$RUN" UNIT_ID \
  --owner AGENT_ID --result "$RUN/inbox/UNIT_ID.result.json"
```

If evidence cannot be collected, mark the unit `blocked` or `unavailable` with an exact reason. Never call it complete.

## Normalize and rank

Map every item through two layers:

1. Language-agnostic concept, task, and operation.
2. Exact language surface, operator, syntax form, idiom, or resolved API.

Use ontology IDs. Extend the existing ontology only when its extension protocol requires it. Do not invent a parallel taxonomy.

Calculate every applicable metric in `references/method.md`. Every table must name its numerator, denominator, eligible opportunity, population, and coverage. Keep raw counts as support, not the headline rank.

Generate the repeatable base aggregation inside the checkpoint directory:

```sh
scripts/agent/jet-env python3 \
  .agents/skills/surface-frequency-audit/scripts/aggregate.py "$RUN" \
  --output "$RUN/analysis/aggregate.json"
```

Run the aggregate only after every unit is terminal. Review it before Jet-specific friction scoring. The checkpoint tool binds it to the final result-set digest. It produces the shared base metrics only. Use deterministic run-local analysis for sensitivity, trend, friction, and priority views that need corpus-specific data. Keep those scripts and outputs under `$RUN` so another agent can resume them. Do not hand-calculate headline totals.

Run these sensitivity views:

- Raw and deduplicated source
- Equal project, language, domain, and stratum weights
- Total-weighted counts
- Median and p90 project density
- Leave-one-project-out and leave-one-domain-out
- Symbol-resolved only versus all classified sites
- Alternative priority weights from `references/method.md`

Explain unstable ranks. Do not choose the view that best supports a preferred Jet change.

## Cross-check Jet and Tower

Do this only after evidence tables and recommendations are nearly final.

1. Search Jet specs, syntax decisions, source, examples, tests, and tooling for each top finding.
2. Read Tower through its CLI.
3. Classify each recommendation as `Covered`, `Partly covered`, `Not covered`, or `Conflicts with current plan`.
4. Cite exact card and decision IDs.
5. Make no Tower writes. Do not draft cards or ballots.

Read `plugins/tower/skills/tower/SKILL.md` before the read-only Tower pass.

Separate measured evidence from Jet-specific inference. Protect memory safety, type safety, clear diagnostics, and expert control. Shorter syntax does not win when explicit syntax buys those properties.

## Write and close the report

Follow `references/report-template.md`. Keep the main narrative below 4,000 words. Keep the executive summary below 800 words. Discuss at most 20 recommendations in full. Put complete coverage and long-tail tables in collapsed sections inside the same file.

Before installation:

1. Run checkpoint validation with `--require-complete`.
2. Reconcile raw totals with project, language, domain, and stratum aggregates.
3. Ask a fresh-context reviewer to sample source classifications, parser gaps, denominators, arithmetic, rank stability, Jet claims, Tower mappings, and report clarity.
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

The retained output is one Markdown report. Report the path, coverage, strongest limit, and review result. Do not create follow-up work.

## Failure guards

- Do not confuse source frequency with runtime frequency, importance, or approval.
- Do not let one monorepo, ecosystem, domain, or copied codebase dominate the rank.
- Do not compare raw glyph or token counts across languages as equivalent semantics.
- Do not hide parse failures, unsupported syntax, unresolved symbols, or unavailable sources.
- Do not treat absence as proof that users do not need an operation.
- Do not penalize safety, accessibility, clear errors, or expert escape hatches as ceremony.
- Do not retain a second report, ledger, manifest, or handoff file after successful closeout.
- Do not mutate Tower or translate recommendations into work unless the owner later asks.
