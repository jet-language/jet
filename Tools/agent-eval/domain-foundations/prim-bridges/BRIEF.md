# Probe prim-bridges — Foreign libraries: bridges to C/Rust libraries

Read `$JET_STATE_ROOT/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: games, ai-ml, data, gui, science.

## Build this

Learn the FFI story (Docs/spec/architecture.md "Adding an FFI bridge", Examples/Interop/FFI/FFI.jet, CBind/, InlineC.jet, Examples/Interop/Languages/**). Then bridge, as a library author would: one C library with a handle-based API (SQLite is already bridged; pick zlib or libpng or FFmpeg's libavformat if installed), one header via `jet inspect bind cpp`, and one Rust crate via the extern rust bridge (note the sandbox rule). Read a file through each bridge from a Jet program and free everything.

## Answer these

1. How many lines of boilerplate per foreign function, and is the bridge safe by construction or by discipline?
2. Which formats (HDF5, NetCDF, Parquet, Arrow, video) are reachable today through bridges versus needing a core reader?

## Research to mine

Domain census and reports: `$JET_STATE_ROOT/dx2/video-vfx-compositing/`, `$JET_STATE_ROOT/dx2/image-processing/`, `$JET_STATE_ROOT/dx2/gis-geospatial/`, `$JET_STATE_ROOT/dx2/medical-imaging/`, `$JET_STATE_ROOT/dx2/climate-weather/` (report.md, census.json, claims.json). Deleted ballots with worked code: `$JET_STATE_ROOT/dx2/ballots/` (grep the mechanism name). Family syntheses: `$JET_STATE_ROOT/dx2/_families/*/synthesis.md`.

## Output

`$JET_STATE_ROOT/dx3/prim-bridges/probe.md`, `gaps.json`, and the code under `$JET_STATE_ROOT/dx3/prim-bridges/pkg/`. Gap ids start with `prim-bridges-G`.
