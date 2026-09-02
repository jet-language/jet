# Probe prim-realtime — Real-time guarantees: deadlines, audio callbacks, jitter

Read `docs/research/domain-foundations/probes/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: games, embedded, gui.

## Build this

Build, as library Jet: a 48 kHz audio callback that must fill 256-sample buffers on time (use a real device if the host has one, else a timer-driven fake device), a 1 kHz control loop with a deadline check, and a report of missed deadlines and jitter over 10 seconds. Try to make the callback allocation-free and lock-free. Use examples/features/time/**, concurrency/**, memory/**, lowlevel/**.

## Answer these

1. Can a library author guarantee no allocation and no blocking on a hot path, and detect a missed deadline, or which runtime/compiler primitive is missing?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/acoustics-audio-engineering/`, `~/.cache/jet-luna/dx2/audio-music-production/`, `~/.cache/jet-luna/dx2/real-time-safety-critical/`, `~/.cache/jet-luna/dx2/robotics/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/prim-realtime/probe.md`, `gaps.json`, and the code under `~/.cache/jet-luna/dx3/prim-realtime/pkg/`. Gap ids start with `prim-realtime-G`.
