# Probe prim-arrays — Labeled, chunked, and image arrays over the core tensor

Read `docs/research/domain-foundations/probes/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: data, ai-ml, science.

## Build this

Build, as library Jet over the core compute/tensor substrate: a labeled N-D array with named dimensions and coordinate lookup; a chunked lazy array that reads 8 chunks on demand and maps a function without loading everything; and an image type (width, height, channels, dtype) with resize and a color-space convert. Time a 4k x 4k float map against numpy/xarray.

## Answer these

1. Does the shared array type give a library author views, strides, dtype generics, and lazy plans, or which primitive is missing?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/climate-weather/`, `~/.cache/jet-luna/dx2/remote-sensing/`, `~/.cache/jet-luna/dx2/image-processing/`, `~/.cache/jet-luna/dx2/oceanography-hydrology/`, `~/.cache/jet-luna/dx2/medical-imaging/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/prim-arrays/probe.md`, `gaps.json`, and the code under `~/.cache/jet-luna/dx3/prim-arrays/pkg/`. Gap ids start with `prim-arrays-G`.
