# Probe prim-tooling-hooks — Tooling hooks: syntax trees, build cache, replay, load runner

Read `docs/research/domain-foundations/probes/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: cli, web, backend.

## Build this

As a tool author: read a Jet file's lossless syntax tree through whatever `jet inspect` exposes and write a formatter-safe rename over it; query the build graph (`jet inspect explain-build`, build receipts) to implement a "what changed" report; replay a recorded run headlessly; and write a load runner that drives an HTTP service with a deterministic threshold. Use examples/features/tooling/**, reflection/**, devloop/**, and `jet --help`.

## Answer these

1. Which tool-author needs are met by exposed facts and which need a new exposed seam (syntax tree with spans, build graph API, replay API)?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/compilers-language-tooling/`, `~/.cache/jet-luna/dx2/editor-ide-extensions/`, `~/.cache/jet-luna/dx2/build-systems-monorepo/`, `~/.cache/jet-luna/dx2/testing-qa-automation/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/prim-tooling-hooks/probe.md`, `gaps.json`, and the code under `~/.cache/jet-luna/dx3/prim-tooling-hooks/pkg/`. Gap ids start with `prim-tooling-hooks-G`.
