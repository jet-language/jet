# Jet docs — where to look

One home per kind of document. Start here.

| If you want to… | Go to |
|---|---|
| Look up the standard library | [reference/stdlib.md](reference/stdlib.md) |
| Understand an error code | [reference/errors/](reference/errors/) (generated from snapshots) |
| Read embedded / freestanding builds | [reference/embedded.md](reference/embedded.md) |
| Read the versioning / release policy | [reference/versioning.md](reference/versioning.md) |
| Know the authoritative rules | [spec/](spec/) — see below |
| See what's planned or in progress | [plans/](plans/) — or run the dashboard (below) |

## spec/ — the authoritative surface

These are binding. When they disagree with anything else, they win.

- [philosophy.md](spec/philosophy.md) — ranked priorities; settles arguments.
- [spec.md](spec/spec.md) — the living language spec (what exists today).
- [syntax-decisions.md](spec/syntax-decisions.md) — the owner's control surface;
  the **only** home for ratified syntax decisions.
- [architecture.md](spec/architecture.md) — pipeline (lex → parse → sema →
  codegen) + rules R1–R7.
- [diagnostics.md](spec/diagnostics.md) — error voice + exact render format;
  snapshot-pinned.
- [roadmap.md](spec/roadmap.md) — what's active / not yet verified (completed work lives in epoch plans).
- [decision-ballots.md](spec/decision-ballots.md) — the owner's open decision
  queue (ratified items live in syntax-decisions.md).

## plans/ — implementation plans

Active epoch plans ([epoch-2/](plans/epoch-2/), [epoch-3/](plans/epoch-3/)), the
[jetpack & jetos](plans/jetpack-jetos/README.md) track (package manager + OS), and
[sidequests/](plans/sidequests/) (one reviewed plan per in-flight task, deleted
once shipped). See [plans/README.md](plans/README.md) for the implementing-agent
protocol.

## The dashboard — tasks, decisions, bugs, scratch

The single management surface: tasks with live pipeline status, every open
decision (grouped so nothing's hidden), bugs, and a scratch pad.

```
nix develop -c node tools/pipeline/pipeline.mjs serve --open
```
