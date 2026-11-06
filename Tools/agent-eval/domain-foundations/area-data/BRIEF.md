# Probe area-data — Data analysis

Read `$JET_STATE_ROOT/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: data.

## Build this

A data-analysis session as one Jet package plus a notebook: load a 100k-row CSV and a Parquet file into the core table type; filter, group, join, pivot, window; descriptive statistics and a linear regression with a typed result; a labeled 2-D array with coordinates; a plot exported to SVG and PNG; a notebook (`jet notebook`) with reactive cells that re-run on change; and a receipt for the run. Compare timing against pandas/polars on the same CSV for one group-by. Start from Examples/features/collections/**, Examples/features/io/db*.jet and stream.jet, Examples/features/math/**, Examples/features/serde/**, and `jet inspect facts` for core.data.

## Answer these

1. Which table operations does a library author lack today, and are they library code or do they need the shared table type to change?
2. Is plotting buildable as a library (drawing primitives to SVG/PNG)?
3. Where is the notebook loop short of Jupyter/Observable?

## Research to mine

Domain census and reports: `$JET_STATE_ROOT/dx2/statistics/`, `$JET_STATE_ROOT/dx2/data-engineering-etl/`, `$JET_STATE_ROOT/dx2/big-data-analytics/`, `$JET_STATE_ROOT/dx2/bi-dashboards/`, `$JET_STATE_ROOT/dx2/spreadsheets-business-logic/`, `$JET_STATE_ROOT/dx2/numerical-computing/` (report.md, census.json, claims.json). Deleted ballots with worked code: `$JET_STATE_ROOT/dx2/ballots/` (grep the mechanism name). Family syntheses: `$JET_STATE_ROOT/dx2/_families/*/synthesis.md`.

## Output

`$JET_STATE_ROOT/dx3/area-data/probe.md`, `gaps.json`, `batteries.json`, and the code under `$JET_STATE_ROOT/dx3/area-data/pkg/`. Gap ids start with `area-data-G`.
