# Probe prim-capabilities — Capabilities, authority, and plugins

Read `$JET_STATE_ROOT/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: backend, cli, gui, games.

## Build this

Learn Jet's authority/effect model (Docs/spec/safety.md, Examples/features/effects/**, safety/**, packages/**, io/db_policy.jet, os_process_control.jet). Then build: a host program that loads a plugin (a Jet package, and a WASM module if the toolchain allows) with only the capabilities the host grants (read one directory, no network), a tool that opens a possibly-malicious file inside a capability boundary, and a library that exposes an OS service (a daemon with start/stop/status) under explicit authority. Record what a library can enforce and what only the runtime can.

## Answer these

1. Can a library author scope authority for code they load, or is that a runtime primitive?
2. Is a plugin ABI (versioned, capability-scoped) buildable today?

## Research to mine

Domain census and reports: `$JET_STATE_ROOT/dx2/wasm-plugins-sandboxing/`, `$JET_STATE_ROOT/dx2/security-tooling/`, `$JET_STATE_ROOT/dx2/digital-forensics/`, `$JET_STATE_ROOT/dx2/containers-orchestration/` (report.md, census.json, claims.json). Deleted ballots with worked code: `$JET_STATE_ROOT/dx2/ballots/` (grep the mechanism name). Family syntheses: `$JET_STATE_ROOT/dx2/_families/*/synthesis.md`.

## Output

`$JET_STATE_ROOT/dx3/prim-capabilities/probe.md`, `gaps.json`, and the code under `$JET_STATE_ROOT/dx3/prim-capabilities/pkg/`. Gap ids start with `prim-capabilities-G`.
