# Probe prim-distributed — Distributed and parallel execution as a library

Read `docs/research/domain-foundations/probes/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: backend, ai-ml, science.

## Build this

Build, as library Jet: a work-stealing parallel map over 16 cores; a two-process message-passing job (spawn workers, typed messages over sockets or pipes, collect results); a bounded channel pipeline with backpressure; and a keyed windowed stream (event time, watermarks) over an in-memory source with recovery from a checkpoint. Use examples/features/concurrency/**, streams/**, net/socket_echo.jet, io/process_builder.jet. Time the parallel map against Rayon or Python multiprocessing.

## Answer these

1. Which of parallel iteration, process spawning with typed channels, and checkpointed streams need runtime support beyond today's concurrency primitives?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/distributed-systems/`, `~/.cache/jet-luna/dx2/hpc-parallel/`, `~/.cache/jet-luna/dx2/stream-processing/`, `~/.cache/jet-luna/dx2/messaging-eventing/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/prim-distributed/probe.md`, `gaps.json`, and the code under `~/.cache/jet-luna/dx3/prim-distributed/pkg/`. Gap ids start with `prim-distributed-G`.
