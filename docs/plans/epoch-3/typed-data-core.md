# Typed Data Core Plus Accelerator Bridges

## Goal

Card #237 turns D-WD9 into an Epoch 3 plan: Core owns a typed data floor for tables, series, statistics, and plotting basics. Python, R, and GPU bridges accelerate gaps during the transition, but every bridge-heavy workflow must expose native replacement status so Jet does not become a thin wrapper around another ecosystem.

The first user story is a single-file analysis: read CSV into a typed row, group and summarize it, print or plot the result, and keep every error in Jet terms.

## Current law

- D-WD9 ratifies the product direction: typed Core data floor plus accelerator bridges.
- D-CORENS1 requires first-party libraries under `core.*`.
- D-DATA-SURFACE1 ratifies the `core.data` facade for tables, series, stats, CSV, and plots.
- D-DATA-BRIDGE1 ratifies direct bridge roots (`py.*`, `r.*`, `gpu.*`) instead of nesting bridge providers under `core.data`.
- D-DATA-STATUS1 ratifies machine-readable status through the Core API plus the future `jet dossier data` human lens.
- D-DATA-PLOT1 ratifies deterministic SVG plus text plotting output.
- D-DEP1 allows crate-backed capability only as Jet packages wrapping pinned sources through `extern rust`, with a native-ize obligation.
- I2 and I3 require Jet front-end diagnostics and sema-owned checking; bridge failures must not leak host-language tracebacks as primary user errors.
- I5 requires examples plus expected output for every shipped feature.

Exact public module names, bridge provider prefixes, accelerator status UI, and plotting backend policy are ratified for the first slice.

## Vertical slices

1. Typed table floor: `Table<Row>`-style internal model, CSV load path, typed row projection, column lookup diagnostics, and one golden example that groups and counts rows.
2. Series and summarize: typed numeric/text series, `count`, `sum`, `mean`, `min`, `max`, and percentile basics, with compile-time rejection for stats on unsupported field types.
3. Join and filter: typed key equality, missing-key diagnostics, stable output ordering for tests, and a small telemetry join example.
4. Plot floor: first-party plot data model and a text or image backend selected by a ratified backend policy; CI proves deterministic output.
5. Accelerator bridge status: Python/R/GPU call sites report which steps are native, bridged, or missing; bridge errors include the Jet trigger site and a native replacement hint when one exists.
6. Native replacement audit: tooling lists bridge usage by domain, sorted by user-facing impact, without inventing a command spelling until ratified.

## Acceptance tests

- Golden example: CSV load, group, summarize, and print deterministic output.
- UI snapshots: missing file, malformed CSV row, unknown column, non-numeric stat, join key type mismatch, bridge unavailable, and bridge returned wrong shape.
- Core registry test: the data modules appear under `core.*` only and obey D-CORENS1.
- I6 guard: compiler crates remain dependency-free; any bridge dependency lives outside `Source/` and carries owner approval if it becomes a new Core external dependency.
- Generated-code check: no host-language error is the primary user message.
- Bridge-status test: a mixed native/accelerated workflow emits machine-readable status for every accelerated step.

## Dependency order

1. Decide public surface: module names, table/series API names, bridge provider names, and native-status reporting surface.
2. Land the typed table model and CSV vertical slice.
3. Add series/stat summaries over the same table model.
4. Add joins and filters.
5. Add plotting data model and one deterministic backend.
6. Add accelerator bridges and native replacement status.

## Ratified owner ballots

- D-DATA-SURFACE1=A: one `core.data` facade.
- D-DATA-BRIDGE1=A: direct provider roots.
- D-DATA-STATUS1=A: Core status API plus `jet dossier data`.
- D-DATA-PLOT1=A: deterministic SVG plus text fallback.

## Adversarial tradeoffs

- Safety first: typed rows and typed columns beat dynamic dataframe convenience; a misspelled column must be a Jet diagnostic, not a runtime foreign exception.
- Beginner experience: single-file CSV-to-summary must work without manifests or bridge setup. Accelerators stay optional and explain themselves only when used.
- Runtime performance: native Core operations own the hot path; bridge calls are explicit enough to audit and replace.
- One mechanical path: `core.data` must not duplicate `core.db` or SQL blocks. Tables are in-memory typed data; database access remains `core.db`.
- Ecosystem breadth: bridges are a conquest path, not the identity. Every bridge slice needs a visible path toward native Jet capability.
