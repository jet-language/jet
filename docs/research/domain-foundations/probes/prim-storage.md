# Probe prim-storage — Durable storage, streams, and pools as libraries

Read `docs/research/domain-foundations/probes/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: backend, data, cli.

## Build this

Build, as library Jet: an append-only log with fsync and crash-safe recovery; a key-value store over it with a compaction step; a bounded connection pool with health checks; a durable queue with consumer groups and at-least-once delivery; and a content-addressed cache. Use examples/features/io/files.jet, byte_buffer.jet, db*.jet, stream.jet, concurrency/**. Crash the process mid-write (kill it) and prove recovery.

## Answer these

1. What does a library author lack for durability: fsync control, file locking, memory-mapped files, atomic rename?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/databases-storage-engines/`, `~/.cache/jet-luna/dx2/stream-processing/`, `~/.cache/jet-luna/dx2/data-engineering-etl/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/prim-storage/probe.md`, `gaps.json`, and the code under `~/.cache/jet-luna/dx3/prim-storage/pkg/`. Gap ids start with `prim-storage-G`.
