# Probe area-data — Data analysis

Read `~/.cache/jet-luna/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: data.

## Build this

A data-analysis session as one Jet package plus a notebook: load a 100k-row CSV and a Parquet file into the core table type; filter, group, join, pivot, window; descriptive statistics and a linear regression with a typed result; a labeled 2-D array with coordinates; a plot exported to SVG and PNG; a notebook (`jet notebook`) with reactive cells that re-run on change; and a receipt for the run. Compare timing against pandas/polars on the same CSV for one group-by. Start from examples/features/collections/**, examples/features/io/db*.jet and stream.jet, examples/features/math/**, examples/features/serde/**, and `jet inspect facts` for core.data.

## Answer these

1. Which table operations does a library author lack today, and are they library code or do they need the shared table type to change?
2. Is plotting buildable as a library (drawing primitives to SVG/PNG)?
3. Where is the notebook loop short of Jupyter/Observable?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/statistics/`, `~/.cache/jet-luna/dx2/data-engineering-etl/`, `~/.cache/jet-luna/dx2/big-data-analytics/`, `~/.cache/jet-luna/dx2/bi-dashboards/`, `~/.cache/jet-luna/dx2/spreadsheets-business-logic/`, `~/.cache/jet-luna/dx2/numerical-computing/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/area-data/probe.md`, `gaps.json`, `batteries.json`, and the code under `~/.cache/jet-luna/dx3/area-data/pkg/`. Gap ids start with `area-data-G`.
