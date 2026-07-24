# Jet docs — where to look

One home per kind of document. Start here.

| If you want to… | Go to |
|---|---|
| Learn Jet in your first hour | [first-hour.md](first-hour.md) |
| Audit toolchain network and telemetry policy | [reference/network-policy.md](reference/network-policy.md) |
| Look up the standard library | [reference/core-library.md](reference/core-library.md) |
| Configure Jet through environment variables | [reference/environment.md](reference/environment.md) |
| Understand an error code | [reference/errors/](reference/errors/) (generated from snapshots) |
| Read embedded / freestanding builds | [reference/embedded.md](reference/embedded.md) |
| Read the versioning / release policy | [reference/versioning.md](reference/versioning.md) |
| Know the authoritative rules | [spec/](spec/) — see below |
| See durable plans | [plans/](plans/) |
| See live work, decisions, and blockers | [AGENTS.md](../AGENTS.md) → Tower |
| Review unexplored proposals | [proposals/](proposals/) |
| Find superseded research / old audits | [archive/](archive/) |
| Re-run improvement / competitive checks | [`.agents/skills/`](../.agents/skills/) |

## spec/ — the authoritative surface

These are binding. When they disagree with anything else, they win.

- [philosophy.md](spec/philosophy.md) — ranked priorities; settles arguments.
- [spec.md](spec/spec.md) — the living language spec (what exists today).
- [syntax-decisions.md](spec/syntax-decisions.md) — the owner's control surface;
  the **only** home for ratified syntax decisions. Syntax facts (what's ratified,
  what's retired, what's provisional) live here and nowhere else.
- [architecture.md](spec/architecture.md) — pipeline (lex → parse → sema →
  codegen) + rules R1–R12.
- [diagnostics.md](spec/diagnostics.md) — error voice + exact render format;
  snapshot-pinned.
- [roadmap.md](spec/roadmap.md) — what's active / not yet verified, plus Epoch 1 & 2 development highlights (completed work).
- Tower — the owner's live decision queue; ratified syntax lives in
  [syntax-decisions.md](spec/syntax-decisions.md).

## plans/ — implementation plans

Plans live in [`plans/`](plans/): durable program law, long-running product
master plans, and later architecture. Tower owns per-card plans and live
status. [`sidequests/`](sidequests/) retains only exceptional reviewed work not
yet folded into an epoch master plan. Unexplored ideas live in
[`proposals/`](proposals/); owner choices live only in Tower.
Epoch 1 & 2 highlights are in
[roadmap.md](spec/roadmap.md). See [plans/README.md](plans/README.md) for the
implementing-agent protocol.

## The dashboard — tasks, decisions, bugs, scratch

The single management surface: tasks with live pipeline status, every open
decision, bugs, and scratch. Start it using the canonical command in
[`AGENTS.md`](../AGENTS.md).
